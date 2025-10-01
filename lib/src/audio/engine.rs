use std::cell::Cell;
use std::iter;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};

use cpal::{BufferSize, Device, SampleFormat, Stream, StreamConfig};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

const CHANNELS: u16 = 2;
const MIN_SAMPLE_RATE: u32 = 44100;
const LATENCY: f32 = 0.01; // [s]
const INACTIVE: u64 = u64::MAX; // As we use atomic, we need a special sentinel value.

pub trait AudioFactory {
    type Source: AudioSource + Send + 'static;
    type Handle;

    fn build(self, channels: u16, sample_rate: u32, subr: AudioSubr) -> (Self::Source, Self::Handle);
}

pub trait AudioSource {
    fn get_samples(&self, buf: &mut [f32]) -> AudioSourceState;
}

pub enum AudioSourceState {
    Inactive,
    Playing,
    Done,
}

pub type AudioEngineRc = Rc<AudioEngine>;

pub struct AudioEngine {
    config: StreamConfig,
    stream: Rc<Stream>,
    inner_rc: Arc<Mutex<Inner>>,
    playing: Cell<bool>,
}

struct Inner { // TODO: We have only a single field, keep struct?
    source_infos: Vec<SourceInfo>,
}

struct SourceInfo {
    start_pos: Arc<AtomicU64>,
    source: Box<dyn AudioSource + Send + 'static>,
}

impl AudioEngine {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let host = cpal::default_host();
        let device = host.default_output_device().expect("Unable to determine default audio device");

        // Range selection criteria:
        // - SampleFormat::F32 is needed for sample rate conversion (rubato).
        // - The sample rate should be closest to MIN_SAMPLE_RATE.

        let mut ranges: Vec<_> = device.supported_output_configs().expect("Unable to determine supported formats").filter(|range| range.channels() == CHANNELS && range.min_sample_rate().0 >= MIN_SAMPLE_RATE && range.sample_format() == SampleFormat::F32).collect();
        ranges.sort_by_key(|range| range.min_sample_rate().0);

        let range = ranges.first().expect("No supported format");
        let mut config: StreamConfig = range.with_sample_rate(range.min_sample_rate()).config();
        config.buffer_size = BufferSize::Fixed((config.sample_rate.0 as f32 * LATENCY) as u32); // TODO: hardcoded bufsize, determine it from device capabilities?

        let inner = Inner {
            source_infos: Vec::new(),
        };
        let inner_rc = Arc::new(Mutex::new(inner));

        let stream = Self::build_stream(&device, &config, Arc::clone(&inner_rc));

        Self {
            config,
            stream: Rc::new(stream),
            inner_rc,
            playing: Cell::new(false),
        }
    }

    fn build_stream(device: &Device, config: &StreamConfig, inner_rc: Arc<Mutex<Inner>>) -> Stream {
        // We don't want to make heap allocation in the mixer thread, so allocate source_buf early.
        // TODO: how can we determine the size of the buffer, since cpal can't guarantee requested
        // buffer size?

        let mut source_buf = Vec::from_iter(iter::repeat_n(0.0, CHANNELS as usize * config.sample_rate.0 as usize));

        // Build output stream.

        let mut sample_count = 0_u64;

        device.build_output_stream(config, move |buf: &mut [f32], _| { // TODO: use simd for mixing?
            let buf_len = buf.len();
            assert!(buf_len <= source_buf.len());
            let source_buf_sl = &mut source_buf.as_mut_slice()[..buf_len];

            buf.fill(0.0);

            let mut inner = inner_rc.lock().unwrap(); // TODO: how to avoid blocking in mixer thread?
            let source_infos = &mut inner.source_infos;
            let mut i = 0;

            while i < source_infos.len() {
                let source_info = &source_infos[i];

                let start_pos = &source_info.start_pos;
                let start_pos_val = start_pos.load(Ordering::Relaxed);

                match source_info.source.get_samples(source_buf_sl) {                        
                    AudioSourceState::Inactive => {
                        assert!(start_pos_val == INACTIVE); // Once Playing, can't go back to Inactive.
                        i += 1;
                    },
                    AudioSourceState::Playing => {
                        if start_pos_val == INACTIVE {
                            start_pos.store(sample_count, Ordering::Relaxed);
                        }

                        for (src_sample, dst_sample) in source_buf_sl.iter().zip(buf.iter_mut()) {
                            *dst_sample += *src_sample;
                        }

                        i += 1;
                    },
                    AudioSourceState::Done => {
                        source_infos.swap_remove(i); // TODO: more opimized removal?
                    },
                };
            }

            if i > 1 {
                for sample in buf.iter_mut() {
                    *sample /= i as f32;
                }
            }

            sample_count += buf_len as u64 / CHANNELS as u64;
        },
        |_| {
        },
        None).expect("Unable to build stream")
    }

    pub fn add<F: AudioFactory>(&self, factory: F) -> F::Handle {
        let start_pos = Arc::new(AtomicU64::new(INACTIVE));
        let subr = AudioSubr::new(self.config.sample_rate.0, Rc::clone(&self.stream), Arc::clone(&start_pos));

        let (source, handle) = factory.build(CHANNELS, self.config.sample_rate.0, subr);

        let source_info = SourceInfo {
            start_pos,
            source: Box::new(source),
        };

        let mut inner = self.inner_rc.lock().unwrap();
        inner.source_infos.push(source_info);

        handle
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
}

pub struct AudioSubr {
    sample_rate: u32,
    stream: Rc<Stream>,
    start_pos: Arc<AtomicU64>,
}

impl AudioSubr {
    fn new(sample_rate: u32, stream: Rc<Stream>, start_pos: Arc<AtomicU64>) -> Self {
        Self {
            sample_rate,
            stream,
            start_pos,
        }
    }

    pub fn get_timestamp(&self) -> Option<f64> {
        let stream_ts = self.stream.get_timestamp()?;

        let start_pos_val = self.start_pos.load(Ordering::Relaxed);
        if start_pos_val == INACTIVE {
            return None;
        }

        let ts = stream_ts - start_pos_val as f64 / self.sample_rate as f64; // TODO: compute start_ts in the mixer thread, so we don't need division by sample_rate?
        if ts < 0.0 { // Source is not started yet in the mixer thread.
            None
        } else {
            Some(ts)
        }
    }
}
