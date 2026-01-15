use std::ops::RangeInclusive;

use crate::audio::{AudioInput, AudioSource, AudioSourceState};
use crate::mailbox::{self, Receiver, Sender, TryRecvError};

const LEVEL_RANGE: RangeInclusive<i8> = -90..=0; // 1/(2^15) =~ 10^(-90/20)

pub struct AudioFader<T> {
    inner_input: T,
    rx: Receiver<Command>,
}

impl<T: AudioInput> AudioFader<T> {
    pub fn new(inner_input: T) -> (Self, AudioFaderHandle) {
        let (tx, rx) = mailbox::mailbox();

        let input = Self {
            inner_input,
            rx,
        };

        let handle = AudioFaderHandle::new(tx);

        (input, handle)
    }

    fn build(self, channels: u16, sample_rate: u32) -> AudioFaderSource<T::Source> {
        let inner_source = self.inner_input.build(channels, sample_rate);
        AudioFaderSource::new(inner_source, self.rx, channels, sample_rate)
    }
}

impl<T: AudioInput> AudioInput for AudioFader<T> {
    type Source = AudioFaderSource<T::Source>;

    fn build(self, channels: u16, sample_rate: u32) -> Self::Source {
        self.build(channels, sample_rate)
    }
}

pub struct AudioFaderSource<T> {
    inner_source: T,
    rx: Receiver<Command>,
    channels: u16,
    sample_rate: u32,
    level: i8, // [dB]
    state_opt: Option<State>,
}

enum Command { // Rate [dB/s]
    Silence,
    FadeIn(u8),
    FadeOut(u8),
}

struct State {
    delta: i8,
    samples_per_db: usize,
    samples_processed: usize,
}

impl<T: AudioSource> AudioFaderSource<T> {
    fn new(inner_source: T, rx: Receiver<Command>, channels: u16, sample_rate: u32) -> Self {
        Self {
            inner_source,
            rx,
            channels,
            sample_rate,
            level: *LEVEL_RANGE.end(), // Default: no fade.
            state_opt: None,
        }
    }
}

impl<T: AudioSource> AudioSource for AudioFaderSource<T> {
    fn get_samples(&mut self, buf: &mut [f32]) -> AudioSourceState {
        match self.rx.try_recv() {
            Ok(cmd) => {
                let param_opt = match cmd {
                    Command::Silence => {
                        self.level = *LEVEL_RANGE.start();
                        None
                    },
                    Command::FadeIn(rate) => Some((1, rate)),
                    Command::FadeOut(rate) => Some((-1, rate)),
                };

                self.state_opt = param_opt.map(|(delta, rate)| {
                    let samples_per_db = (self.sample_rate as usize / rate as usize) * self.channels as usize;
                    assert!(samples_per_db > 0);

                    State {
                        delta,
                        samples_per_db,
                        samples_processed: 0,
                    }
                });
            },
            Err(e) => match e {
                TryRecvError::Empty => (),
                TryRecvError::Disconnected => return AudioSourceState::Drop,
            },
        }

        match self.inner_source.get_samples(buf) {
            AudioSourceState::Paused => {
                AudioSourceState::Paused
            },
            AudioSourceState::Playing => { // TODO: use simd for processing
                let buf_len = buf.len();
                let mut i: usize = 0;

                loop {
                    let buf_todo = buf_len - i;
                    if buf_todo == 0 {
                        break;
                    }

                    if let Some(state) = &mut self.state_opt {
                        let level_stop = if state.delta > 0 { *LEVEL_RANGE.end() } else { *LEVEL_RANGE.start() };
                        if self.level == level_stop {
                            self.state_opt = None;
                            continue;
                        }

                        let todo = (state.samples_per_db - state.samples_processed).min(buf_todo);
                        assert!(todo > 0);

                        let buf_sl = &mut buf[i..i + todo];
                        let level = 10_f32.powf(self.level as f32 / 20.0);

                        for sample in buf_sl {
                            *sample *= level;
                        }

                        state.samples_processed += todo;
                        if state.samples_per_db == state.samples_processed {
                            state.samples_processed = 0;
                            self.level += state.delta;
                        }

                        i += todo;
                    } else if self.level == *LEVEL_RANGE.start() {
                        // Fill with silence.
                        // TODO: More optimized, e.g. discard self.inner_source.get_samples()?

                        let buf_sl = &mut buf[i..];
                        buf_sl.fill(0.0);
                        break;
                    } else {
                        // No fade.

                        assert!(self.level == *LEVEL_RANGE.end());
                        break;
                    }
                }

                AudioSourceState::Playing                
            },
            AudioSourceState::Drop => {
                AudioSourceState::Drop
            },
        }
    }
}

pub struct AudioFaderHandle {
    tx: Sender<Command>,
}

impl AudioFaderHandle {
    fn new(tx: Sender<Command>) -> Self {
        Self {
            tx,
        }
    }

    pub fn silence(&self) {
        self.send(Command::Silence);
    }

    pub fn fade_in(&self, rate: u8) {
        assert!(rate > 0);
        self.send(Command::FadeIn(rate));
    }

    pub fn fade_out(&self, rate: u8) {
        assert!(rate > 0);
        self.send(Command::FadeOut(rate));
    }

    fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd); // Ignore if the source has been dropped.
    }
}
