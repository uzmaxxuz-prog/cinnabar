use std::num::NonZeroU64;

use bevy::render::{
    render_resource::{
        BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, BufferBindingType,
        SamplerBindingType, ShaderStages, ShaderType, TextureSampleType, TextureViewDimension,
    },
    view::ViewUniform,
};
use render_model::{EntityShadow, EntityShadowParams};

pub(crate) const SHADOW_GROUP: usize = 0;
pub(crate) const INSTANCE_BYTES: u64 = size_of::<EntityShadow>() as u64;

/// Describes the view, scene depth and caster data for one shadow depth variant.
pub(crate) fn shadow_layout(multisampled: bool, compatibility: bool) -> BindGroupLayoutDescriptor {
    let fragment = ShaderStages::FRAGMENT;
    let entry = |binding, visibility, ty| BindGroupLayoutEntry {
        binding,
        visibility,
        ty,
        count: None,
    };
    let mut entries = vec![
        entry(
            0,
            ShaderStages::VERTEX_FRAGMENT,
            BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: Some(ViewUniform::min_size()),
            },
        ),
        entry(
            1,
            fragment,
            BindingType::Texture {
                sample_type: TextureSampleType::Depth,
                view_dimension: TextureViewDimension::D2,
                multisampled: multisampled && !compatibility,
            },
        ),
        entry(
            3,
            ShaderStages::VERTEX_FRAGMENT,
            BindingType::Buffer {
                ty: BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: NonZeroU64::new(INSTANCE_BYTES),
            },
        ),
        entry(
            4,
            ShaderStages::VERTEX_FRAGMENT,
            BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: NonZeroU64::new(size_of::<EntityShadowParams>() as u64),
            },
        ),
    ];
    if compatibility {
        entries.push(entry(
            2,
            fragment,
            BindingType::Sampler(SamplerBindingType::NonFiltering),
        ));
    }
    BindGroupLayoutDescriptor::new("entity shadow bind group layout", &entries)
}
