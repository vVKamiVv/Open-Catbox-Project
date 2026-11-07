mod nv12;

use std::io::{Read, Seek};

use anyhow::Result;
use cfg_if::cfg_if;
use dpi::PhysicalSize;
pub use nv12::Nv12Frame;

use crate::mp4::Mp4TrackReader;

pub trait H264DecoderTrait: Sized {
    fn new<S: Read + Seek + Send + 'static>(track: Mp4TrackReader<S>) -> Result<Self>;

    fn read_frame(&mut self) -> Result<Option<(FrameTiming, Nv12Frame)>>;

    fn frame_size(&mut self) -> Result<PhysicalSize<u32>>;
}

// each system's own decoder first, then ffmpeg on the PATH, then none (the movies are skipped)
mod none;
cfg_if! {
    if #[cfg(target_os = "linux")] {
        mod y4m;
        mod spawn_ffmpeg;
        mod gstreamer;
        mod linux;
        pub use linux::AutoH264Decoder as H264Decoder;
    } else if #[cfg(windows)] {
        mod y4m;
        mod spawn_ffmpeg;
        mod media_foundation;
        pub use media_foundation::AutoH264Decoder as H264Decoder;
    } else if #[cfg(target_os = "macos")] {
        mod y4m;
        mod spawn_ffmpeg;
        mod video_toolbox;
        pub use video_toolbox::AutoH264Decoder as H264Decoder;
    } else {
        // pub use y4m::{BitsPerSample, Colorspace, FrameSize, PlanarFrame, PlaneSize};
        mod y4m;
        mod spawn_ffmpeg;
        pub use spawn_ffmpeg::SpawnFfmpegH264Decoder as H264Decoder;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameTiming {
    pub frame_number: u32,
    pub start_time: u64,
    pub duration: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With `SHIN_TEST_MOVIE` set to an H.264 MP4 (CI makes one with ffmpeg): this system's decoder decodes every
    /// frame, in display order, at the movie's size, and ends with `None`
    #[test]
    #[ignore]
    fn this_systems_decoder_decodes_a_movie() {
        let path = std::env::var("SHIN_TEST_MOVIE").expect("SHIN_TEST_MOVIE");
        let expected_frames: u32 = std::env::var("SHIN_TEST_MOVIE_FRAMES")
            .map_or(Ok(0), |n| n.parse())
            .unwrap();
        let mp4 = crate::mp4::Mp4::new(std::fs::File::open(&path).unwrap()).unwrap();
        let (width, height) = mp4
            .video_track
            .get_mp4_track_info(|t| (t.width() as u32, t.height() as u32));
        let mut decoder = H264Decoder::new(mp4.video_track).unwrap();
        assert_eq!(
            decoder.frame_size().unwrap(),
            PhysicalSize::new(width, height)
        );
        let mut count = 0;
        let mut last_start = None;
        while let Some((timing, frame)) = decoder.read_frame().unwrap() {
            assert_eq!(frame.size, PhysicalSize::new(width, height));
            assert_eq!(frame.y_plane.len(), (width * height) as usize);
            assert_eq!(frame.uv_plane.len(), (width * height.div_ceil(2)) as usize);
            assert!(
                last_start.is_none_or(|last| timing.start_time > last),
                "frames in display order"
            );
            last_start = Some(timing.start_time);
            count += 1;
        }
        assert!(count > 0, "no frames decoded");
        if expected_frames > 0 {
            assert_eq!(count, expected_frames);
        }
        println!("decoded {count} frames of {width}x{height}");
    }
}
