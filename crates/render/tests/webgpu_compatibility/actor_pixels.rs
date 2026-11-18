//! Real actor material draws with original geometry, storage layouts and textures.
use super::{shader_safety, shader_source};
use bevy::math::Mat4;
use render_model::{ActorGpuInstance, ActorRigGeometrySpan, ActorRigVertex};

/// Emits independent literal colour expectations, not values recomputed by the shader.
pub(super) fn export(output: &std::path::Path) -> Vec<serde_json::Value> {
    let rows = [
        [1.0_f32, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    let vertices =
        [[-1.0, -1.0, 0.5], [3.0, -1.0, 0.5], [-1.0, 3.0, 0.5]].map(|position| ActorRigVertex {
            position,
            normal: [0.0, 0.0, 1.0],
            uv: [0.5; 2],
            back_uv: [0.5; 2],
            ..Default::default()
        });
    let mut view = [0.0_f32; 104];
    for matrix in view[..96].chunks_exact_mut(16) {
        matrix.copy_from_slice(&Mat4::IDENTITY.to_cols_array());
    }
    view[100..].copy_from_slice(&[0.0, 0.0, 4.0, 4.0]);
    let mut atmosphere = [0.0_f32; 32];
    atmosphere[19] = 1_000_000.0;
    atmosphere[20] = 1_001_000.0;
    let lightmap = [1.0_f32; 2 * 256 * 4];
    let shader = shader_safety::from_actor_wesl(
        include_str!("../../src/actor.wesl"),
        "actor.wesl",
        render_model::ACTOR_GPU_INSTANCE_WORDS,
        render_model::ACTOR_RIG_VERTEX_WORDS,
    );
    let bevy::shader::Source::Wesl(source) = shader.source else {
        panic!("actor WESL")
    };
    let mut result = Vec::new();
    for (mode, definitions) in [
        ("core", &[][..]),
        ("compatibility", &["WEBGPU_COMPATIBILITY"][..]),
    ] {
        let file = format!("actor-pixels-{mode}.wgsl");
        std::fs::write(
            output.join(&file),
            shader_source::standalone_prepared(&source, definitions),
        )
        .expect("actor material fixture WGSL");
        for (id, overlay_rgba8, expected) in [
            ("actor-red-tint", 0u32, [255, 0, 0, 255]),
            (
                "actor-overlay-byte-precision",
                0x8000_ff00,
                [127, 128, 0, 255],
            ),
        ] {
            let instance = ActorGpuInstance {
                world_from_actor: rows,
                tint: 0xff00_00ff,
                overlay_rgba8,
                uv_anim: [0.0, 0.0, 1.0, 1.0],
                material: assets::EntityRenderMaterial::Default as u32,
                ..Default::default()
            };
            let mut bindings: Vec<_> = [
                (0, 0, "uniform", bytemuck::cast_slice::<f32, u8>(&view).to_vec()),
                (0, 1, "storage", bytemuck::bytes_of(&instance).to_vec()),
                (0, 2, "storage", bytemuck::cast_slice::<ActorRigVertex, u8>(&vertices).to_vec()),
                (0, 3, "storage", bytemuck::bytes_of(&ActorRigGeometrySpan { first_vertex: 0, vertex_count: 3 }).to_vec()),
                (0, 4, "storage", bytemuck::cast_slice::<[f32; 4], u8>(&rows).to_vec()),
                (0, 5, "storage", bytemuck::cast_slice::<[f32; 4], u8>(&rows).to_vec()),
                (0, 8, "uniform", vec![0; 16]),
                (1, 0, "uniform", bytemuck::cast_slice::<f32, u8>(&lightmap).to_vec()),
                (1, 1, "uniform", bytemuck::cast_slice::<f32, u8>(&atmosphere).to_vec()),
            ].map(|(group,binding,usage,bytes)| serde_json::json!({"group":group,"binding":binding,"usage":usage,"bytes":bytes})).into_iter().collect();
            for binding in [6, 9, 10, 11, 12, 14] {
                bindings.push(serde_json::json!({"group":0,"binding":binding,"texture":{"size":[1,1,1],"format":"rgba8unorm-srgb","viewDimension":if binding == 12 || binding == 14 {"2d"} else {"2d-array"},"rgba":[255,255,255,255]}}));
            }
            for binding in [7, 13] {
                bindings.push(serde_json::json!({"group":0,"binding":binding,"sampler":{"magFilter":"nearest","minFilter":"nearest","mipmapFilter":"nearest"}}));
            }
            result.push(serde_json::json!({"name":id,"mode":mode,"file":file,"vertex":"actor_vertex","fragment":"actor_fragment","targets":[{"format":"rgba8unorm-srgb"}],"semantic":{"id":id,"vertexCount":3,"width":4,"height":4,"bindings":bindings,"expectedPixels":[{"x":1,"y":1,"rgba":expected,"tolerance":1}]}}));
        }
    }
    result
}
