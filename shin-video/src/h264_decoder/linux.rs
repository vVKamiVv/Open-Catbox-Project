//! Linux: GStreamer, the system's own media framework, when it has an H.264 decoder (gst-libav's `avdec_h264`,
//! openh264, or a hardware one); else ffmpeg on the PATH; else no movies (see `none.rs`).

use std::io::{Read, Seek};

use anyhow::Result;
use dpi::PhysicalSize;
use tracing::warn;

use crate::{
    h264_decoder::{
        FrameTiming, H264DecoderTrait, Nv12Frame,
        gstreamer::{GStreamerH264Decoder, can_decode_h264},
        none::NoH264Decoder,
        spawn_ffmpeg::{SpawnFfmpegH264Decoder, ffmpeg_available},
    },
    mp4::Mp4TrackReader,
};

pub enum AutoH264Decoder {
    GStreamer(GStreamerH264Decoder),
    Ffmpeg(SpawnFfmpegH264Decoder),
    None(NoH264Decoder),
}

impl H264DecoderTrait for AutoH264Decoder {
    fn new<S: Read + Seek + Send + 'static>(track: Mp4TrackReader<S>) -> Result<Self> {
        if can_decode_h264() {
            return Ok(Self::GStreamer(GStreamerH264Decoder::new(track)?));
        }
        if ffmpeg_available() {
            warn!("GStreamer has no H.264 decoder; using ffmpeg");
            return Ok(Self::Ffmpeg(SpawnFfmpegH264Decoder::new(track)?));
        }
        Ok(Self::None(NoH264Decoder::with_reason(
            track,
            "GStreamer has no H.264 decoder; install gst-libav (gstreamer1.0-libav on Debian and Ubuntu, \
             gstreamer1-plugin-openh264 on Fedora)",
        )))
    }

    fn read_frame(&mut self) -> Result<Option<(FrameTiming, Nv12Frame)>> {
        match self {
            Self::GStreamer(decoder) => decoder.read_frame(),
            Self::Ffmpeg(decoder) => decoder.read_frame(),
            Self::None(decoder) => decoder.read_frame(),
        }
    }

    fn frame_size(&mut self) -> Result<PhysicalSize<u32>> {
        match self {
            Self::GStreamer(decoder) => decoder.frame_size(),
            Self::Ffmpeg(decoder) => decoder.frame_size(),
            Self::None(decoder) => decoder.frame_size(),
        }
    }
}
