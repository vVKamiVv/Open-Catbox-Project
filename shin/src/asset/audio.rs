use std::sync::Arc;

use anyhow::{Context, Result};
use shin_core::format::{
    audio::{AudioFile, read_audio},
    sysse::{SysSe, read_sys_se},
};

use crate::asset::system::{Asset, AssetDataAccessor, AssetLoadContext};

impl Asset for AudioFile {
    type Args = ();

    async fn load(
        _context: &Arc<AssetLoadContext>,
        _args: (),
        _name: &str,
        data: AssetDataAccessor,
    ) -> Result<Self> {
        read_audio(&data.read_all().await).context("Parsing audio file")
    }
}

/// `/sysse.bin`: the engine's built-in system sounds, by name
impl Asset for SysSe {
    type Args = ();

    async fn load(
        _context: &Arc<AssetLoadContext>,
        _args: (),
        _name: &str,
        data: AssetDataAccessor,
    ) -> Result<Self> {
        read_sys_se(&data.read_all().await).context("Parsing sysse.bin")
    }
}
