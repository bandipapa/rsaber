use std::io;
use std::iter;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;

use atomic_enum::atomic_enum;
use rubato::{FftFixedInOut, Resampler};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::errors::Error as symphonia_Error;
use symphonia::core::io::{MediaSourceStream, ReadOnlySource};

use crate::AssetManagerRc;
use crate::audio::{AudioFactory, AudioSource, AudioSourceState, AudioSubr};
use crate::circbuf;

const BUF_LEN: u16 = 3; // [s]
const RATE_CONV_CHUNK: usize = 1024;

pub struct AudioFileFactory {
    asset_mgr: AssetManagerRc,
    name: String,
}

impl AudioFileFactory {
    pub fn new<S: AsRef<str>>(asset_mgr: AssetManagerRc, name: S) -> Self {
        Self {
            asset_mgr,
            name: name.as_ref().to_string(),
        }
    }

    fn create_rate_conv_buf(channels: usize, len: usize) -> Box<[Box<[f32]>]> {
        Box::from_iter(iter::repeat_n(Box::from_iter(iter::repeat_n(0.0, len)), channels))
    }
}

#[atomic_enum]
enum State {
    Inactive,
    Playing,
    Done,
}

struct DecodedBuf {
    buf: SampleBuffer::<f32>,
    i: usize,
    len: usize,
}

impl AudioFactory for AudioFileFactory {
    type Source = AudioFileSource;
    type Handle = AudioFileHandle;

    fn build(self, channels: u16, sample_rate: u32, subr: AudioSubr) -> (Self::Source, Self::Handle) {
        let channels = channels as usize;

        // Open audio file.
        // TODO: if we move opening to the decoder thread (on android), then we can get rid of complexity in asset.rs.

        let f = self.asset_mgr.open_thr(&self.name);
        let src = ReadOnlySource::new(f);
        let mss = MediaSourceStream::new(Box::new(src), Default::default());
        let probe = symphonia::default::get_probe().format(&Default::default(), mss, &Default::default(), &Default::default()).expect("Unable to probe");
        let mut format = probe.format;

        let track = format.default_track().expect("Unable to determine default track");
        let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &Default::default()).expect("Unable to create decoder");
        let track_id = track.id;

        let codec_params = decoder.codec_params();
        assert!(codec_params.channels.unwrap().count() == channels);
        let decoder_sample_rate = codec_params.sample_rate.unwrap();

        // Setup circular buffer.

        let len = channels * sample_rate as usize * BUF_LEN as usize;
        let (sender, receiver) = circbuf::circbuf::<f32>(len);

        // Determine, if we need rate conversion.

        let rate_conv_opt = if decoder_sample_rate != sample_rate {
            let rate_conv = FftFixedInOut::<f32>::new(decoder_sample_rate as usize, sample_rate as usize, RATE_CONV_CHUNK, channels).expect("Unable to create sample rate converter");
            Some(rate_conv)
        } else {
            None
        };

        // Start decoder thread. At the end, the decoded data should be converted
        // to interleaved samples, since this is the format expected by the audio engine.

        thread::spawn(move || {
            let mut do_decode = |interleave| {
                loop {
                    let packet = match format.next_packet() {
                        Ok(packet) => packet,
                        Err(err) => match err {
                            symphonia_Error::IoError(err) => {
                                if err.kind() == io::ErrorKind::UnexpectedEof {
                                    break None;
                                }

                                panic!("I/O error");
                            },
                            _ => {
                                panic!("I/O error");
                            }
                        }
                    };

                    while !format.metadata().is_latest() {
                        format.metadata().pop();
                    }

                    if packet.track_id() != track_id {
                        continue;
                    }

                    let decoded = decoder.decode(&packet).expect("Decode error");
                    let decoded_len = decoded.frames();

                    if decoded_len == 0 {
                        continue;
                    }

                    let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
                    
                    if interleave {
                        buf.copy_interleaved_ref(decoded);
                    } else {
                        buf.copy_planar_ref(decoded);
                    };

                    break Some((buf, decoded_len));
                }
            };

            if let Some(mut rate_conv) = rate_conv_opt {
                // Rate conversion is needed.

                let out_max = rate_conv.output_frames_max();
                let mut in_buf = Self::create_rate_conv_buf(channels, rate_conv.input_frames_max());
                let mut out_buf = Self::create_rate_conv_buf(channels, out_max);
                let mut interleave_buf = Box::from_iter(iter::repeat_n(0.0, channels * out_max));

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
                            match do_decode(false) {
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

                                    for in_buf_ch in in_buf.iter_mut().map(|in_buf_ch| &mut in_buf_ch[in_i..in_next]) {
                                        in_buf_ch.fill(0.0);
                                    }

                                    in_i += todo;
                                    end = true;
                                    break;
                                },
                            };
                        }

                        // Copy from decoded_buf into in_buf: it is needed, since the decoder and the rate converter have
                        // different buffer sizes.

                        let decoded_buf = decoded_buf_opt.as_mut().unwrap();
                        todo = [decoded_buf.len - decoded_buf.i,
                                todo].into_iter().min().unwrap();
                        assert!(todo > 0);

                        for (decoded_buf_ch, in_buf_ch) in iter::zip(
                            decoded_buf.buf.samples().chunks_exact(decoded_buf.len).map(|decoded_buf_ch| &decoded_buf_ch[decoded_buf.i..decoded_buf.i + todo]),
                            in_buf.iter_mut().map(|in_buf_ch| &mut in_buf_ch[in_i..in_i + todo])) {
                            in_buf_ch.copy_from_slice(decoded_buf_ch);
                        }

                        // Consume decode buffer.

                        decoded_buf.i += todo;
                        if decoded_buf.i == decoded_buf.len {
                            decoded_buf_opt = None;
                        }

                        in_i += todo;
                    }

                    assert!(in_i == in_next);

                    // Do rate conversion.

                    let (in_rd, out_wr) = rate_conv.process_into_buffer(&in_buf, &mut out_buf, None).expect("Unable to do rate conversion");

                    // Consume input buffer.

                    if in_rd > 0 && in_rd < in_i {
                        for in_buf_ch in &mut in_buf {
                            in_buf_ch.copy_within(in_rd..in_i, 0);
                        }
                    }

                    in_i -= in_rd;

                    // Interleave and send output.

                    if out_wr > 0 {
                        let mut out_buf_ch_its: Vec<_> = out_buf.iter().map(|out_buf_ch| out_buf_ch.iter()).collect();
                        let mut interleave_buf_it = interleave_buf.iter_mut();

                        for _ in 0..out_wr {
                            for out_buf_ch_it in &mut out_buf_ch_its {
                                *interleave_buf_it.next().unwrap() = *out_buf_ch_it.next().unwrap();
                            }
                        }

                        if !sender.send(&interleave_buf[..channels * out_wr]) {
                            break;
                        }
                    }
                }
            } else {
                // Rate conversion is not needed.

                while let Some((interleave_buf, _)) = do_decode(true) {
                    if !sender.send(interleave_buf.samples()) {
                        break;
                    }
                }
            }
        });

        // Wait until the buffer is full.

        receiver.wait_full();

        // Prepare return value.

        let state = Arc::new(AtomicState::new(State::Inactive));

        let source = AudioFileSource::new(Arc::clone(&state), receiver);
        let handle = AudioFileHandle::new(state, subr);

        (source, handle)
    }
}

pub struct AudioFileSource {
    state: Arc<AtomicState>,
    receiver: circbuf::Receiver<f32>,
}

impl AudioFileSource {
    fn new(state: Arc<AtomicState>, receiver: circbuf::Receiver<f32>) -> Self {
        Self {
            state,
            receiver,
        }
    }
}

impl AudioSource for AudioFileSource {
    fn get_samples(&self, buf: &mut [f32]) -> AudioSourceState {
        match self.state.load(Ordering::Relaxed) {
            State::Inactive => {
                AudioSourceState::Inactive
            },
            State::Playing => {
                let len = self.receiver.recv(buf);

                if len == 0 {
                    self.state.store(State::Done, Ordering::Relaxed);
                    return AudioSourceState::Done;
                }

                if len < buf.len() { // TODO: Do this one in engine?
                    // At EOF, pad with silence.

                    buf[len..].fill(0.0);
                }

                AudioSourceState::Playing
            },
            State::Done => {
                AudioSourceState::Done
            },
        }
    }
}

pub struct AudioFileHandle {
    state: Arc<AtomicState>,
    subr: AudioSubr,
}

impl AudioFileHandle {
    fn new(state: Arc<AtomicState>, subr: AudioSubr) -> Self {
        Self {
            state,
            subr,
        }
    }

    pub fn play(&self) {
        if matches!(self.state.load(Ordering::Relaxed), State::Inactive) {
            self.state.store(State::Playing, Ordering::Relaxed);
        }
    }

    pub fn get_timestamp(&self) -> AudioFileTimestamp {
        match self.state.load(Ordering::Relaxed) {
            State::Inactive => AudioFileTimestamp::Inactive,
            State::Playing => self.subr.get_timestamp().map_or(AudioFileTimestamp::Unavail, AudioFileTimestamp::Playing),
            State::Done => AudioFileTimestamp::Done,
        }
    }
}

impl Drop for AudioFileHandle {
    fn drop(&mut self) {
        self.state.store(State::Done, Ordering::Relaxed);
    }
}

pub enum AudioFileTimestamp {
    Inactive,
    Unavail,
    Playing(f64),
    Done,
}
