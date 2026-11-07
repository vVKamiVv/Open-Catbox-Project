use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use kira::{
    manager::{
        AudioManagerSettings,
        backend::{
            Backend, Renderer,
            cpal::{self, CpalBackend},
        },
    },
    sound::SoundData,
};
use parking_lot::Mutex;
use tracing::warn;

pub struct AudioManager {
    manager: Mutex<kira::manager::AudioManager<OutputBackend>>,
}

impl AudioManager {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let manager = kira::manager::AudioManager::new(AudioManagerSettings::default())
            .expect("Failed to create kira audio manager");

        Self {
            manager: Mutex::new(manager),
        }
    }

    pub fn play<S: SoundData>(&self, data: S) -> S::Handle
    where
        S::Error: std::fmt::Debug,
    {
        let mut manager = self.manager.lock();

        manager.play(data).expect("Failed to start playing audio")
    }

    pub fn kira_manager(&self) -> &Mutex<kira::manager::AudioManager<OutputBackend>> {
        &self.manager
    }
}

/// The system's sound output through cpal; or, when it has none (no device, or ALSA that reaches neither PulseAudio
/// nor PipeWire), a silent one that still runs the sounds in real time, so the game neither stops at the start nor
/// waits forever on a voice or a sound effect
pub enum OutputBackend {
    Cpal(CpalBackend),
    Silent {
        sample_rate: u32,
        stop: Arc<AtomicBool>,
    },
}

const SILENT_SAMPLE_RATE: u32 = 48_000;

impl Backend for OutputBackend {
    type Settings = ();
    type Error = cpal::Error;

    fn setup((): ()) -> Result<(Self, u32), Self::Error> {
        match CpalBackend::setup(Default::default()) {
            Ok((backend, sample_rate)) => Ok((Self::Cpal(backend), sample_rate)),
            Err(e) => {
                warn!("No sound output ({e}); the game plays silently");
                let stop = Arc::new(AtomicBool::new(false));
                let silent = Self::Silent {
                    sample_rate: SILENT_SAMPLE_RATE,
                    stop,
                };
                Ok((silent, SILENT_SAMPLE_RATE))
            }
        }
    }

    fn start(&mut self, mut renderer: Renderer) -> Result<(), Self::Error> {
        match self {
            Self::Cpal(backend) => backend.start(renderer),
            Self::Silent { sample_rate, stop } => {
                let (sample_rate, stop) = (*sample_rate as f64, stop.clone());
                std::thread::Builder::new()
                    .name("silent-audio".to_string())
                    .spawn(move || {
                        // as an output would: the frames due since the start, in chunks, the samples thrown away
                        let started = Instant::now();
                        let mut rendered = 0u64;
                        while !stop.load(Ordering::Relaxed) {
                            let due = (started.elapsed().as_secs_f64() * sample_rate) as u64;
                            if due > rendered {
                                renderer.on_start_processing();
                                for _ in rendered..due {
                                    let _ = renderer.process();
                                }
                                rendered = due;
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                    })
                    .expect("could not start the silent audio thread");
                Ok(())
            }
        }
    }
}

impl Drop for OutputBackend {
    fn drop(&mut self) {
        if let Self::Silent { stop, .. } = self {
            stop.store(true, Ordering::Relaxed);
        }
    }
}
