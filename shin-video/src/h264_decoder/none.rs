//! The last fallback when this system can decode no H.264 at all: every movie ends at once and the game goes on
//! without it (MOVIEWAIT passes, the opening movie is skipped). Logged once, so the player can be told what to install.

use std::{
    io::{Read, Seek},
    sync::Once,
};

use anyhow::Result;
use dpi::PhysicalSize;
use tracing::error;

use crate::{
    h264_decoder::{FrameTiming, H264DecoderTrait, Nv12Frame},
    mp4::Mp4TrackReader,
};

pub struct NoH264Decoder {
    size: PhysicalSize<u32>,
}

impl NoH264Decoder {
    /// `why`: what is missing and what to install, said once in the log
    pub fn with_reason<S: Read + Seek + Send + 'static>(
        track: Mp4TrackReader<S>,
        why: &str,
    ) -> Self {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| error!("Movies cannot be played on this system and are skipped: {why}"));
        let (width, height) = track.get_mp4_track_info(|t| (t.width() as u32, t.height() as u32));
        Self {
            size: PhysicalSize::new(width.max(2), height.max(2)),
        }
    }
}

impl H264DecoderTrait for NoH264Decoder {
    fn new<S: Read + Seek + Send + 'static>(track: Mp4TrackReader<S>) -> Result<Self> {
        Ok(Self::with_reason(track, "no H.264 decoder"))
    }

    fn read_frame(&mut self) -> Result<Option<(FrameTiming, Nv12Frame)>> {
        Ok(None)
    }

    fn frame_size(&mut self) -> Result<PhysicalSize<u32>> {
        Ok(self.size)
    }
}
