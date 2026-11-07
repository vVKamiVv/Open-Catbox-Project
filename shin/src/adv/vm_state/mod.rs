//! The scene model the commands keep (the original's `shin::ADV::Context`): layers and their properties, music and
//! looping sounds, the message window's style, SAVEINFO texts, CHARS, TIPSGET and NOTIFYSET state, and the save
//! counter, route and pending SSETs ([`save`]). It is all a load needs: replaying the script rebuilds it without
//! drawing, and `adv/restore.rs` builds the scene from it.

pub mod audio;
pub mod layers;
pub mod save;

use layers::LayersState;
use save::SaveState;
use shin_core::{
    format::save::{GameDataEntry, SaveData},
    vm::command::types::MessageboxStyle,
};

use crate::adv::vm_state::audio::AudioState;

/// The SAVEINFO texts (model `+0x90f8..+0x9118`): level 0 the episode, level 1 the scene title
pub struct SaveInfo {
    /// What SAVEINFO last set (`+0x90f8 + 16 * level`)
    pub pending: [String; 2],
    /// What the last AUTOSAVE committed (`+0x9100`, `+0x9110`): the save slots, the banner and the system menu use these
    pub info: [String; 2],
}

impl SaveInfo {
    /// SAVEINFO (`0x7100002590`) sets the pending text; the script uses levels 0 and 1 only
    pub fn set_save_info(&mut self, level: i32, info: String) {
        match self.pending.get_mut(level as usize) {
            Some(slot) if level >= 0 => *slot = info,
            _ => tracing::warn!("SAVEINFO level {level} is outside the model's two; ignored"),
        }
    }

    /// AUTOSAVE (`0x71000026e0`) commits the pending texts
    pub fn commit(&mut self) {
        self.info = self.pending.clone();
    }
}

#[derive(Debug)]
pub struct MessageState {
    pub msginit: MessageboxStyle,
    pub messagebox_shown: bool,
    pub text: Option<String>,
}

impl MessageState {
    pub fn new() -> Self {
        Self {
            msginit: MessageboxStyle::default(),
            messagebox_shown: false,
            text: None,
        }
    }
}

pub struct VmState {
    pub save_info: SaveInfo,
    pub messagebox_state: MessageState,
    /// The save file's contents (persistent data, slots, settings); it outlives this state across loads
    pub global: SaveData,
    pub save: SaveState,
    pub layers: LayersState,
    pub audio: AudioState,
    /// NOTIFYSET's level (`+0x9138`): 0 no banners, 1 or 2 the "updated" banners' sound
    pub notify_level: i32,
    /// CHARS' arguments (`+0x9118`), for the character screen
    pub chars: (i32, i32),
    /// The tips TIPSGET gave in this playthrough (`+0x9120`), for the tips screen
    pub tips: Vec<i32>,
}

impl VmState {
    pub fn new(global: SaveData, entry: GameDataEntry) -> Self {
        Self {
            save_info: SaveInfo {
                pending: Default::default(),
                info: Default::default(),
            },
            messagebox_state: MessageState::new(),
            global,
            save: SaveState::new(entry),
            layers: LayersState::new(),
            audio: AudioState::new(),
            notify_level: 0,
            // below 0 until the first CHARS: the system menu has no Characters item at the start of a game (as on
            // the Switch)
            chars: (-1, -1),
            tips: Vec::new(),
        }
    }
}
