//! Evaluate the production label fragment with faded text and controlled atlas coverage.

use crate::{
    gpu_snapshot::{Draw, Gpu},
    nametag_shader, shader_source,
};
use render_model::NametagRecord;
use wgpu::util::DeviceExt;

/// Supplies a fullscreen triangle while retaining the production fragment entry point.
fn fragment_fixture(record: NametagRecord, definitions: &[&str]) -> String {
    let shader = nametag_shader::from_wesl(
        include_str!("../../src/nametag.wesl"),
        "label coverage test",
    );
    let mut source = shader_source::standalone_prepared(shader.source.as_str(), definitions);
    let [r, g, b, a] = record.color;
    source.push_str(&format!(
        r#"
@vertex fn coverage_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {{
    let point = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0))[index];
    var out: VertexOutput;
    out.position = vec4(point, 0.5, 1.0);
    out.uv = vec2(0.5);
    out.color = vec4<f32>({r}, {g}, {b}, {a});
    out.textured = {textured}u;
    return out;
}}
"#,
        textured = u32::from(record.uv[2] >= 0.0)
    ));
    source
}

#[test]
fn production_fragments_discard_coverage_before_cosmetic_alpha() {
    let Some(gpu) = Gpu::for_fixture("nametag coverage and cosmetic opacity") else {
        return;
    };
    let sampler = gpu
        .device
        .create_sampler(&wgpu::SamplerDescriptor::default());
    // Ordinary dim text and sneaking text share the same native alpha.
    for native_alpha in [render_model::NAMETAG_DIM_TEXT_ALPHA, 1.0, 0.25] {
        for cosmetic_alpha in [0.0, 0.5, 0.125] {
            for coverage in [0_u8, 25, 26, 128, 255] {
                let record = NametagRecord {
                    color: [1.0, 1.0, 1.0, native_alpha * cosmetic_alpha],
                    uv: if native_alpha == 0.25 {
                        [0.0, 0.0, -1.0, -1.0]
                    } else {
                        [0.0, 0.0, 1.0, 1.0]
                    },
                    ..Default::default()
                };
                let texture = gpu.device.create_texture_with_data(
                    &gpu.queue,
                    &wgpu::TextureDescriptor {
                        label: Some("owned nametag coverage fixture"),
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
                    },
                    wgpu::util::TextureDataOrder::LayerMajor,
                    &[255, 255, 255, coverage],
                );
                let view = texture.create_view(&Default::default());
                let bindings = [
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ];
                for definitions in [&[][..], &["NAMETAG_GAMMA_BLEND"][..]] {
                    let source = fragment_fixture(record, definitions);
                    let pixels = gpu.render(
                        &source,
                        "coverage_vertex",
                        &[Draw {
                            fragment: "nametag_fragment",
                            vertices: 0..3,
                            bindings: &bindings,
                            blend: None,
                            write_depth: false,
                        }],
                    );
                    let clear = crate::gpu_snapshot::CLEAR_COLOR;
                    let expected = record
                        .fragment_color([1.0, 1.0, 1.0, f32::from(coverage) / 255.0])
                        .unwrap_or([
                            clear.r as f32,
                            clear.g as f32,
                            clear.b as f32,
                            clear.a as f32,
                        ])
                        .map(|channel| (channel * 255.0).round() as u8);
                    let at = ((128 * crate::gpu_snapshot::SNAPSHOT_SIDE + 128) * 4) as usize;
                    for (&actual, expected) in pixels[at..at + 4].iter().zip(expected) {
                        assert!(
                            actual.abs_diff(expected) <= 1,
                            "native {native_alpha}, cosmetic {cosmetic_alpha}, coverage {coverage}, defs {definitions:?}"
                        );
                    }
                }
            }
        }
    }
}
