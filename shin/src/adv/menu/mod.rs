//! The adventure screen's task pool (`ADV+0x158`, add `0x7100100930`): the windows and menus over the scene. Every
//! task updates each frame; the top one has the focus and takes the player's input, and the ADV's own buttons only
//! work while the pool is empty.
//!
//! A menu that opens another (the system menu's dialog, its config screen) owns it and runs it on top of itself, in
//! place of the original's callbacks.

pub mod config_menu;
pub mod dialog;
pub mod hidden_window;
pub mod save_menu;
pub mod system_menu;

use std::sync::Arc;

use shin_audio::AudioManager;
use shin_core::{
    format::{save::SaveData, scenario::Scenario},
    vm::command::types::Volume,
};
use shin_render::render_pass::RenderPass;

use crate::{
    asset::{font::GpuFontLazy, system::AssetServer, texture_archive::NamedTextures},
    audio::play_sys_se,
    render::PreRenderContext,
    ui::MenuInput,
};

/// What the menus draw with
#[derive(Clone)]
pub struct MenuAssets {
    /// `/systex.txa`
    pub system_textures: Arc<NamedTextures>,
    /// `/adv.txa`
    pub adv_textures: Arc<NamedTextures>,
    pub system_font: Arc<GpuFontLazy>,
    /// For replaying bookmarks
    pub scenario: Arc<Scenario>,
}

/// What a task asks of the adventure screen
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvRequest {
    /// The hidden window has closed: the message window and the banners come back (`0x71000145f0(adv, 0)`)
    Unhide,
    /// The system menu has closed (`0x7100014570`): the scene, voices and sounds go on, and the backdrop fades
    SystemMenuClosed,
    /// "Return to title" (`ADV::ExitGame` `0x71000147e0`): write the save file, then go to the title
    ExitToTitle,
    /// Write the save file (a `SaveGame` task, `0x71000d52f0`)
    WriteSave,
    /// Put the current page in this bookmark and write the save file (`0x71000da920`)
    SaveToSlot(usize),
    /// Leave for this bookmark: a new adventure screen loads it (`0x7100016410`)
    LoadSlot(usize),
}

/// What a task gets each frame
pub struct TaskCx<'a> {
    pub frames: f32,
    /// Whether the task is on top and takes the input
    pub focused: bool,
    /// The pad and the pointer; buttons are all released when the task is not focused
    pub input: MenuInput,
    pub pre_render: &'a PreRenderContext<'a, 'a, 'a, 'a>,
    pub audio: &'a AudioManager,
    pub assets: &'a AssetServer,
    /// The save data, with the settings the config screen edits
    pub global: &'a mut SaveData,
    pub requests: &'a mut Vec<AdvRequest>,
}

impl TaskCx<'_> {
    /// Plays a system sound (`SysSePlayer::play(1.0, name)`)
    pub fn sound(&self, name: &str) {
        play_sys_se(self.audio, self.assets, name, Volume::default());
    }

    /// The same frame for a task's child, which has the focus instead when it exists
    pub fn reborrow(&mut self, focused: bool) -> TaskCx<'_> {
        TaskCx {
            frames: self.frames,
            focused,
            input: if focused {
                self.input
            } else {
                MenuInput::released(self.input.pointer)
            },
            pre_render: self.pre_render,
            audio: self.audio,
            assets: self.assets,
            global: self.global,
            requests: self.requests,
        }
    }
}

pub trait Task {
    /// Advances the task by a frame; false once it has finished and can be dropped (vfunc 2)
    fn update(&mut self, cx: &mut TaskCx) -> bool;
    fn render(&self, pass: &mut RenderPass);
}
