//! Reflect the actual linked shader interfaces and export them for browser pipelines.
#![allow(
    dead_code,
    reason = "share production shader constructors with export fixtures"
)]
#![expect(
    clippy::print_stderr,
    reason = "report an unavailable browser fixture directory"
)]
#[path = "webgpu_compatibility/actor_pixels.rs"]
mod actor_pixels;
#[path = "webgpu_compatibility/hand_flat.rs"]
mod hand_flat;
#[path = "../src/material_shader.rs"]
mod material_shader;
#[path = "../src/shader_safety.rs"]
mod shader_safety;
#[path = "it/support/shader_source.rs"]
mod shader_source;

struct Variant {
    name: String,
    source: String,
    definitions: Vec<&'static str>,
}

/// Enumerates the renderer's independent switches, including motion and mask MSAA paths.
fn variants() -> Vec<Variant> {
    let mut variants = Vec::new();
    let shader = shader_safety::from_actor_wesl(
        include_str!("../src/actor.wesl"),
        "actor.wesl",
        render_model::ACTOR_GPU_INSTANCE_WORDS,
        render_model::ACTOR_RIG_VERTEX_WORDS,
    );
    let bevy::shader::Source::Wesl(actor) = shader.source else {
        panic!("actor WESL")
    };
    for compact in [false, true] {
        for coverage in [false, true] {
            for gamma in [false, true] {
                for route in [
                    "colour",
                    "motion",
                    "mask",
                    "mask-blend",
                    "mask-dissolve",
                    "mask-dissolve-msaa",
                ] {
                    let mut definitions = Vec::new();
                    if compact {
                        definitions.push("COMPACT_BONES");
                    }
                    if coverage {
                        definitions.push("ALPHA_TO_COVERAGE");
                    }
                    if gamma {
                        definitions.push("ACTOR_GAMMA_BLEND");
                    }
                    if route == "motion" {
                        definitions.push("ACTOR_MOTION");
                    }
                    if route.starts_with("mask") {
                        definitions.push("ACTOR_MASK");
                    }
                    if route == "mask-blend" {
                        definitions.push("MASK_BLEND");
                    }
                    if route.starts_with("mask-dissolve") {
                        definitions.push("MASK_DISSOLVE");
                    }
                    if route == "mask-dissolve-msaa" {
                        definitions.push("MASK_DISSOLVE_MSAA");
                    }
                    variants.push(Variant {
                        name: format!("actor-{route}-{compact}-{coverage}-{gamma}"),
                        source: actor.to_string(),
                        definitions,
                    });
                }
            }
        }
    }
    for (name, source) in [
        ("model", include_str!("../src/model.wesl")),
        ("liquid", include_str!("../src/liquid.wesl")),
        (
            "transparent",
            include_str!("../src/transparent_terrain.wesl"),
        ),
        ("chunk", include_str!("../src/chunk.wesl")),
    ] {
        for enhanced in [false, true] {
            for coverage in [false, true] {
                for units in [false, true] {
                    let mut definitions = Vec::new();
                    if enhanced {
                        definitions.push("ENHANCED");
                    }
                    if coverage {
                        definitions.push("ALPHA_TO_COVERAGE");
                    }
                    if units {
                        definitions.push("TERRAIN_UNITS");
                    }
                    variants.push(Variant {
                        name: format!("{name}-{enhanced}-{coverage}-{units}"),
                        source: material_shader::source(source),
                        definitions,
                    });
                }
            }
        }
    }
    variants
}

/// Parses linked WGSL, never source-text approximations of its interface.
fn linked(variant: &Variant, compatibility: bool) -> (String, naga::Module) {
    let mut definitions = variant.definitions.clone();
    if compatibility {
        definitions.push("WEBGPU_COMPATIBILITY");
    }
    let source = shader_source::standalone_prepared(&variant.source, &definitions);
    let module = naga::front::wgsl::parse_str(&source)
        .unwrap_or_else(|error| panic!("{}: {}", variant.name, error.emit_to_string(&source)));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap_or_else(|error| panic!("{}: {error:?}", variant.name));
    (source, module)
}

/// Flattens the declared entry interface, retaining builtin and interpolation metadata.
fn bindings<'a>(
    module: &'a naga::Module,
    ty: naga::Handle<naga::Type>,
    binding: &'a Option<naga::Binding>,
) -> Vec<&'a naga::Binding> {
    if let Some(binding) = binding {
        return vec![binding];
    }
    let naga::TypeInner::Struct { members, .. } = &module.types[ty].inner else {
        return Vec::new();
    };
    members
        .iter()
        .flat_map(|member| bindings(module, member.ty, &member.binding))
        .collect()
}

/// Describes actual vertex attributes so pipeline validation also checks the real stage link.
fn vertex_buffer_layouts(
    module: &naga::Module,
    entry: &naga::EntryPoint,
) -> Vec<serde_json::Value> {
    entry.function.arguments.iter().filter_map(|argument| {
        let Some(naga::Binding::Location { location, .. }) = argument.binding else { return None; };
        let (components, scalar) = match module.types[argument.ty].inner {
            naga::TypeInner::Scalar(scalar) => (1, scalar),
            naga::TypeInner::Vector { size, scalar } => (size as u32, scalar),
            _ => panic!("unsupported vertex attribute type in {}", entry.name),
        };
        assert_eq!(scalar.width, 4, "vertex fixture requires a 32-bit attribute");
        let prefix = match scalar.kind {
            naga::ScalarKind::Float => "float32",
            naga::ScalarKind::Uint => "uint32",
            naga::ScalarKind::Sint => "sint32",
            _ => panic!("unsupported vertex attribute scalar in {}", entry.name),
        };
        let format = if components == 1 { prefix.to_owned() } else { format!("{prefix}x{components}") };
        Some(serde_json::json!({"arrayStride":components * 4,"stepMode":"vertex","attributes":[{"shaderLocation":location,"offset":0,"format":format}]}))
    }).collect()
}

#[test]
fn linked_compatibility_interfaces_have_dense_locations_and_fit_the_device() {
    for variant in variants() {
        let (_, module) = linked(&variant, true);
        for entry in &module.entry_points {
            if entry.stage != naga::ShaderStage::Fragment {
                continue;
            }
            let interface: Vec<_> = entry
                .function
                .arguments
                .iter()
                .flat_map(|argument| bindings(&module, argument.ty, &argument.binding))
                .collect();
            let mut locations = Vec::new();
            let mut builtins = 0;
            for binding in interface {
                match binding {
                    naga::Binding::Location {
                        location, sampling, ..
                    } => {
                        assert_ne!(
                            *sampling,
                            Some(naga::Sampling::First),
                            "{} {} location {location}: compatibility cannot use provoking-first interpolation",
                            variant.name,
                            entry.name
                        );
                        locations.push(*location);
                    }
                    naga::Binding::BuiltIn(naga::BuiltIn::FrontFacing) => builtins += 1,
                    naga::Binding::BuiltIn(naga::BuiltIn::SampleIndex) => panic!(
                        "{} {} uses forbidden sample_index",
                        variant.name, entry.name
                    ),
                    _ => {}
                }
            }
            locations.sort_unstable();
            assert!(
                locations.len() + builtins <= 12,
                "{} {} consumes {} locations + {builtins} front_facing",
                variant.name,
                entry.name,
                locations.len()
            );
            assert_eq!(
                locations,
                (0..locations.len() as u32).collect::<Vec<_>>(),
                "{} {} leaves location holes",
                variant.name,
                entry.name
            );
        }
    }
}

#[test]
fn export_linked_browser_fixtures() {
    let Some(output) = std::env::var_os("CINNABAR_SHADER_FIXTURE_OUT") else {
        eprintln!("missing-fixture: CINNABAR_SHADER_FIXTURE_OUT; browser shader export not run");
        return;
    };
    let output = std::path::PathBuf::from(output);
    std::fs::create_dir_all(&output).expect("shader output directory");
    let mut manifest = hand_flat::export(&output);
    manifest.extend(actor_pixels::export(&output));
    for variant in variants() {
        for (mode, compatibility) in [("core", false), ("compatibility", true)] {
            let (source, module) = linked(&variant, compatibility);
            let file = format!("{}-{mode}.wgsl", variant.name);
            std::fs::write(output.join(&file), source).expect("write linked shader");
            let vertices: Vec<_> = module
                .entry_points
                .iter()
                .filter(|entry| entry.stage == naga::ShaderStage::Vertex)
                .collect();
            let fragments: Vec<_> = module
                .entry_points
                .iter()
                .filter(|entry| entry.stage == naga::ShaderStage::Fragment)
                .collect();
            for vertex in &vertices {
                for fragment in &fragments {
                    manifest.push(serde_json::json!({"name": format!("{}-{}-{}", variant.name, vertex.name, fragment.name), "mode":mode, "file":file, "vertex":vertex.name, "fragment":fragment.name,"buffers":vertex_buffer_layouts(&module,vertex)}));
                }
            }
        }
    }
    let required_variants: std::collections::BTreeSet<_> = manifest
        .iter()
        .filter_map(|item| item["name"].as_str())
        .collect();
    let required_semantic: std::collections::BTreeSet<_> = manifest
        .iter()
        .filter_map(|item| item["semantic"]["id"].as_str())
        .collect();
    let coverage = serde_json::json!({"requiredVariants":required_variants,"requiredSemanticCases":required_semantic});
    std::fs::write(
        output.join("shaders.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"variants":manifest,"coverage":coverage}))
            .expect("shader manifest JSON"),
    )
    .expect("shader manifest");
}
