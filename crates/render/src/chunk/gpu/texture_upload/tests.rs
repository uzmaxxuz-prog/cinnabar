use super::*;

/// Unknown devices conservatively allocate and upload both complete terrain mip chains.
#[test]
fn paired_atlas_uploads_every_layer_and_mip() {
    let (raw, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::new(raw);
    let queue = RenderQueue::new(queue);
    let (prepared, stats) =
        build_chunk_texture_assets(&ChunkTextureAssets::default(), &device, &queue).unwrap();
    let leaves = prepared
        ._native_leaf_textures
        .as_ref()
        .expect("no VIEW_FORMATS needs physical leaf pages");
    let mut bytes = 0;
    for (terrain, leaf) in prepared._textures.iter().zip(leaves) {
        assert_eq!(terrain.format(), TextureFormat::Rgba8UnormSrgb);
        assert_eq!(leaf.format(), TextureFormat::Rgba8Unorm);
        assert_eq!(terrain.size(), leaf.size());
        assert_eq!(terrain.mip_level_count(), leaf.mip_level_count());
        assert_ne!(terrain.id(), leaf.id());
        for mip in 0..terrain.mip_level_count() {
            let size = terrain.size().mip_level_size(mip, terrain.dimension());
            bytes += 2
                * u64::from(size.width)
                * u64::from(size.height)
                * u64::from(terrain.depth_or_array_layers())
                * 4;
        }
    }
    assert_eq!(stats.texture_bytes_including_mips, bytes);
    assert_eq!(stats.queue_upload_bytes, bytes);
}

/// Production uploads preserve every texel's RGB and alpha in every physical layer and mip.
#[test]
fn paired_pages_sample_identical_bytes_across_all_layers_and_mips() {
    use crate::gpu_snapshot::{Draw, Gpu, SNAPSHOT_SIDE};
    let Some(gpu) = Gpu::for_fixture("paired terrain mip readback") else {
        return;
    };
    let device = RenderDevice::new(gpu.device.clone());
    let queue = RenderQueue::new(gpu.queue.clone());
    let layers = 3;
    let mips = [4, 2, 1]
        .into_iter()
        .enumerate()
        .map(|(mip, size)| {
            let rgba8 = (0..layers)
                .flat_map(|layer| {
                    let rgba = [
                        32 + layer as u8 * 47,
                        10 + mip as u8 * 45,
                        149,
                        83 + layer as u8 * 23,
                    ];
                    std::iter::repeat_n(rgba, (size * size) as usize).flatten()
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            assets::texture::TextureMip { size, rgba8 }
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let array = TextureArray { layers, mips };
    let plans = plan_queue_texture_mips(&array).unwrap();
    let (_, srgb, srgb_bytes) =
        upload_texture_page(&device, &queue, &array, &plans, "terrain mip witness");
    let (_, unorm, unorm_bytes) = upload_texture_page_format(
        &device,
        &queue,
        &array,
        &plans,
        "leaf mip witness",
        TextureFormat::Rgba8Unorm,
        &[],
    );
    assert_eq!(srgb_bytes, unorm_bytes);
    let mip_count = array.mips.len();
    let source = format!(
        r#"
@group(0) @binding(0) var image: texture_2d_array<f32>;
@vertex fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{
    let xy = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(xy * 2.0 - vec2<f32>(1.0), 0.5, 1.0);
}}
@fragment fn fragment(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {{
    let mip = i32(u32(p.x) % {mip_count}u);
    let layer = i32((u32(p.x) / {mip_count}u) % {layers}u);
    let xy = vec2<i32>(vec2<u32>(p.xy) % textureDimensions(image, mip));
    return textureLoad(image, xy, layer, mip);
}}
"#
    );
    for (view, encoded) in [(&srgb, true), (&unorm, false)] {
        let bindings = [wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(view),
        }];
        let draws = [Draw {
            fragment: "fragment",
            vertices: 0..3,
            bindings: &bindings,
            blend: None,
            write_depth: false,
        }];
        let pixels = if encoded {
            gpu.render_srgb(&source, "vertex", &draws)
        } else {
            gpu.render(&source, "vertex", &draws)
        };
        for (index, pixel) in pixels.chunks_exact(4).enumerate() {
            let x = index % SNAPSHOT_SIDE as usize;
            let mip = x % mip_count;
            let layer = (x / mip_count) % layers as usize;
            let expected = [
                32 + layer as u8 * 47,
                10 + mip as u8 * 45,
                149,
                83 + layer as u8 * 23,
            ];
            assert!(
                pixel.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1),
                "layer {layer}, mip {mip}, sRGB {encoded}: {pixel:?} != {expected:?}"
            );
        }
    }
}

/// Both atlas views must share one uploaded allocation per physical page.
#[test]
fn atlas_views_share_each_uploaded_texture_page() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions::enabled(),
            ..Default::default()
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = bevy::tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let device =
        crate::render_test_support::profiled_device(&instance, &adapter, device, queue.clone());
    let queue = RenderQueue::new(queue);
    let (prepared, stats) =
        build_chunk_texture_assets(&ChunkTextureAssets::default(), &device, &queue).unwrap();

    let report = instance.generate_report().unwrap();
    assert_eq!(report.hub.textures.num_allocated, assets::MAX_TEXTURE_PAGES);
    assert_eq!(
        report.hub.texture_views.num_allocated,
        crate::material_shader::CHUNK_SAMPLED_TEXTURE_BINDINGS as usize
    );
    assert_eq!(
        report.hub.samplers.num_allocated,
        crate::material_shader::CHUNK_SAMPLER_COUNT as usize
    );

    let mut uploaded_bytes = 0;
    for ((texture, view), native_view) in prepared
        ._textures
        .iter()
        .zip(&prepared.views)
        .zip(&prepared.native_leaf_views)
    {
        assert_eq!(texture.format(), TextureFormat::Rgba8UnormSrgb);
        assert_eq!(view.texture(), &**texture);
        assert_eq!(native_view.texture(), &**texture);
        assert_ne!(view.id(), native_view.id());
        // Creating the alternate view exercises format compatibility validation.
        let _linear_view = texture.create_view(&TextureViewDescriptor {
            format: Some(TextureFormat::Rgba8Unorm),
            dimension: Some(TextureViewDimension::D2Array),
            ..Default::default()
        });
        for mip in 0..texture.mip_level_count() {
            let size = texture.size().mip_level_size(mip, texture.dimension());
            uploaded_bytes += u64::from(size.width)
                * u64::from(size.height)
                * u64::from(size.depth_or_array_layers)
                * u64::from(texture.format().block_copy_size(None).unwrap());
        }
    }
    assert_eq!(stats.queue_upload_bytes, uploaded_bytes);
}
