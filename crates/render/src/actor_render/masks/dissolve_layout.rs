use bevy::render::render_resource::{
    BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, SamplerBindingType, ShaderStages,
    TextureSampleType, TextureViewDimension,
};

pub(crate) const DISSOLVE_GROUP: usize = 3;

/// Sampled companion depth is present only in the later dissolve-colour pass.
///
/// `compatibility` mirrors the global `WEBGPU_COMPATIBILITY` shader def, which the pipeline
/// cache derives from the device profile mode; it must not come from an independent cap such as
/// `VIEW_FORMATS`, or the layout and the generated shader disagree on binding 1.
pub(crate) fn dissolve_layout(multisampled: bool, compatibility: bool) -> BindGroupLayoutDescriptor {
    let mut entries = vec![BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStages::FRAGMENT,
        ty: BindingType::Texture {
            sample_type: TextureSampleType::Depth,
            view_dimension: TextureViewDimension::D2,
            multisampled: multisampled && !compatibility,
        },
        count: None,
    }];
    if compatibility {
        entries.push(BindGroupLayoutEntry {
            binding: 1,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Sampler(SamplerBindingType::NonFiltering),
            count: None,
        });
    }
    BindGroupLayoutDescriptor::new("mask dissolve depth", &entries)
}
