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

pub struct SaveInfo {
    pub info: [String; 4],
}

impl SaveInfo {
    pub fn set_save_info(&mut self, level: i32, info: String) {
        assert!(
            (0..=4).contains(&level),
            "SaveInfo::set_save_info: level out of range"
        );

        self.info[level as usize] = info;
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
}

impl VmState {
    pub fn new(global: SaveData, entry: GameDataEntry) -> Self {
        Self {
            save_info: SaveInfo {
                info: ["", "", "", ""].map(|v| v.to_string()),
            },
            messagebox_state: MessageState::new(),
            global,
            save: SaveState::new(entry),
            layers: LayersState::new(),
            audio: AudioState::new(),
        }
    }
}
