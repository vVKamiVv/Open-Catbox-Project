//! Glue together `shin-core` and `kira` to provide an API to play NXA audio files.
//!
//! [`AudioManager`] owns kira's manager (cpal output; mixing runs on kira's audio thread). A sound is an NXA stream
//! (Opus decoded as it plays, with loop points) wrapped in [`AudioData`]; playing it gives an [`AudioHandle`] for
//! volume, pan, pause and stop with tweens, the playback position, the wait status bits the *WAIT commands test, and
//! the peak level used for lip-sync. Panning uses the original's linear balance (`0x710002bc40`). The game's players
//! (BGM, sound effects, voices, system sounds) are in `shin/src/audio/`.

mod data;
mod handle;
mod manager;
mod resampler;
mod sound;

pub use data::AudioData;
pub use handle::AudioHandle;
use kira::track::TrackId;
pub use manager::AudioManager;
pub use shin_core::format::audio::AudioFile;
use shin_core::{
    time::Tween,
    vm::command::types::{Pan, Volume},
};

pub struct AudioSettings {
    pub track: TrackId,
    pub fade_in: Tween,
    pub loop_start: Option<u32>,
    pub volume: Volume,
    pub pan: Pan,
    // no play speed: every SEPLAY in Umineko passes 1000
}
