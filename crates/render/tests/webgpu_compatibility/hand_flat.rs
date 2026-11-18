//! Raster witness for the production hand vertex producer's provoking-first shade.
use super::{shader_safety, shader_source};
use bevy::math::Mat4;
use render_model::{ActorGpuInstance, ActorRigGeometrySpan, ActorRigVertex, HandRigLight};

/// Keeps every resource byte identical between core and compatibility draws.
pub(super) fn export(output: &std::path::Path) -> Vec<serde_json::Value> {
    let rows = [
        [1.0_f32, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    let vertices = [
        ([-1.0, -1.0, 0.5], [0.0, -1.0, 0.0], 0),
        ([3.0, -1.0, 0.5], [1.0, 0.0, 0.0], 1),
        ([-1.0, 3.0, 0.5], [0.0, 1.0, 0.0], 1),
    ]
    .map(|(position, normal, bone_index)| ActorRigVertex {
        position,
        normal,
        bone_index,
        uv: [0.5; 2],
        back_uv: [0.5; 2],
        ..Default::default()
    });
    let instance = ActorGpuInstance {
        world_from_actor: rows,
        ..Default::default()
    };
    let mut light = HandRigLight::default();
    light.java_lights = [[0.0, 1.0, 0.0, 1.0], [0.0, 0.0, 0.0, 0.0]];
    light.java_normal_axes = [[0.0, 0.0, 1.0, 0.0]; 3];
    let bones = [rows, rows];
    let bindings = [
        (0, "uniform", bytemuck::cast_slice::<f32, u8>(&Mat4::IDENTITY.to_cols_array()).to_vec()),
        (1, "storage", bytemuck::bytes_of(&instance).to_vec()),
        (2, "storage", bytemuck::cast_slice::<ActorRigVertex, u8>(&vertices).to_vec()),
        (3, "storage", bytemuck::bytes_of(&ActorRigGeometrySpan { first_vertex: 0, vertex_count: 3 }).to_vec()),
        (4, "storage", bytemuck::cast_slice::<[[f32; 4]; 3], u8>(&bones).to_vec()),
        (5, "storage", bytemuck::cast_slice::<[[f32; 4]; 3], u8>(&bones).to_vec()),
        (8, "uniform", vec![0; 32]),
        (9, "uniform", bytemuck::bytes_of(&light).to_vec()),
    ].map(|(binding, usage, bytes)| serde_json::json!({"group":0,"binding":binding,"usage":usage,"bytes":bytes}));
    let shader = shader_safety::from_actor_wesl(
        include_str!("../../src/hand_rig.wesl"),
        "hand_rig.wesl",
        render_model::ACTOR_GPU_INSTANCE_WORDS,
        render_model::ACTOR_RIG_VERTEX_WORDS,
    );
    let bevy::shader::Source::Wesl(source) = shader.source else {
        panic!("hand WESL")
    };
    let mut result = Vec::new();
    for (mode, definitions, interpolation, shade) in [
        ("core", &[][..], "flat", "actual.shade"),
        (
            "compatibility",
            &["WEBGPU_COMPATIBILITY"][..],
            "flat, either",
            "hand_vertex(2u, 0u).shade",
        ),
    ] {
        let source = source
            .replace("@vertex\nfn hand_vertex(", "fn hand_vertex(")
            .replace(
                "@builtin(vertex_index) vertex_index: u32",
                "vertex_index: u32",
            )
            .replace(
                "@builtin(instance_index) instance_index: u32",
                "instance_index: u32",
            );
        let linked = shader_source::standalone_prepared(&source, definitions);
        let source = format!(
            "{linked}\nstruct ShadeWitness {{ @builtin(position) position: vec4<f32>, @location(0) @interpolate({interpolation}) shade: f32, }}\n@vertex fn shade_vertex(@builtin(vertex_index) index: u32) -> ShadeWitness {{ let actual = hand_vertex(index, 0u); return ShadeWitness(actual.position, {shade}); }}\n@fragment fn shade_fragment(input: ShadeWitness) -> @location(0) vec4<f32> {{ return vec4(vec3(input.shade), 1.0); }}"
        );
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("production hand raster witness validates");
        let file = format!("hand-flat-{mode}.wgsl");
        std::fs::write(output.join(&file), source).expect("hand raster fixture WGSL");
        let normal_case = serde_json::json!({"name":"hand-flat-provoking-first","mode":mode,"file":file,"vertex":"shade_vertex","fragment":"shade_fragment","semantic":{"id":"hand-flat-provoking-first","vertexCount":3,"width":4,"height":4,"bindings":bindings,"expectedPixels":[{"x":1,"y":1,"rgba":[102,102,102,255],"tolerance":1}]}});
        let mut bone_case = normal_case.clone();
        bone_case["name"] = "hand-flat-bone-provoking-first".into();
        bone_case["semantic"]["id"] = "hand-flat-bone-provoking-first".into();
        let bone_vertices = vertices.map(|mut vertex| {
            vertex.normal = [0.0, -1.0, 0.0];
            if vertex.bone_index == 1 {
                vertex.position[0] = -vertex.position[0];
                vertex.position[1] = -vertex.position[1];
            }
            vertex
        });
        let different_bones = [
            rows,
            [
                [-1.0_f32, 0.0, 0.0, 0.0],
                [0.0, -1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
        ];
        for binding in bone_case["semantic"]["bindings"]
            .as_array_mut()
            .expect("fixture bindings")
        {
            match binding["binding"].as_u64().expect("binding number") {
                2 => {
                    binding["bytes"] = serde_json::to_value(bytemuck::cast_slice::<
                        ActorRigVertex,
                        u8,
                    >(&bone_vertices))
                    .expect("bone fixture vertices")
                }
                4 | 5 => {
                    binding["bytes"] = serde_json::to_value(
                        bytemuck::cast_slice::<[[f32; 4]; 3], u8>(&different_bones),
                    )
                    .expect("bone fixture matrices")
                }
                _ => {}
            }
        }
        result.extend([normal_case, bone_case]);
    }
    result
}
