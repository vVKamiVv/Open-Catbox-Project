use std::sync::Arc;

use anyhow::Result;
pub use shin_derive::TextureArchive;
use shin_render::gpu_texture::GpuTexture;

use crate::asset::system::{Asset, AssetDataAccessor, AssetLoadContext};

pub trait TextureArchiveBuilder {
    type Output;

    fn new() -> Self;
    fn add_texture(&mut self, name: &str, texture: GpuTexture);
    fn build(self) -> Self::Output;
}

pub trait TextureArchive: Sync + Send + 'static {
    type Builder: TextureArchiveBuilder<Output = Self>;
}

impl<T: TextureArchive> Asset for T {
    type Args = ();

    async fn load(
        context: &Arc<AssetLoadContext>,
        _args: (),
        name: &str,
        data: AssetDataAccessor,
    ) -> Result<Self> {
        let label = format!("TXA[{}]", name);
        let data = data.read_all().await;
        let context = context.clone();

        shin_tasks::compute::spawn(move || {
            let archive = shin_core::format::texture_archive::read_texture_archive(&data)?;

            let mut builder = T::Builder::new();

            for (tex_name, index) in archive.name_to_index {
                let texture = &archive.textures[index];

                let texture = GpuTexture::new_static_from_rgba_image(
                    &context.wgpu_device,
                    &context.wgpu_queue,
                    Some(&format!("{}/{}", label, tex_name)),
                    texture,
                );

                builder.add_texture(&tex_name, texture);
            }

            Ok(builder.build())
        })
        .await
    }
}

/// Every texture of an archive by name, for archives too large to list in a [`TextureArchive`] struct
pub struct NamedTextures {
    by_name: std::collections::HashMap<String, GpuTexture>,
    /// The names in archive order
    order: Vec<String>,
}

impl NamedTextures {
    /// The texture called `name`; panics when the archive has none, as a missing texture is a bug in the caller
    pub fn get(&self, name: &str) -> &GpuTexture {
        self.by_name
            .get(name)
            .unwrap_or_else(|| panic!("texture archive has no texture {name:?}"))
    }

    /// Whether the archive has a texture called `name` (`0x7100118b00`)
    pub fn contains(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    /// The first texture of the archive (`0x7100118ae0` with index 0)
    pub fn first(&self) -> Option<&GpuTexture> {
        self.order.first().map(|name| self.get(name))
    }
}

impl Asset for NamedTextures {
    type Args = ();

    async fn load(
        context: &Arc<AssetLoadContext>,
        _args: (),
        name: &str,
        data: AssetDataAccessor,
    ) -> Result<Self> {
        let label = format!("TXA[{}]", name);
        let data = data.read_all().await;
        let context = context.clone();

        shin_tasks::compute::spawn(move || {
            let archive = shin_core::format::texture_archive::read_texture_archive(&data)?;
            let mut order = archive.name_to_index.iter().collect::<Vec<_>>();
            order.sort_by_key(|&(_, &index)| index);
            Ok(Self {
                order: order.iter().map(|(name, _)| (*name).clone()).collect(),
                by_name: archive
                    .name_to_index
                    .iter()
                    .map(|(tex_name, &index)| {
                        let texture = GpuTexture::new_static_from_rgba_image(
                            &context.wgpu_device,
                            &context.wgpu_queue,
                            Some(&format!("{}/{}", label, tex_name)),
                            &archive.textures[index],
                        );
                        (tex_name.clone(), texture)
                    })
                    .collect(),
            })
        })
        .await
    }
}
