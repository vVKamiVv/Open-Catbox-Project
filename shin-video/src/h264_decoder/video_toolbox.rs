//! macOS: VideoToolbox, the system's own H.264 decoder, on a thread per movie; ffmpeg on the PATH when it cannot start,
//! else no movies (see `none.rs`).
//!
//! VideoToolbox takes the MP4's samples as they are (length-prefixed NAL units) with a format description made from
//! the track's SPS and PPS. With temporal processing it hands the pictures out in display order, each with the timing
//! its sample carried in (`sourceFrameRefCon`), as NV12 in video range: the layout the movie shader expects.

use std::{
    collections::{BTreeMap, VecDeque},
    ffi::c_void,
    io::{Read, Seek},
    ptr::{null, null_mut},
    sync::{
        Mutex,
        mpsc::{Receiver, SyncSender},
    },
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use dpi::PhysicalSize;
use tracing::{debug, warn};

use crate::{
    h264_decoder::{
        FrameTiming, H264DecoderTrait, Nv12Frame,
        none::NoH264Decoder,
        spawn_ffmpeg::{SpawnFfmpegH264Decoder, ffmpeg_available},
    },
    mp4::Mp4TrackReader,
};

/// Pictures decoded ahead of the game (about 3 MB each at 1920x1080)
const FRAMES_AHEAD: usize = 8;
/// VideoToolbox gives pictures in decode order: up to this many wait to be put in display order (H.264 never reorders
/// across more than its largest picture buffer, 16)
const REORDER: usize = 16;

/// VideoToolbox, or ffmpeg when it cannot start, or none
pub enum AutoH264Decoder {
    VideoToolbox(VideoToolboxH264Decoder),
    Ffmpeg(SpawnFfmpegH264Decoder),
    None(NoH264Decoder),
}

impl H264DecoderTrait for AutoH264Decoder {
    fn new<S: Read + Seek + Send + 'static>(track: Mp4TrackReader<S>) -> Result<Self> {
        match VideoToolboxH264Decoder::start(track) {
            Ok(decoder) => Ok(Self::VideoToolbox(decoder)),
            Err((e, track)) if ffmpeg_available() => {
                warn!("VideoToolbox cannot decode the movie ({e:?}); trying ffmpeg");
                Ok(Self::Ffmpeg(SpawnFfmpegH264Decoder::new(track)?))
            }
            Err((e, track)) => Ok(Self::None(NoH264Decoder::with_reason(
                track,
                &format!("VideoToolbox cannot decode H.264 ({e:?})"),
            ))),
        }
    }

    fn read_frame(&mut self) -> Result<Option<(FrameTiming, Nv12Frame)>> {
        match self {
            Self::VideoToolbox(decoder) => decoder.read_frame(),
            Self::Ffmpeg(decoder) => decoder.read_frame(),
            Self::None(decoder) => decoder.read_frame(),
        }
    }

    fn frame_size(&mut self) -> Result<PhysicalSize<u32>> {
        match self {
            Self::VideoToolbox(decoder) => decoder.frame_size(),
            Self::Ffmpeg(decoder) => decoder.frame_size(),
            Self::None(decoder) => decoder.frame_size(),
        }
    }
}

type Picture = Result<(FrameTiming, Nv12Frame)>;

pub struct VideoToolboxH264Decoder {
    frames: Receiver<Picture>,
    /// A picture read to learn the size, not yet returned
    peeked: Option<Picture>,
    size: Option<PhysicalSize<u32>>,
}

impl VideoToolboxH264Decoder {
    /// Starts the decoding thread; gives the track back when VideoToolbox cannot be set up for it
    fn start<S: Read + Seek + Send + 'static>(
        track: Mp4TrackReader<S>,
    ) -> std::result::Result<Self, (anyhow::Error, Mp4TrackReader<S>)> {
        let (ready_sender, ready_receiver) = std::sync::mpsc::channel();
        let (frame_sender, frames) = std::sync::mpsc::sync_channel(FRAMES_AHEAD);
        std::thread::Builder::new()
            .name("video-toolbox-h264".to_string())
            .spawn(move || {
                let session = match Session::new(&track) {
                    Ok(session) => session,
                    Err(e) => {
                        let _ = ready_sender.send(Err((e, track)));
                        return;
                    }
                };
                let _ = ready_sender.send(Ok(()));
                if let Err(e) = session.run(track, &frame_sender) {
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

/// The pictures the output callback has made since they were last taken
type Ready = Mutex<VecDeque<Picture>>;

/// A decompression session, owned by its thread
struct Session {
    session: VTDecompressionSessionRef,
    format: CMFormatDescriptionRef,
    /// Boxed so its address, given to the callback, stays put
    ready: Box<Ready>,
    /// Decoded pictures by presentation time, until no earlier one can come
    reorder: BTreeMap<u64, (FrameTiming, Nv12Frame)>,
}

impl Session {
    fn new<S: Read + Seek>(track: &Mp4TrackReader<S>) -> Result<Self> {
        let (parameter_sets, length_size) = track.get_mp4_track_info(|track| {
            let config = &track
                .trak
                .mdia
                .minf
                .stbl
                .stsd
                .avc1
                .as_ref()
                .context("the movie is not H.264 (no avc1 configuration)")?
                .avcc;
            let sets: Vec<Vec<u8>> = config
                .sequence_parameter_sets
                .iter()
                .chain(&config.picture_parameter_sets)
                .map(|set| set.bytes.clone())
                .collect();
            anyhow::Ok((sets, config.length_size_minus_one as i32 + 1))
        })?;
        ensure!(parameter_sets.len() >= 2, "the movie has no SPS and PPS");
        let pointers: Vec<*const u8> = parameter_sets.iter().map(|set| set.as_ptr()).collect();
        let sizes: Vec<usize> = parameter_sets.iter().map(Vec::len).collect();

        unsafe {
            let mut format = null_mut();
            check(
                CMVideoFormatDescriptionCreateFromH264ParameterSets(
                    null(),
                    pointers.len(),
                    pointers.as_ptr(),
                    sizes.as_ptr(),
                    length_size,
                    &mut format,
                ),
                "making the format description",
            )?;

            // NV12 in video range ('420v')
            let pixel_format = CFNumberCreate(
                null(),
                K_CF_NUMBER_SINT32_TYPE,
                (&raw const PIXEL_FORMAT_NV12_VIDEO_RANGE).cast(),
            );
            let keys = [kCVPixelBufferPixelFormatTypeKey];
            let values = [pixel_format.cast_const()];
            let attributes = CFDictionaryCreate(
                null(),
                keys.as_ptr(),
                values.as_ptr(),
                1,
                &raw const kCFTypeDictionaryKeyCallBacks,
                &raw const kCFTypeDictionaryValueCallBacks,
            );
            CFRelease(pixel_format.cast_const());

            let ready: Box<Ready> = Box::new(Mutex::new(VecDeque::new()));
            let callback = VTDecompressionOutputCallbackRecord {
                decompression_output_callback: output_callback,
                decompression_output_ref_con: (&raw const *ready).cast_mut().cast(),
            };
            let mut session = null_mut();
            let status = VTDecompressionSessionCreate(
                null(),
                format,
                null(),
                attributes,
                &callback,
                &mut session,
            );
            CFRelease(attributes);
            if let Err(e) = check(status, "starting the decoder") {
                CFRelease(format.cast_const());
                return Err(e);
            }
            Ok(Self {
                session,
                format,
                ready,
                reorder: BTreeMap::new(),
            })
        }
    }

    fn run<S: Read + Seek>(
        mut self,
        mut track: Mp4TrackReader<S>,
        frames: &SyncSender<Picture>,
    ) -> Result<()> {
        let timescale = track.get_mp4_track_info(|t| t.timescale()).max(1) as i32;
        let mut frame_number = 0;
        while let Some(sample) = track.next_sample().context("reading the movie")? {
            // a picture is shown at its presentation time: the decode time plus the sample's offset (B-frames)
            let timing = FrameTiming {
                frame_number,
                start_time: (sample.start_time as i64 + sample.rendering_offset as i64) as u64,
                duration: sample.duration,
            };
            frame_number += 1;
            self.decode(&sample.bytes, timing, sample.start_time, timescale)?;
            if !self.send_ready(frames, false) {
                return Ok(());
            }
        }
        unsafe {
            check(
                VTDecompressionSessionFinishDelayedFrames(self.session),
                "finishing the movie",
            )?;
            check(
                VTDecompressionSessionWaitForAsynchronousFrames(self.session),
                "finishing the movie",
            )?;
        }
        self.send_ready(frames, true);
        debug!("VideoToolbox: the movie is decoded");
        Ok(())
    }

    /// Hands one sample to the decoder; its pictures come out through the callback
    fn decode(
        &self,
        bytes: &[u8],
        timing: FrameTiming,
        decode_time: u64,
        timescale: i32,
    ) -> Result<()> {
        unsafe {
            // a block buffer that owns a copy of the sample
            let mut block = null_mut();
            check(
                CMBlockBufferCreateWithMemoryBlock(
                    null(),
                    null_mut(),
                    bytes.len(),
                    null(),
                    null(),
                    0,
                    bytes.len(),
                    K_CM_BLOCK_BUFFER_ASSURE_MEMORY_NOW,
                    &mut block,
                ),
                "making a block buffer",
            )?;
            let copied =
                CMBlockBufferReplaceDataBytes(bytes.as_ptr().cast(), block, 0, bytes.len());
            if let Err(e) = check(copied, "filling a block buffer") {
                CFRelease(block.cast_const());
                return Err(e);
            }
            let time = |value: u64| CMTime {
                value: value as i64,
                timescale,
                flags: K_CM_TIME_FLAGS_VALID,
                epoch: 0,
            };
            let sample_timing = CMSampleTimingInfo {
                duration: time(timing.duration as u64),
                presentation_time_stamp: time(timing.start_time),
                decode_time_stamp: time(decode_time),
            };
            let size = bytes.len();
            let mut sample = null_mut();
            let made = CMSampleBufferCreateReady(
                null(),
                block,
                self.format,
                1,
                1,
                &sample_timing,
                1,
                &size,
                &mut sample,
            );
            CFRelease(block.cast_const());
            check(made, "making a sample buffer")?;

            // the callback takes the timing back
            let timing = Box::into_raw(Box::new(timing));
            let mut info = 0;
            let status = VTDecompressionSessionDecodeFrame(
                self.session,
                sample,
                K_VT_DECODE_FRAME_ENABLE_TEMPORAL_PROCESSING,
                timing.cast(),
                &mut info,
            );
            CFRelease(sample.cast_const());
            if status != 0 && info & K_VT_DECODE_INFO_FRAME_DROPPED == 0 {
                // the callback may not have run: the timing is still ours only if it did not
                return Err(anyhow!("decoding a sample (OSStatus {status})"));
            }
            Ok(())
        }
    }

    /// Sends the pictures the callback made, in display order: the earliest one once more than `REORDER` wait, all of
    /// them at the end of the movie (`last`); false when the game no longer wants them
    fn send_ready(&mut self, frames: &SyncSender<Picture>, last: bool) -> bool {
        let Ok(mut ready) = self.ready.lock() else {
            return false;
        };
        let pictures: Vec<Picture> = ready.drain(..).collect();
        drop(ready);
        for picture in pictures {
            match picture {
                Ok((timing, frame)) => {
                    self.reorder.insert(timing.start_time, (timing, frame));
                }
                Err(e) => {
                    if frames.send(Err(e)).is_err() {
                        return false;
                    }
                }
            }
        }
        while self.reorder.len() > if last { 0 } else { REORDER } {
            let (_, picture) = self.reorder.pop_first().unwrap();
            if frames.send(Ok(picture)).is_err() {
                return false;
            }
        }
        true
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            VTDecompressionSessionInvalidate(self.session);
            CFRelease(self.session.cast_const());
            CFRelease(self.format.cast_const());
        }
    }
}

/// Called by VideoToolbox for every picture (on the decoding thread, during a decode or the final flush)
extern "C" fn output_callback(
    ready: *mut c_void,
    timing: *mut c_void,
    status: OSStatus,
    _info: u32,
    image: CVImageBufferRef,
    _presentation_time: CMTime,
    _presentation_duration: CMTime,
) {
    // SAFETY: `timing` is the box `decode` gave up for this sample, taken back exactly once here; `ready` is the
    // session's queue, which outlives the session
    let timing = unsafe { Box::from_raw(timing.cast::<FrameTiming>()) };
    let ready = unsafe { &*ready.cast::<Ready>() };
    let picture = if status != 0 {
        Err(anyhow!(
            "the decoder failed on a picture (OSStatus {status})"
        ))
    } else if image.is_null() {
        // a dropped picture: nothing to show
        return;
    } else {
        unsafe { copy_picture(image) }.map(|frame| (*timing, frame))
    };
    // no panic may cross into VideoToolbox: a poisoned queue only means the game has stopped reading
    if let Ok(mut ready) = ready.lock() {
        ready.push_back(picture);
    }
}

/// Copies the NV12 planes out of a pixel buffer, dropping the padding past each row
unsafe fn copy_picture(image: CVImageBufferRef) -> Result<Nv12Frame> {
    unsafe {
        check(
            CVPixelBufferLockBaseAddress(image, K_CV_PIXEL_BUFFER_LOCK_READ_ONLY),
            "reading a picture",
        )?;
        let width = CVPixelBufferGetWidth(image);
        let height = CVPixelBufferGetHeight(image);
        let plane = |index: usize, rows: usize| -> Vec<u8> {
            let base = CVPixelBufferGetBaseAddressOfPlane(image, index).cast::<u8>();
            let stride = CVPixelBufferGetBytesPerRowOfPlane(image, index);
            let mut out = Vec::with_capacity(width * rows);
            for row in 0..rows {
                out.extend_from_slice(std::slice::from_raw_parts(base.add(row * stride), width));
            }
            out
        };
        // NV12: the full-size luma plane, then interleaved chroma at half the height (and half the width, in pairs)
        let y_plane = plane(0, height);
        let uv_plane = plane(1, height.div_ceil(2));
        CVPixelBufferUnlockBaseAddress(image, K_CV_PIXEL_BUFFER_LOCK_READ_ONLY);
        Ok(Nv12Frame {
            y_plane,
            uv_plane,
            size: PhysicalSize::new(width as u32, height as u32),
        })
    }
}

fn check(status: OSStatus, what: &str) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(anyhow!("{what} (OSStatus {status})"))
    }
}

// --- the C API (CoreFoundation, CoreMedia, CoreVideo, VideoToolbox) ---------------------------------------------

type OSStatus = i32;
type CFTypeRef = *const c_void;
type CFAllocatorRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFStringRef = *const c_void;
type CFNumberRef = *mut c_void;
type CMFormatDescriptionRef = *mut c_void;
type CMBlockBufferRef = *mut c_void;
type CMSampleBufferRef = *mut c_void;
type CVImageBufferRef = *mut c_void;
type VTDecompressionSessionRef = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct CMTime {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}

#[repr(C)]
struct CMSampleTimingInfo {
    duration: CMTime,
    presentation_time_stamp: CMTime,
    decode_time_stamp: CMTime,
}

#[repr(C)]
struct VTDecompressionOutputCallbackRecord {
    decompression_output_callback:
        extern "C" fn(*mut c_void, *mut c_void, OSStatus, u32, CVImageBufferRef, CMTime, CMTime),
    decompression_output_ref_con: *mut c_void,
}

/// CFDictionaryKeyCallBacks (version, retain, release, copyDescription, equal, hash): only its address is used
#[repr(C)]
struct CFDictionaryKeyCallBacks {
    _fields: [usize; 6],
}

/// CFDictionaryValueCallBacks (the same without hash): only its address is used
#[repr(C)]
struct CFDictionaryValueCallBacks {
    _fields: [usize; 5],
}

const K_CM_TIME_FLAGS_VALID: u32 = 1;
const K_CM_BLOCK_BUFFER_ASSURE_MEMORY_NOW: u32 = 1;
const K_CF_NUMBER_SINT32_TYPE: isize = 3;
/// `kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange`, '420v'
static PIXEL_FORMAT_NV12_VIDEO_RANGE: i32 = 0x3432_3076;
const K_CV_PIXEL_BUFFER_LOCK_READ_ONLY: u64 = 1;
const K_VT_DECODE_FRAME_ENABLE_TEMPORAL_PROCESSING: u32 = 1 << 3;
const K_VT_DECODE_INFO_FRAME_DROPPED: u32 = 1 << 1;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFTypeDictionaryKeyCallBacks: CFDictionaryKeyCallBacks;
    static kCFTypeDictionaryValueCallBacks: CFDictionaryValueCallBacks;
    fn CFDictionaryCreate(
        allocator: CFAllocatorRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        count: isize,
        key_callbacks: *const CFDictionaryKeyCallBacks,
        value_callbacks: *const CFDictionaryValueCallBacks,
    ) -> CFDictionaryRef;
    fn CFNumberCreate(allocator: CFAllocatorRef, kind: isize, value: *const c_void) -> CFNumberRef;
    fn CFRelease(object: CFTypeRef);
}

#[link(name = "CoreMedia", kind = "framework")]
unsafe extern "C" {
    fn CMVideoFormatDescriptionCreateFromH264ParameterSets(
        allocator: CFAllocatorRef,
        parameter_set_count: usize,
        parameter_set_pointers: *const *const u8,
        parameter_set_sizes: *const usize,
        nal_unit_header_length: i32,
        format_description_out: *mut CMFormatDescriptionRef,
    ) -> OSStatus;
    fn CMBlockBufferCreateWithMemoryBlock(
        structure_allocator: CFAllocatorRef,
        memory_block: *mut c_void,
        block_length: usize,
        block_allocator: CFAllocatorRef,
        custom_block_source: *const c_void,
        offset_to_data: usize,
        data_length: usize,
        flags: u32,
        block_buffer_out: *mut CMBlockBufferRef,
    ) -> OSStatus;
    fn CMBlockBufferReplaceDataBytes(
        source_bytes: *const c_void,
        destination_buffer: CMBlockBufferRef,
        offset_into_destination: usize,
        data_length: usize,
    ) -> OSStatus;
    fn CMSampleBufferCreateReady(
        allocator: CFAllocatorRef,
        data_buffer: CMBlockBufferRef,
        format_description: CMFormatDescriptionRef,
        num_samples: isize,
        num_sample_timing_entries: isize,
        sample_timing_array: *const CMSampleTimingInfo,
        num_sample_size_entries: isize,
        sample_size_array: *const usize,
        sample_buffer_out: *mut CMSampleBufferRef,
    ) -> OSStatus;
}

#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {
    static kCVPixelBufferPixelFormatTypeKey: CFStringRef;
    fn CVPixelBufferLockBaseAddress(pixel_buffer: CVImageBufferRef, flags: u64) -> OSStatus;
    fn CVPixelBufferUnlockBaseAddress(pixel_buffer: CVImageBufferRef, flags: u64) -> OSStatus;
    fn CVPixelBufferGetWidth(pixel_buffer: CVImageBufferRef) -> usize;
    fn CVPixelBufferGetHeight(pixel_buffer: CVImageBufferRef) -> usize;
    fn CVPixelBufferGetBaseAddressOfPlane(
        pixel_buffer: CVImageBufferRef,
        plane: usize,
    ) -> *mut c_void;
    fn CVPixelBufferGetBytesPerRowOfPlane(pixel_buffer: CVImageBufferRef, plane: usize) -> usize;
}

#[link(name = "VideoToolbox", kind = "framework")]
unsafe extern "C" {
    fn VTDecompressionSessionCreate(
        allocator: CFAllocatorRef,
        video_format_description: CMFormatDescriptionRef,
        video_decoder_specification: CFDictionaryRef,
        destination_image_buffer_attributes: CFDictionaryRef,
        output_callback: *const VTDecompressionOutputCallbackRecord,
        decompression_session_out: *mut VTDecompressionSessionRef,
    ) -> OSStatus;
    fn VTDecompressionSessionDecodeFrame(
        session: VTDecompressionSessionRef,
        sample_buffer: CMSampleBufferRef,
        decode_flags: u32,
        source_frame_ref_con: *mut c_void,
        info_flags_out: *mut u32,
    ) -> OSStatus;
    fn VTDecompressionSessionFinishDelayedFrames(session: VTDecompressionSessionRef) -> OSStatus;
    fn VTDecompressionSessionWaitForAsynchronousFrames(
        session: VTDecompressionSessionRef,
    ) -> OSStatus;
    fn VTDecompressionSessionInvalidate(session: VTDecompressionSessionRef);
}
