use std::io;
use std::iter;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use atomic::Atomic;
use audioadapter_buffers::direct::InterleavedSlice;
use bytemuck::NoUninit;
use rubato::{Fft, FixedSync, Resampler};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::errors::Error as symphonia_Error;
use symphonia::core::io::{MediaSourceStream, ReadOnlySource};

use crate::asset::AssetFileBox;
use crate::audio::{AudioInput, AudioSource, AudioSourceState};
use crate::circbuf::{self, Receiver};

const BUF_LEN: u16 = 3; // [s]
const RATE_CONV_CHUNK: usize = 1024;

pub struct AudioFile {
    asset_file: AssetFileBox,
    inner: InnerRc,
}

impl AudioFile {
    pub fn new(asset_file: AssetFileBox) -> (Self, AudioFileHandle) {
        let inner = Inner {
            state: Atomic::new(State::Paused),
            at_eof: AtomicBool::new(false),
        };
        let inner_rc = Arc::new(inner);

        let input = Self {
            asset_file,
            inner: Arc::clone(&inner_rc),
        };

        let handle = AudioFileHandle::new(inner_rc);

        (input, handle)
    }

    fn build(self, channels: u16, sample_rate: u32) -> AudioFileSource {
        let rx = Self::build_impl(self.asset_file, channels, sample_rate);
        AudioFileSource::new(self.inner, rx)
    }

    fn build_impl(asset_file: AssetFileBox, channels: u16, sample_rate: u32) -> Receiver<f32> {
        let channels = channels as usize;

        // Setup circular buffer.

        let len = channels * sample_rate as usize * BUF_LEN as usize;
        let (tx, rx) = circbuf::circbuf::<f32>(len);

        // Open audio file.

        let read = match asset_file.read() { // TODO: Report error on UI
            Ok(read) => read,
            Err(_) => return rx,
        };

        let src = ReadOnlySource::new(read);
        let mss = MediaSourceStream::new(Box::new(src), Default::default());
        let probe = match symphonia::default::get_probe().format(&Default::default(), mss, &Default::default(), &Default::default()) { // TODO: Report error on UI
            Ok(probe) => probe,
            Err(_) => return rx,
        };
        let mut format = probe.format;

        let track = match format.default_track() { // TODO: Report error on UI
            Some(track) => track,
            None => return rx,
        };
        let mut decoder = match symphonia::default::get_codecs().make(&track.codec_params, &Default::default()) { // TODO: Report error on UI
            Ok(decoder) => decoder,
            Err(_) => return rx,
        };
        let track_id = track.id;

        let codec_params = decoder.codec_params();
        if codec_params.channels.unwrap().count() != channels { // TODO: Report error on UI
            return rx;
        }
        let decoder_sample_rate = codec_params.sample_rate.unwrap();

        // Determine, if we need rate conversion.

        let rate_conv_opt = if decoder_sample_rate != sample_rate {
            let rate_conv = Fft::<f32>::new(decoder_sample_rate as usize, sample_rate as usize, RATE_CONV_CHUNK, 1, channels, FixedSync::Both).expect("Unable to create sample rate converter");
            Some(rate_conv)
        } else {
            None
        };

        // Start decoder thread. At the end, the decoded data should be converted
        // to interleaved samples, since this is the format expected by the audio engine.

        thread::spawn(move || {
            let mut do_decode = || {
                loop {
                    let packet = match format.next_packet() {
                        Ok(packet) => packet,
                        Err(err) => match err {
                            symphonia_Error::IoError(err) => {
                                if err.kind() == io::ErrorKind::UnexpectedEof {
                                    break None;
                                }

                                break None; // TODO: Report error on UI
                            },
                            _ => {
                                break None; // TODO: Report error on UI
                            }
                        }
                    };

                    while !format.metadata().is_latest() {
                        format.metadata().pop();
                    }

                    if packet.track_id() != track_id {
                        continue;
                    }

                    let decoded = match decoder.decode(&packet) { // TODO: Report error on UI
                        Ok(decoded) => decoded,
                        Err(_) => break None,
                    };
                    let decoded_len = decoded.frames();

                    if decoded_len == 0 {
                        continue;
                    }

                    let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
                    buf.copy_interleaved_ref(decoded);

                    break Some((buf, decoded_len));
                }
            };

            if let Some(mut rate_conv) = rate_conv_opt {
                // Rate conversion is needed.

                let in_max = rate_conv.input_frames_max();
                let out_max = rate_conv.output_frames_max();
                let mut in_buf = Self::create_rate_conv_buf(channels, in_max);
                let mut out_buf = Self::create_rate_conv_buf(channels, out_max);

                let mut decoded_buf_opt = None;
                let mut in_i = 0;
                let mut end = false;

                while !end { // Process entire input.
                    // Collect the required number of input frames for rate converter.

                    let in_next = rate_conv.input_frames_next();

                    loop {
                        let mut todo = in_next - in_i;
                        if todo == 0 {
                            break;
                        }

                        if decoded_buf_opt.is_none() {
                            match do_decode() {
                                Some((buf, len)) => {
                                    let decoded_buf = DecodedBuf {
                                        buf,
                                        i: 0,
                                        len,
                                    };

                                    decoded_buf_opt = Some(decoded_buf);
                                },
                                None => {
                                    // At EOF, fill remaining input with silence.

                                    let in_buf_sl = &mut in_buf[(in_i * channels)..];
                                    in_buf_sl.fill(0.0);

                                    in_i += todo;
                                    end = true;
                                    break;
                                },
                            }
                        }

                        // Copy from decoded_buf into in_buf: it is needed, since the decoder and the rate converter have
                        // different buffer sizes.

                        let decoded_buf = decoded_buf_opt.as_mut().unwrap();
                        todo = (decoded_buf.len - decoded_buf.i).min(todo);
                        assert!(todo > 0);

                        let decoded_buf_sl = &decoded_buf.buf.samples()[(decoded_buf.i * channels)..((decoded_buf.i + todo) * channels)];
                        let in_buf_sl = &mut in_buf[(in_i * channels)..((in_i + todo) * channels)];
                        in_buf_sl.copy_from_slice(decoded_buf_sl);

                        // Consume decode buffer.

                        decoded_buf.i += todo;
                        if decoded_buf.i == decoded_buf.len {
                            decoded_buf_opt = None;
                        }

                        in_i += todo;
                    }

                    assert!(in_i == in_next);

                    // Do rate conversion. We can't create adapters before the while loop, since:
                    // - in_buf: they don't support memcpy (see copy_from_slice above).
                    // - out_buf: not possible to obtain a reference to the internal buffer (see send below).

                    let in_adapter = Self::create_rate_conv_adapter(&mut in_buf, channels, in_max); // TODO: We don't need mutability for in_buf.
                    let mut out_adapter = Self::create_rate_conv_adapter(&mut out_buf, channels, out_max);
                    let (in_rd, out_wr) = rate_conv.process_into_buffer(&in_adapter, &mut out_adapter, None).expect("Unable to do rate conversion");

                    // Consume input buffer.

                    if (1..in_i).contains(&in_rd) {
                        in_buf.copy_within((in_rd * channels)..(in_i * channels), 0);
                    }

                    in_i -= in_rd;

                    // Send output.

                    if !tx.send(&out_buf[..(out_wr * channels)]) {
                        break;
                    }
                }
            } else {
                // Rate conversion is not needed.

                while let Some((buf, _)) = do_decode() {
                    if !tx.send(buf.samples()) {
                        break;
                    }
                }
            }
        });

        // Wait until the buffer is full.

        rx.wait_full();

        rx
    }

    fn create_rate_conv_buf(channels: usize, len: usize) -> Box<[f32]> {
        Box::from_iter(iter::repeat_n(0.0_f32, channels * len))
    }

    fn create_rate_conv_adapter(buf: &mut [f32], channels: usize, len: usize) -> InterleavedSlice<&mut [f32]> {
        InterleavedSlice::new_mut(buf, channels, len).expect("Unable to create adapter")
    }
}

type InnerRc = Arc<Inner>;

struct Inner {
    state: Atomic<State>,
    at_eof: AtomicBool,
}

#[repr(C)]
#[derive(Clone, Copy, NoUninit)]
enum State {
    Paused,
    Playing,
    Drop,
}

struct DecodedBuf {
    buf: SampleBuffer::<f32>,
    i: usize,
    len: usize,
}

impl AudioInput for AudioFile {
    type Source = AudioFileSource;

    fn build(self, channels: u16, sample_rate: u32) -> Self::Source {
        self.build(channels, sample_rate)
    }
}

pub struct AudioFileSource {
    inner: InnerRc,
    rx: Receiver<f32>,
}

impl AudioFileSource {
    fn new(inner: InnerRc, rx: Receiver<f32>) -> Self {
        Self {
            inner,
            rx,
        }
    }
}

impl AudioSource for AudioFileSource {
    fn get_samples(&mut self, buf: &mut [f32]) -> AudioSourceState {
        match self.inner.state.load(Ordering::Relaxed) {
            State::Paused => {
                AudioSourceState::Paused
            },
            State::Playing => {
                let len = self.rx.recv(buf); // TODO: It should never block.

                if len == 0 {
                    self.inner.at_eof.store(true, Ordering::Relaxed);
                    return AudioSourceState::Drop;
                } else if len < buf.len() { // TODO: Do this one in engine?
                    // At EOF, pad with silence.

                    buf[len..].fill(0.0);
                }

                AudioSourceState::Playing
            },
            State::Drop => {
                AudioSourceState::Drop
            },
        }
    }
}

pub struct AudioFileHandle {
    inner: InnerRc,
}

impl AudioFileHandle {
    fn new(inner: InnerRc) -> Self {
        Self {
            inner,
        }
    }

    pub fn at_eof(&self) -> bool {
        self.inner.at_eof.load(Ordering::Relaxed)
    }

    pub fn play(&self) {
        self.inner.state.store(State::Playing, Ordering::Relaxed);
    }

    #[allow(dead_code)] // TODO: remove dead_code once it is used
    pub fn pause(&self) {
        self.inner.state.store(State::Paused, Ordering::Relaxed);
    }
}

impl Drop for AudioFileHandle {
    fn drop(&mut self) {
        self.inner.state.store(State::Drop, Ordering::Relaxed);
    }
}
