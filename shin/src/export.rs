//! Saving from the galleries (port-only): the Picture Box viewer's picture, the Music Box's track, the Movie Box's
//! movie and the Character Box's sprite and background, written under the user's Pictures, Music and Videos folders
//! on a background thread. Each screen gets a "Save" entry in its key help (the Save action, F5, P or ZR by default,
//! or −, or a click on it), and a short notice tells where the file went. See `research/docs/engine-internals.md` §19.

use std::{
    cell::Cell,
    panic::AssertUnwindSafe,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, anyhow};
use enum_map::EnumMap;
use glam::{Mat4, vec3};
use image::RgbaImage;
use shin_audio::AudioManager;
use shin_core::{
    format::{
        audio::{AudioSource, read_audio},
        bustup::{default_builder::DefaultBustupBuilder, read_bustup},
        picture::{SimpleMergedPicture, read_picture},
    },
    layout::LayoutParams,
    primitives::color::FloatColor4,
    vm::command::types::Volume,
};
use shin_input::{ActionState, Pointer};
use shin_render::{gpu_texture::GpuTexture, render_pass::RenderPass};
use shin_tasks::ComputeTask;
use tracing::{info, warn};

use crate::{
    adv::hud::{OUTLINE, text_layout},
    app::AppAction,
    asset::{
        font::GpuFontLazy,
        system::{AssetDataAccessor, AssetServer},
    },
    audio::play_sys_se,
    layer::{text_block::TextBlock, user::minigame},
    render::PreRenderContext,
    ui,
};

const BORDER: FloatColor4 = FloatColor4::from_rgba(0.1843, 0.0980, 0.0784, 1.0);
/// The notice shows for 2 s, the last half second fading
const NOTICE_FRAMES: f32 = 120.0;
const FADE_FRAMES: f32 = 30.0;
/// The `key` atlas' − glyph
const SAVE_GLYPH: u8 = 13;

pub struct Exporter {
    font: Arc<GpuFontLazy>,
    label: Option<TextBlock>,
    /// The notice and its frames left
    notice: Option<(TextBlock, f32)>,
    task: Option<ComputeTask<Result<PathBuf>>>,
    /// The Save entry as last drawn (empty while hidden), for the pointer
    hit: Cell<[f32; 4]>,
    held: bool,
    /// The pointer went down on the Save entry
    on_entry: bool,
}

impl Exporter {
    pub fn new(font: Arc<GpuFontLazy>) -> Self {
        Self {
            font,
            label: None,
            notice: None,
            task: None,
            hit: Cell::new([0.0; 4]),
            held: false,
            on_entry: false,
        }
    }

    /// The Save action (F5, P, ZR by default); − (the keyboard's -), unused by the galleries, also still saves
    pub fn pressed(state: &EnumMap<AppAction, ActionState>) -> bool {
        state[AppAction::Minus].is_clicked || state[AppAction::Save].is_clicked
    }

    /// The pointer as the screen should see it: a press on the Save entry is the exporter's alone. True when the
    /// entry was clicked (released over it).
    pub fn pointer(&mut self, pointer: Pointer) -> (Pointer, bool) {
        let inside = pointer
            .position
            .is_some_and(|at| minigame::contains(self.hit.get(), at));
        if pointer.held && !self.held {
            self.on_entry = inside;
        }
        self.held = pointer.held;
        let on_entry = self.on_entry;
        if !pointer.held {
            self.on_entry = false;
        }
        if on_entry {
            let released = Pointer {
                held: false,
                ..pointer
            };
            (released, !pointer.held && inside)
        } else {
            (pointer, false)
        }
    }

    /// Runs `job` (which returns the folder it saved to) on a background thread, unless one is still running
    pub fn start(&mut self, job: impl FnOnce() -> Result<PathBuf> + Send + 'static) {
        if self.task.is_some() {
            return;
        }
        self.task = Some(shin_tasks::compute::spawn(move || {
            // a panic would abort the thread pool
            std::panic::catch_unwind(AssertUnwindSafe(job))
                .unwrap_or_else(|_| Err(anyhow!("the file could not be decoded")))
        }));
    }

    /// Each frame: the finished job's notice and sound ("decide", or "error" with the error as the notice)
    pub fn update(
        &mut self,
        ctx: &PreRenderContext,
        audio: &AudioManager,
        assets: &AssetServer,
        frames: f32,
    ) {
        let layout = LayoutParams {
            always_leave_space_for_rubi: false,
            ..text_layout(1800.0, 30.0)
        };
        if self.label.is_none() {
            self.label = Some(TextBlock::new(ctx, &self.font, "Save", layout, OUTLINE));
        }
        if let Some((_, left)) = &mut self.notice {
            *left -= frames;
            if *left <= 0.0 {
                self.notice = None;
            }
        }
        let Some(result) = self.task.as_mut().and_then(ComputeTask::poll_naive) else {
            return;
        };
        self.task = None;
        let (text, sound) = match result {
            Ok(dir) => (format!("Saved to {}", shown(&dir).display()), "decide"),
            Err(e) => {
                warn!("Saving from the gallery failed: {e:?}");
                (format!("Could not save: {e}"), "error")
            }
        };
        play_sys_se(audio, assets, sound, Volume::default());
        self.notice = Some((
            TextBlock::new(ctx, &self.font, &text, layout, OUTLINE),
            NOTICE_FRAMES,
        ));
    }

    /// The Save entry ending at `right`, glyph left of the label as in the key help; returns the next entry's right
    /// edge
    pub fn draw_entry(
        &self,
        pass: &mut RenderPass,
        keys: &GpuTexture,
        right: f32,
        y: f32,
        color: FloatColor4,
    ) -> f32 {
        let Some(label) = &self.label else {
            return right;
        };
        let projection = ui::projection();
        let size = label.size();
        let text_x = right - size.x;
        ui::draw_text(
            pass,
            label,
            projection * Mat4::from_translation(vec3(text_x, y + (48.0 - size.y) / 2.0, 0.0)),
            color,
            BORDER.with_alpha(color.a),
        );
        let x = text_x - 52.0;
        let rect = [x, y, x + 48.0, y + 48.0];
        ui::prompts::draw(
            pass,
            ui::builder(),
            projection,
            keys,
            SAVE_GLYPH,
            rect,
            color,
        );
        self.hit.set([x, y, right, y + 48.0]);
        x - 16.0
    }

    /// The Save entry is not on screen: the pointer cannot click it
    pub fn hide_entry(&self) {
        self.hit.set([0.0; 4]);
    }

    /// The notice, right-aligned above the key help
    pub fn draw_notice(&self, pass: &mut RenderPass) {
        let Some((text, left)) = &self.notice else {
            return;
        };
        let a = (left / FADE_FRAMES).min(1.0);
        let x = 1872.0 - text.size().x;
        ui::draw_text(
            pass,
            text,
            ui::projection() * Mat4::from_translation(vec3(x, 950.0, 0.0)),
            FloatColor4::WHITE.with_alpha(a),
            BORDER.with_alpha(a),
        );
    }
}

/// `<base>/Umineko`, `base` being one of the user's folders (Pictures, Music, Videos); the data dir without one
pub fn folder(base: Option<PathBuf>) -> PathBuf {
    base.or_else(|| dirs_next::data_dir().map(|dir| dir.join("shin")))
        .unwrap_or_default()
        .join("Umineko")
}

/// A name made safe for a file name
pub fn file_name(name: &str) -> String {
    let name = name
        .chars()
        .map(|c| {
            if c.is_control() || r#"<>:"/\|?*"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect::<String>();
    name.trim().trim_end_matches('.').to_string()
}

/// A folder as the notice shows it, from the user's folder by its English name ("Pictures\Umineko\Pictures"): the
/// font has no glyphs for a localised one ("Imágenes")
fn shown(dir: &Path) -> PathBuf {
    let known = [
        (dirs_next::picture_dir(), "Pictures"),
        (dirs_next::audio_dir(), "Music"),
        (dirs_next::video_dir(), "Videos"),
        (dirs_next::home_dir(), ""),
    ];
    known
        .into_iter()
        .find_map(|(base, name)| Some(Path::new(name).join(dir.strip_prefix(base?).ok()?)))
        .unwrap_or_else(|| dir.to_path_buf())
}

/// Writes `file` (creating its folder) and returns the folder
fn write(file: &Path, write: impl FnOnce(&Path) -> Result<()>) -> Result<PathBuf> {
    let dir = file.parent().context("no folder")?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    write(file).with_context(|| format!("writing {}", file.display()))?;
    info!("Saved {}", file.display());
    Ok(dir.to_path_buf())
}

/// A `.pic` decoded whole, at full resolution
pub fn decode_picture(data: &AssetDataAccessor) -> Result<RgbaImage> {
    Ok(read_picture::<SimpleMergedPicture>(&data.read_all_sync(), ())?.image)
}

/// A `.bup`'s base with `emotion`'s face, its first mouth and its first eyes, as the sprite shows at rest
pub fn compose_bustup(data: &AssetDataAccessor, emotion: &str) -> Result<RgbaImage> {
    let bustup = read_bustup::<DefaultBustupBuilder>(&data.read_all_sync(), ())?;
    let mut image = bustup.base_image;
    if let Some(expression) = bustup.expressions.get(emotion) {
        let blocks = [&expression.face1, &expression.face2]
            .into_iter()
            .flatten()
            .chain(expression.mouths.first())
            .chain(expression.eyes.first());
        for block in blocks {
            let (x, y) = (block.offset_x as i64, block.offset_y as i64);
            image::imageops::overlay(&mut image, &block.data, x, y);
        }
    }
    Ok(image)
}

pub fn save_png(image: &RgbaImage, file: &Path) -> Result<PathBuf> {
    write(file, |file| Ok(image.save(file)?))
}

/// An `.nxa` track as a 16-bit WAV, once through (the loop is not repeated), titled `title`
pub fn save_track(data: &AssetDataAccessor, title: &str, file: &Path) -> Result<PathBuf> {
    let audio = read_audio(&data.read_all_sync())?;
    let info = audio.info().clone();
    let mut source = AudioSource::new(audio.decode()?);
    let pcm = |s: f32| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
    let mut samples = Vec::with_capacity(info.num_samples as usize * info.channel_count as usize);
    while let Some((left, right)) = source.read_sample() {
        if samples.len() >= info.num_samples as usize * info.channel_count as usize {
            break;
        }
        samples.push(pcm(left));
        if info.channel_count == 2 {
            samples.push(pcm(right));
        }
    }
    let bytes = wav(info.channel_count, info.sample_rate, &samples, title);
    write(file, |file| Ok(std::fs::write(file, bytes)?))
}

/// The asset's bytes as they are (the Movie Box's `.mp4`)
pub fn save_raw(data: &AssetDataAccessor, file: &Path) -> Result<PathBuf> {
    write(file, |file| {
        std::io::copy(&mut data.cursor(), &mut std::fs::File::create(file)?)?;
        Ok(())
    })
}

/// A PCM WAV with a LIST/INFO title (INAM)
fn wav(channels: u16, sample_rate: u32, samples: &[i16], title: &str) -> Vec<u8> {
    let data_len = samples.len() as u32 * 2;
    let mut name = title.as_bytes().to_vec();
    name.push(0);
    let name_len = name.len() as u32;
    if name.len() % 2 == 1 {
        name.push(0);
    }
    let list_len = 4 + 8 + name.len() as u32;
    let mut out = Vec::with_capacity(64 + name.len() + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(4 + (8 + 16) + (8 + list_len) + (8 + data_len)).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * channels as u32 * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"LIST");
    out.extend_from_slice(&list_len.to_le_bytes());
    out.extend_from_slice(b"INFOINAM");
    out.extend_from_slice(&name_len.to_le_bytes());
    out.extend_from_slice(&name);
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{file_name, wav};

    #[test]
    fn file_names_lose_reserved_characters() {
        assert_eq!(file_name(r#"a/b\c:d*e?"f<g>h|i. "#), "a_b_c_d_e__f_g_h_i");
        assert_eq!(file_name("Hope"), "Hope");
    }

    #[test]
    fn wav_chunks_add_up() {
        let bytes = wav(2, 48000, &[1, -1, 2, -2], "Hope");
        let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(u32_at(4) as usize, bytes.len() - 8);
        // "Hope\0" padded to 6
        assert_eq!(u32_at(40), 4 + 8 + 6);
        assert_eq!(u32_at(52), 5);
        assert_eq!(&bytes[62..66], b"data");
        assert_eq!(u32_at(66), 8);
        assert_eq!(bytes.len(), 70 + 8);
    }
}
