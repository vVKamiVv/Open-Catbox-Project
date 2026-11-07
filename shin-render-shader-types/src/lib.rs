//! Types shared between the Rust side and the shaders: uniform blocks and vertex formats (from which
//! `shin-render-shaders` generates `types.wgsl`), typed GPU buffers, textures and samplers, and [`RenderClone`],
//! a clone that also copies GPU buffers and textures.

pub mod buffer;
mod clone;
pub mod texture;
pub mod uniforms;
pub mod vertices;

pub use clone::{RenderClone, RenderCloneCtx};
