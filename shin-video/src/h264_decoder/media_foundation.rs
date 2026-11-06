//! H.264 decoding with Windows' own decoder (Media Foundation's `CLSID_MSH264DecoderMFT`), so the game needs no
//! ffmpeg on Windows. A thread per movie feeds the track's samples (as Annex B, through [`Mp4BitstreamConverter`]) to
//! the decoder and sends the NV12 pictures it returns, in display order, through a channel; the samples' timings are
//! paired with them in order, as the ffmpeg backend does.
//!
//! When Media Foundation cannot decode (the "N" editions of Windows without the Media Feature Pack),
//! [`AutoH264Decoder`] falls back to ffmpeg.

use std::{
    collections::VecDeque,
    io::{Read, Seek},
    mem::ManuallyDrop,
    sync::mpsc::{Receiver, SyncSender},
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use dpi::PhysicalSize;
use tracing::{debug, warn};
use windows::Win32::{
    Media::MediaFoundation::{
        CLSID_MSH264DecoderMFT, IMFMediaType, IMFSample, IMFTransform,
        MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE, MF_MT_DEFAULT_STRIDE,
        MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_SUBTYPE,
        MF_VERSION, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample, MFMediaType_Video,
        MFSTARTUP_NOSOCKET, MFStartup, MFT_MESSAGE_COMMAND_DRAIN,
        MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_END_OF_STREAM,
        MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFVideoArea,
        MFVideoFormat_H264, MFVideoFormat_NV12,
    },
    System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
};

use crate::{
    h264_decoder::{
        FrameTiming, H264DecoderTrait, Nv12Frame, spawn_ffmpeg::SpawnFfmpegH264Decoder,
    },
    mp4::Mp4TrackReader,
    mp4_bitstream_converter::Mp4BitstreamConverter,
};

/// Pictures decoded ahead of the game (about 3 MB each at 1920x1080)
const FRAMES_AHEAD: usize = 8;

/// Media Foundation, or ffmpeg when it is not available
pub enum AutoH264Decoder {
    MediaFoundation(MediaFoundationH264Decoder),
    Ffmpeg(SpawnFfmpegH264Decoder),
}

impl H264DecoderTrait for AutoH264Decoder {
    fn new<S: Read + Seek + Send + 'static>(track: Mp4TrackReader<S>) -> Result<Self> {
        match MediaFoundationH264Decoder::start(track) {
            Ok(decoder) => Ok(Self::MediaFoundation(decoder)),
            Err((e, track)) => {
                warn!("Media Foundation cannot decode the movie ({e:?}); trying ffmpeg");
                Ok(Self::Ffmpeg(SpawnFfmpegH264Decoder::new(track)?))
            }
        }
    }

    fn read_frame(&mut self) -> Result<Option<(FrameTiming, Nv12Frame)>> {
        match self {
            Self::MediaFoundation(decoder) => decoder.read_frame(),
            Self::Ffmpeg(decoder) => decoder.read_frame(),
        }
    }

    fn frame_size(&mut self) -> Result<PhysicalSize<u32>> {
        match self {
            Self::MediaFoundation(decoder) => decoder.frame_size(),
            Self::Ffmpeg(decoder) => decoder.frame_size(),
        }
    }
}

type Picture = Result<(FrameTiming, Nv12Frame)>;

pub struct MediaFoundationH264Decoder {
    frames: Receiver<Picture>,
    /// A picture read to learn the size, not yet returned
    peeked: Option<Picture>,
    size: Option<PhysicalSize<u32>>,
}

impl MediaFoundationH264Decoder {
    /// Starts the decoding thread; gives the track back when Media Foundation cannot be set up
    fn start<S: Read + Seek + Send + 'static>(
        track: Mp4TrackReader<S>,
    ) -> std::result::Result<Self, (anyhow::Error, Mp4TrackReader<S>)> {
        let (ready_sender, ready_receiver) = std::sync::mpsc::channel();
        let (frame_sender, frames) = std::sync::mpsc::sync_channel(FRAMES_AHEAD);
        std::thread::Builder::new()
            .name("media-foundation-h264".to_string())
            .spawn(move || {
                let mut mft = match Mft::new() {
                    Ok(mft) => mft,
                    Err(e) => {
                        let _ = ready_sender.send(Err((e, track)));
                        return;
                    }
                };
                let _ = ready_sender.send(Ok(()));
                if let Err(e) = mft.run(track, &frame_sender) {
                    let _ = frame_sender.send(Err(e));
                }
            })
            .expect("could not start the decoding thread");
        match ready_receiver
            .recv()
            .expect("the decoding thread always reports")
        {
            Ok(()) => Ok(Self {
                frames,
                peeked: None,
                size: None,
            }),
            Err(failure) => Err(failure),
        }
    }

    fn next(&mut self) -> Option<Picture> {
        self.peeked.take().or_else(|| self.frames.recv().ok())
    }

    fn read_frame(&mut self) -> Result<Option<(FrameTiming, Nv12Frame)>> {
        let picture = self.next().transpose()?;
        if let Some((_, frame)) = &picture {
            self.size = Some(frame.size);
        }
        Ok(picture)
    }

    fn frame_size(&mut self) -> Result<PhysicalSize<u32>> {
        if let Some(size) = self.size {
            return Ok(size);
        }
        let Some(picture) = self.next().transpose()? else {
            bail!("No frames available, don't know the format");
        };
        let size = picture.1.size;
        self.size = Some(size);
        self.peeked = Some(Ok(picture));
        Ok(size)
    }
}

/// The decoder transform, owned by its thread
struct Mft {
    transform: IMFTransform,
    /// The visible picture, cropped as the stream says (1920x1080 of a coded 1920x1088)
    size: PhysicalSize<u32>,
    /// The visible picture's top-left corner in the coded one
    offset: (usize, usize),
    coded_height: usize,
    stride: usize,
    /// Bytes of an output picture when the decoder does not allocate its own samples
    output_size: Option<u32>,
}

impl Mft {
    fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .context("CoInitializeEx")?;
            MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET).context("MFStartup")?;
            let transform: IMFTransform =
                CoCreateInstance(&CLSID_MSH264DecoderMFT, None, CLSCTX_INPROC_SERVER)
                    .context("creating the H.264 decoder")?;
            let input = MFCreateMediaType()?;
            input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            transform
                .SetInputType(0, &input, 0)
                .context("the decoder refuses H.264 input")?;
            let mut mft = Self {
                transform,
                size: PhysicalSize::new(0, 0),
                offset: (0, 0),
                coded_height: 0,
                stride: 0,
                output_size: None,
            };
            mft.choose_output()?;
            mft.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            Ok(mft)
        }
    }

    /// Picks the NV12 output, and learns the picture's size and stride with it (again after a stream change)
    fn choose_output(&mut self) -> Result<()> {
        unsafe {
            let mut index = 0;
            let output: IMFMediaType = loop {
                let candidate = self
                    .transform
                    .GetOutputAvailableType(0, index)
                    .context("the decoder offers no NV12 output")?;
                if candidate.GetGUID(&MF_MT_SUBTYPE)? == MFVideoFormat_NV12 {
                    break candidate;
                }
                index += 1;
            };
            self.transform.SetOutputType(0, &output, 0)?;
            // before the first picture the decoder may not know the size yet
            if let Ok(packed) = output.GetUINT64(&MF_MT_FRAME_SIZE) {
                let (width, height) = ((packed >> 32) as u32, packed as u32);
                self.coded_height = height as usize;
                self.stride = output
                    .GetUINT32(&MF_MT_DEFAULT_STRIDE)
                    .map_or(width as usize, |stride| stride as usize);
                let mut area = MFVideoArea::default();
                let blob = std::slice::from_raw_parts_mut(
                    (&raw mut area).cast::<u8>(),
                    size_of::<MFVideoArea>(),
                );
                (self.size, self.offset) =
                    match output.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, blob, None) {
                        Ok(()) => (
                            PhysicalSize::new(area.Area.cx as u32, area.Area.cy as u32),
                            (area.OffsetX.value as usize, area.OffsetY.value as usize),
                        ),
                        Err(_) => (PhysicalSize::new(width, height), (0, 0)),
                    };
            }
            let info = self.transform.GetOutputStreamInfo(0)?;
            let provides = info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
            self.output_size = (!provides).then_some(info.cbSize);
            Ok(())
        }
    }

    fn run<S: Read + Seek>(
        &mut self,
        mut track: Mp4TrackReader<S>,
        frames: &SyncSender<Picture>,
    ) -> Result<()> {
        let timescale = track.get_mp4_track_info(|t| t.timescale()).max(1) as u64;
        let mut converter = track.get_mp4_track_info(Mp4BitstreamConverter::for_mp4_track);
        let mut timings = VecDeque::new();
        let mut buffer = Vec::new();
        let mut frame_number = 0;
        loop {
            // the pictures that are ready, before more input
            if !self.drain_output(&mut timings, frames)? {
                return Ok(());
            }
            let Some(sample) = track.next_sample().context("reading the movie")? else {
                break;
            };
            timings.push_back(FrameTiming {
                frame_number,
                start_time: sample.start_time,
                duration: sample.duration,
            });
            frame_number += 1;
            converter.convert_packet(&sample.bytes, &mut buffer);
            // in 100 ns units
            let time = (sample.start_time * 10_000_000 / timescale) as i64;
            let input = make_sample(&buffer, time)?;
            unsafe { self.transform.ProcessInput(0, &input, 0) }.context("decoding a sample")?;
        }
        unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)?;
            self.transform
                .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)?;
        }
        self.drain_output(&mut timings, frames)?;
        debug!("Media Foundation: the movie is decoded");
        Ok(())
    }

    /// Sends every picture the decoder has ready; false when the game no longer wants them
    fn drain_output(
        &mut self,
        timings: &mut VecDeque<FrameTiming>,
        frames: &SyncSender<Picture>,
    ) -> Result<bool> {
        loop {
            let sample = self.output_size.map(make_buffer_sample).transpose()?;
            let mut output = [MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(sample),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            }];
            let mut status = 0;
            let result = unsafe { self.transform.ProcessOutput(0, &mut output, &mut status) };
            let [output] = output;
            let sample = ManuallyDrop::into_inner(output.pSample);
            drop(ManuallyDrop::into_inner(output.pEvents));
            match result {
                Ok(()) => {
                    let sample =
                        sample.ok_or_else(|| anyhow!("the decoder returned no picture"))?;
                    let frame = self.read_picture(&sample)?;
                    // shortcut: a picture without a sample (never seen) gets a zero timing
                    let timing = timings.pop_front().unwrap_or(FrameTiming {
                        frame_number: 0,
                        start_time: 0,
                        duration: 0,
                    });
                    if frames.send(Ok((timing, frame))).is_err() {
                        return Ok(false);
                    }
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(true),
                Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => self.choose_output()?,
                Err(e) => return Err(e).context("taking a picture from the decoder"),
            }
        }
    }

    /// Copies an NV12 picture out of the decoder's buffer, dropping the padding past each row
    fn read_picture(&self, sample: &IMFSample) -> Result<Nv12Frame> {
        let (width, height) = (self.size.width as usize, self.size.height as usize);
        ensure!(width > 0 && height > 0, "the decoder gave no picture size");
        let stride = self.stride.max(width);
        unsafe {
            let buffer = sample.ConvertToContiguousBuffer()?;
            let mut data = std::ptr::null_mut();
            let mut length = 0;
            buffer.Lock(&mut data, None, Some(&mut length))?;
            let bytes = std::slice::from_raw_parts(data, length as usize);
            let (x, y) = self.offset;
            let rows = |from: usize, count: usize| {
                (0..count)
                    .flat_map(|row| {
                        bytes
                            .get(from + row * stride..from + row * stride + width)
                            .unwrap_or(&[])
                    })
                    .copied()
                    .collect::<Vec<u8>>()
            };
            // the interleaved chroma rows follow all the coded luma rows
            let y_plane = rows(y * stride + x, height);
            let uv_plane = rows(stride * self.coded_height + y / 2 * stride + x, height / 2);
            buffer.Unlock()?;
            ensure!(
                y_plane.len() == width * height && uv_plane.len() == width * (height / 2),
                "the decoder's picture is smaller than {width}x{height}"
            );
            Ok(Nv12Frame {
                y_plane,
                uv_plane,
                size: self.size,
            })
        }
    }
}

/// A sample holding `bytes`, at `time` (100 ns)
fn make_sample(bytes: &[u8], time: i64) -> Result<IMFSample> {
    unsafe {
        let buffer = MFCreateMemoryBuffer(bytes.len() as u32)?;
        let mut data = std::ptr::null_mut();
        buffer.Lock(&mut data, None, None)?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
        buffer.Unlock()?;
        buffer.SetCurrentLength(bytes.len() as u32)?;
        let sample = MFCreateSample()?;
        sample.AddBuffer(&buffer)?;
        sample.SetSampleTime(time)?;
        Ok(sample)
    }
}

/// An empty sample of `size` bytes for the decoder to write a picture into
fn make_buffer_sample(size: u32) -> Result<IMFSample> {
    unsafe {
        let buffer = MFCreateMemoryBuffer(size)?;
        let sample = MFCreateSample()?;
        sample.AddBuffer(&buffer)?;
        Ok(sample)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp4::Mp4;

    /// Decodes `SHIN_TEST_MOVIE` with Media Foundation and with ffmpeg and checks both give the same pictures, with
    /// the same timings, close in luma and chroma. Needs a movie from the user's own dump, so it only runs on request:
    /// `SHIN_TEST_MOVIE=<path> cargo test -p shin-video -- --ignored`
    #[test]
    #[ignore]
    fn media_foundation_matches_ffmpeg() {
        let path = std::env::var("SHIN_TEST_MOVIE").expect("set SHIN_TEST_MOVIE to an mp4 path");
        shin_tasks::create_task_pools();
        let open = || {
            Mp4::new(std::fs::File::open(&path).unwrap())
                .unwrap()
                .video_track
        };

        let mut mf = match MediaFoundationH264Decoder::start(open()) {
            Ok(decoder) => decoder,
            Err((e, _)) => panic!("Media Foundation: {e:?}"),
        };
        let mut ffmpeg = SpawnFfmpegH264Decoder::new(open()).unwrap();
        let mut count = 0;
        let mut worst = 0.0f64;
        loop {
            match (mf.read_frame().unwrap(), ffmpeg.read_frame().unwrap()) {
                (Some((ta, fa)), Some((tb, fb))) => {
                    assert_eq!(fa.size, fb.size);
                    assert_eq!(
                        (ta.frame_number, ta.start_time, ta.duration),
                        (tb.frame_number, tb.start_time, tb.duration)
                    );
                    assert_eq!(fa.uv_plane.len(), fb.uv_plane.len());
                    let (a, b) = (
                        fa.y_plane.iter().chain(&fa.uv_plane),
                        fb.y_plane.iter().chain(&fb.uv_plane),
                    );
                    let diff = a
                        .zip(b)
                        .map(|(&x, &y)| (x as f64 - y as f64).abs())
                        .sum::<f64>()
                        / (fa.y_plane.len() + fa.uv_plane.len()) as f64;
                    worst = worst.max(diff);
                    count += 1;
                }
                (None, None) => break,
                (a, b) => panic!(
                    "picture {count}: Media Foundation has one: {}, ffmpeg has one: {}",
                    a.is_some(),
                    b.is_some()
                ),
            }
        }
        println!("{count} pictures, worst mean difference {worst:.2}");
        assert!(count > 0);
        assert!(worst < 2.0, "the pictures differ: {worst}");
    }
}
