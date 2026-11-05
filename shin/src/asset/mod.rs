mod audio;
#[expect(unused)]
pub mod bustup;
pub mod executable;
pub mod font;
pub mod mask;
pub mod movie;
pub mod picture;
mod scenario;
pub mod system;
pub mod texture_archive;

/// Common asset paths used in the game.
pub mod asset_paths {
    pub const SCENARIO: &str = "/main.snr";
    pub const SYSTEM_FNT: &str = "/system.fnt";
    pub const MSGTEX: &str = "/msgtex.txa";
    /// Particle and effect textures (rain, snow, petals, the dissolve "breakup" mask, focus lines)
    pub const EFFECTS: &str = "/effects.txa";
    /// The quiz's answer buttons
    pub const QUIZ: &str = "/quiz.txa";
    /// The cake game's cake, slices, finger and banner
    pub const CAKE: &str = "/cake.txa";
    /// The last choice's two doors
    pub const LAST: &str = "/last.txa";
    /// The culprit game's screens
    pub const MURDERER: &str = "/murderer.txa";
    /// System textures (the scroll bar)
    pub const SYSTEX: &str = "/systex.txa";
    pub const NEWRODIN_MEDIUM_FNT: &str = "/newrodin-medium.fnt";
    pub const NEWRODIN_BOLD_FNT: &str = "/newrodin-bold.fnt";
}
