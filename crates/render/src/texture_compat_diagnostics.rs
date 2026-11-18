//! Browser-only validation of production texture allocation paths.
use bevy::render::renderer::{RenderDevice, RenderQueue};
use serde_json::{Value, json};

/// Allocates, binds and samples the production dropped-item and UI single-layer arrays.
/// Independent validation scopes preserve the original resource-specific errors.
pub async fn production_arrays(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    compatibility: bool,
) -> Value {
    let render_device = RenderDevice::new(device.clone());
    let render_queue = RenderQueue::new(queue.clone());
    let mut tests = Vec::new();
    let mut pixels = Vec::new();
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("production array sampling fixture"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        }],
    });
    for id in [
        "dropped-item-one-layer",
        "ui-one-layer",
        "none-array-negative-control",
    ] {
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let (texture, view): (wgpu::Texture, wgpu::TextureView) = match id {
            "dropped-item-one-layer" => {
                let (texture, view) = crate::dropped_item_render::model_upload::diagnostic_texture(
                    &render_device,
                    &render_queue,
                );
                ((*texture).clone(), (*view).clone())
            }
            "ui-one-layer" => {
                let (texture, view) =
                    crate::ui_render::textures::diagnostic_texture(&render_device);
                ((*texture).clone(), (*view).clone())
            }
            _ => {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("none-array-negative-control"),
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                    texture_binding_view_dimension: None,
                });
                let view = texture.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2Array),
                    ..Default::default()
                });
                (texture, view)
            }
        };
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(id),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        let error = scope.pop().await.map(|error| error.to_string());
        let expected_error = id == "none-array-negative-control" && compatibility;
        let ok = if expected_error {
            error.as_ref().is_some_and(|message| {
                let message = message.to_ascii_lowercase();
                message.contains("dimension") && message.contains("array")
            })
        } else {
            error.is_none()
        };
        tests.push(json!({"id":id,"ok":ok,"error":error,"expectedValidationError":expected_error}));
        if error.is_some() || id == "none-array-negative-control" {
            continue;
        }
        if id == "ui-one-layer" {
            queue.write_texture(
                texture.as_image_copy(),
                &[128, 64, 32, 255],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                texture.size(),
            );
        }
        let actual = sample_pixel(device, queue, &layout, &binding).await;
        let expected = if id == "dropped-item-one-layer" {
            [55, 13, 4, 255]
        } else {
            [128, 64, 32, 255]
        };
        let ok = actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual.abs_diff(expected) <= 1);
        tests.push(json!({"id":format!("{id}-sample"),"ok":ok}));
        pixels.push(json!({"id":id,"actual":actual,"expected":expected}));
    }
    json!({"tests":tests,"pixelComparisons":pixels})
}

/// Renders the bound array into an independent UNORM target and maps its actual GPU bytes.
async fn sample_pixel(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    binding: &wgpu::BindGroup,
) -> [u8; 4] {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production array pixel witness"),
        source: wgpu::ShaderSource::Wgsl(
            r#"
@group(0) @binding(0) var image: texture_2d_array<f32>;
@vertex fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let xy = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(xy * 2.0 - vec2<f32>(1.0), 0.0, 1.0);
}
@fragment fn fragment() -> @location(0) vec4<f32> {
    return textureLoad(image, vec2<i32>(0), 0, 0);
}
"#
            .into(),
        ),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("production array pixel witness"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("production array pixel witness"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fragment"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("array sample readback target"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
        texture_binding_view_dimension: None,
    });
    let view = target.create_view(&Default::default());
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("array sample pixel bytes"),
        size: 256,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sample production array"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, binding, &[]);
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(1),
            },
        },
        target.size(),
    );
    queue.submit([encoder.finish()]);
    let (send, receive) = futures::channel::oneshot::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = send.send(result);
        });
    receive
        .await
        .expect("mapping callback runs")
        .expect("pixel readback maps");
    let bytes = buffer
        .slice(..)
        .get_mapped_range()
        .expect("pixel bytes are mapped");
    bytes[..4].try_into().expect("RGBA pixel")
}
