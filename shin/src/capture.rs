//! `--capture-messages`: fast-forwards the script to given lines of text and saves the screen as a PNG once each has
//! typed out, for comparison with the same lines captured from the original by
//! `research/tools/compare/eden_capture.py`. Message ids are the script's, as `sdu` prints them.

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::mpsc,
};

use anyhow::Context as _;
use shin_core::time::Ticks;
use shin_render::render_texture::RenderTexture;
use tracing::{info, warn};

use crate::{adv::Adv, render::PreRenderContext};

/// Where the capture is; the ADV drives it
#[derive(Debug)]
pub struct CaptureTargets {
    remaining: VecDeque<u32>,
    settle: Ticks,
    state: TargetState,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TargetState {
    /// Fast-forwarding to the next target
    Seeking,
    /// The target line has started and is typing out
    Typing,
    /// Typed out; animations get this much more time to settle
    Settling(Ticks),
    /// Ready to be saved
    Due,
}

impl CaptureTargets {
    pub fn new(targets: Vec<u32>, settle_seconds: f32) -> Self {
        Self {
            remaining: targets.into(),
            settle: Ticks::from_seconds(settle_seconds),
            state: TargetState::Seeking,
        }
    }

    pub fn is_seeking(&self) -> bool {
        self.state == TargetState::Seeking && !self.remaining.is_empty()
    }

    /// Called with each line as it starts
    pub fn on_line(&mut self, message_id: u32) {
        if self.state != TargetState::Seeking {
            return;
        }
        while self
            .remaining
            .front()
            .is_some_and(|&target| target < message_id)
        {
            warn!(
                "Line {} came before line {message_id}; it is not captured",
                self.remaining[0]
            );
            self.remaining.pop_front();
        }
        if self.remaining.front() == Some(&message_id) {
            info!("Reached line {message_id}; letting it type out");
            self.state = TargetState::Typing;
        }
    }

    /// Called once per frame after the ADV has updated
    pub fn update(&mut self, delta: Ticks, waiting_for_click: bool) {
        self.state = match self.state {
            TargetState::Typing if waiting_for_click => TargetState::Settling(self.settle),
            TargetState::Settling(left) if left - delta <= Ticks::ZERO => TargetState::Due,
            TargetState::Settling(left) => TargetState::Settling(left - delta),
            state => state,
        };
    }

    /// The line to save now, if any
    pub fn due(&self) -> Option<u32> {
        (self.state == TargetState::Due).then(|| self.remaining[0])
    }

    /// Moves on after the due line has been saved; false when it was the last one
    pub fn finish_due(&mut self) -> bool {
        self.remaining.pop_front();
        self.state = TargetState::Seeking;
        !self.remaining.is_empty()
    }
}

/// A frame copied into a readable buffer; save it once the frame has been submitted
pub struct PendingCapture {
    message_id: u32,
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    bytes_per_row: u32,
    _texture: RenderTexture,
}

/// Renders the ADV into a canvas-sized texture and queues a copy of it into a buffer
pub fn render(ctx: &mut PreRenderContext, adv: &Adv, message_id: u32) -> PendingCapture {
    let mut texture = ctx.new_render_texture(format!("capture/{message_id}"));
    let depth_stencil = ctx.depth_stencil;
    {
        let mut pass = ctx.begin_pass(texture.as_texture_target(), Some(depth_stencil), "capture");
        adv.render(&mut pass);
    }

    let gpu_texture = texture.texture();
    let (width, height) = (gpu_texture.width(), gpu_texture.height());
    let bytes_per_row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("capture/readback"),
        size: u64::from(bytes_per_row) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    ctx.encoder.copy_texture_to_buffer(
        gpu_texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: None,
            },
        },
        gpu_texture.size(),
    );

    PendingCapture {
        message_id,
        buffer,
        width,
        height,
        bytes_per_row,
        _texture: texture,
    }
}

impl PendingCapture {
    /// Waits for the copy and saves `<dir>/<id>.png`
    pub fn save(self, device: &wgpu::Device, dir: &Path) -> anyhow::Result<PathBuf> {
        let slice = self.buffer.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        let _ = device.poll(wgpu::Maintain::Wait);
        receiver
            .recv()
            .context("the readback never finished")?
            .context("mapping the readback buffer failed")?;

        let image = {
            let data = slice.get_mapped_range();
            // the canvas is opaque, so alpha is dropped
            image::RgbImage::from_fn(self.width, self.height, |x, y| {
                let offset = (y * self.bytes_per_row + x * 4) as usize;
                image::Rgb([data[offset], data[offset + 1], data[offset + 2]])
            })
        };
        self.buffer.unmap();

        let path = dir.join(format!("{}.png", self.message_id));
        image
            .save(&path)
            .with_context(|| format!("saving {}", path.display()))?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_through_targets() {
        let frame = Ticks::from_seconds(1.0 / 60.0);
        let mut capture = CaptureTargets::new(vec![5, 7], 0.025);
        assert!(capture.is_seeking());

        capture.on_line(4);
        assert!(capture.is_seeking());
        capture.on_line(5);
        assert!(!capture.is_seeking());
        capture.update(frame, false);
        assert_eq!(capture.due(), None); // still typing
        capture.update(frame, true);
        capture.update(frame, true);
        assert_eq!(capture.due(), None); // settling
        capture.update(frame, true);
        assert_eq!(capture.due(), Some(5));
        assert!(capture.finish_due());

        // a target the script jumps over is dropped
        capture.on_line(8);
        assert_eq!(capture.due(), None);
        assert!(!capture.is_seeking());
    }
}
