use std::cell::Cell;
use std::iter;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use atomic::{Atomic, Ordering};
use bytemuck::NoUninit;

use cpal::{BufferSize, Device, SampleFormat, SampleRate, Stream, StreamConfig};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

const CHANNELS: u16 = 2;
const MIN_SAMPLE_RATE: u32 = 44100;
const LATENCY: f32 = 0.01; // [s]

pub trait AudioInput {
    type Source: AudioSource + Send;

    fn build(self, channels: u16, sample_rate: u32) -> Self::Source;
}

pub trait AudioSource {
    fn get_samples(&mut self, buf: &mut [f32]) -> AudioSourceState;
}

pub enum AudioSourceState {
    Paused,
    Playing,
    Drop,
}

pub type AudioEngineRc = Rc<AudioEngine>;

pub struct AudioEngine {
    config: StreamConfig,
    stream: Rc<Stream>,
    worker_tx: Sender<WorkerMessage>,
    playing: Cell<bool>,
}

type WorkerMessage = (Box<dyn FnOnce() -> Box<dyn AudioSource + Send> + Send>, AudioPosAtomic);

type InnerMutex = Arc<Mutex<Inner>>;

struct Inner { // TODO: We have only a single field, keep struct?
    source_infos: Vec<SourceInfo>,
}

struct SourceInfo {
    source: Box<dyn AudioSource + Send>,
    pos_atomic: AudioPosAtomic,
}

impl AudioEngine {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let host = cpal::default_host();
        let device = host.default_output_device().expect("Unable to determine default audio device");

        // Range selection criteria:
        // - SampleFormat::F32 is needed for sample rate conversion (rubato).
        // - The sample rate should be closest to MIN_SAMPLE_RATE.

        let all_ranges: Vec<_> = device.supported_output_configs().expect("Unable to determine supported formats").filter(|range| range.channels() == CHANNELS && range.sample_format() == SampleFormat::F32).collect();

        let mut ranges: Vec<_> = all_ranges.iter().filter(|range| range.min_sample_rate().0 <= MIN_SAMPLE_RATE && range.max_sample_rate().0 >= MIN_SAMPLE_RATE).collect();
        let (range, sample_rate) = if !ranges.is_empty() {
            (ranges[0], MIN_SAMPLE_RATE)
        } else {
            ranges = all_ranges.iter().filter(|range| range.min_sample_rate().0 >= MIN_SAMPLE_RATE).collect();
            ranges.sort_by_key(|range| range.min_sample_rate().0);
            let range = ranges.first().expect("No supported format");

            (*range, range.min_sample_rate().0)
        };
  
        let mut config: StreamConfig = range.with_sample_rate(SampleRate(sample_rate)).config();
        config.buffer_size = BufferSize::Fixed((config.sample_rate.0 as f32 * LATENCY) as u32); // TODO: hardcoded bufsize, determine it from device capabilities?

        // Construct inner.

        let inner = Inner {
            source_infos: Vec::new(),
        };
        let inner_mutex = Arc::new(Mutex::new(inner));

        // Setup stream.

        let stream = Self::build_stream(&device, &config, Arc::clone(&inner_mutex));

        // Start worker.

        let (worker_tx, worker_rx) = mpsc::channel();
        thread::spawn(move || Self::worker_impl(inner_mutex, worker_rx));

        Self {
            config,
            stream: Rc::new(stream),
            worker_tx,
            playing: Cell::new(false),
        }
    }

    fn build_stream(device: &Device, config: &StreamConfig, inner_mutex: InnerMutex) -> Stream {
        // We don't want to make heap allocation in the mixer thread, so allocate source_buf early.
        // TODO: how can we determine the size of the buffer, since cpal can't guarantee requested
        // buffer size?

        let sample_rate = config.sample_rate.0;
        let mut source_buf = Vec::from_iter(iter::repeat_n(0.0, CHANNELS as usize * sample_rate as usize));
        let mut frame_count = 0_u64;

        // Build output stream.

        device.build_output_stream(config, move |buf: &mut [f32], _| { // TODO: use simd for processing
            let buf_len = buf.len();
            assert!(buf_len <= source_buf.len());
            let source_buf_sl = &mut source_buf.as_mut_slice()[..buf_len];

            buf.fill(0.0);

            let mut inner = inner_mutex.lock().unwrap(); // TODO: how to avoid blocking in mixer thread?
            let source_infos = &mut inner.source_infos;
            let mut i = 0;

            let frame_count_pause = frame_count + sample_rate as u64;

            while i < source_infos.len() {
                let source_info = &mut source_infos[i];
                let mut pos = source_info.pos_atomic.load(Ordering::Relaxed);

                // Notes:
                // - We don't know the exact latency between the data_callback and
                //   actual audio output.
                // - However, we can assume that stream.get_timestamp() lags behind
                //   frame_count.
                // - We are calculating AudioPos.start/end, which gets compared to
                //   stream.get_timestamp() (see AudioTimestamp.get_timestamp()).

                // If the source is paused, then don't call get_samples again until
                // pos.end has been reached, otherwise it would cause incorrect timestamp
                // calculation (we are maintaining a single AudioPos only).
                // TODO: Usage of frame_count_pause is just a code simplification, the
                // correct solution would be to use stream.get_timestamp().
                if pos.end != u64::MAX && pos.end > frame_count_pause {
                    i += 1;
                } else {
                    match source_info.source.get_samples(source_buf_sl) {                        
                        AudioSourceState::Paused => {
                            if pos.end == u64::MAX {
                                pos.end = frame_count;
                                source_info.pos_atomic.store(pos, Ordering::Relaxed);
                            }

                            i += 1;
                        },
                        AudioSourceState::Playing => {
                            if pos.end != u64::MAX {
                                pos.offset += pos.end - pos.start;
                                pos.start = frame_count;
                                pos.end = u64::MAX;
                                source_info.pos_atomic.store(pos, Ordering::Relaxed);
                            }

                            // TODO: scale output depending on the number of active sources.
                            for (src_sample, dst_sample) in source_buf_sl.iter().zip(buf.iter_mut()) {
                                *dst_sample += *src_sample;
                            }

                            i += 1;
                        },
                        AudioSourceState::Drop => {
                            source_infos.swap_remove(i); // TODO: more optimized removal?
                        },
                    }
                }
            }

            frame_count += buf_len as u64 / CHANNELS as u64;
        },
        |_| {
        },
        None).expect("Unable to build stream")
    }

    pub fn add<T: AudioInput + Send + 'static>(&self, input: T) -> AudioTimestamp {
        // Execute build_func on the worker thread to avoid blocking of
        // the render thread. For example: before playing, the factory function is
        // doing some buffering.

        let sample_rate = self.config.sample_rate.0;

        let pos = AudioPos {
            start: 0,
            end: 0,
            offset: 0,
        };
        let pos_atomic = Arc::new(Atomic::new(pos));

        self.worker_tx.send((Box::new(move || Box::new(input.build(CHANNELS, sample_rate))), Arc::clone(&pos_atomic))).unwrap();

        AudioTimestamp::new(sample_rate, Rc::clone(&self.stream), pos_atomic)
    }

    pub fn start(&self) {
        if !self.playing.get() {
            self.stream.play().expect("Unable to start stream");
            self.playing.set(true);
        }
    }

    pub fn pause(&self) {
        if self.playing.get() {
            self.stream.pause().expect("Unable to pause stream");
            self.playing.set(false);
        }
    }

    fn worker_impl(inner_mutex: InnerMutex, worker_rx: Receiver<WorkerMessage>) {
        loop {
            let (build_func, pos_atomic) = match worker_rx.recv() {
                Ok(msg) => msg,
                Err(_) => break,
            };

            let source_info = SourceInfo {
                source: build_func(),
                pos_atomic,
            };

            let mut inner = inner_mutex.lock().unwrap();
            inner.source_infos.push(source_info);
        }
    }
}

pub struct AudioTimestamp {
    sample_rate: f64,
    stream: Rc<Stream>,
    pos_atomic: AudioPosAtomic,
}

impl AudioTimestamp {
    fn new(sample_rate: u32, stream: Rc<Stream>, pos_atomic: AudioPosAtomic) -> Self {
        Self {
            sample_rate: sample_rate.into(),
            stream,
            pos_atomic,
        }
    }

    pub fn get_timestamp(&self) -> Option<f64> {
        let stream_ts = self.stream.get_timestamp()?;
        let pos = self.pos_atomic.load(Ordering::Relaxed);

        let start_ts = pos.start as f64 / self.sample_rate;
        let end_ts = pos.end as f64 / self.sample_rate;

        let ts = if stream_ts < end_ts {
            0.0_f64.max(stream_ts - start_ts) // If stream_ts < start_ts: 0, otherwise increasing.
        } else {
            end_ts - start_ts // If stream_ts >= end_ts: not changing.
        };

        Some(pos.offset as f64 / self.sample_rate + ts)
    }
}

type AudioPosAtomic = Arc<Atomic<AudioPos>>;

#[repr(C)]
#[derive(Clone, Copy, NoUninit)]
struct AudioPos {
    start: u64,
    end: u64,
    offset: u64,
}
