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
    packed: bool,
}

/// Enumerates shader variants, including motion and mask MSAA paths.
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
                        packed: true,
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
                    // queue_chunks and ChunkPipeline::prewarm only use units outside Enhanced.
                    if enhanced && units {
                        continue;
                    }
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
                        packed: true,
                    });
                }
            }
        }
    }
    let mut additional = Vec::new();
    for variant in &variants {
        // Enhanced shadow pipelines use `vertex` and reserve group 2 for the caster,
        // not the Enhanced frame or terrain units.
        if (variant.name.starts_with("model-") || variant.name.starts_with("chunk-"))
            && !variant.definitions.contains(&"ENHANCED")
            && !variant.definitions.contains(&"TERRAIN_UNITS")
        {
            let coverage = variant.definitions.contains(&"ALPHA_TO_COVERAGE");
            for packed_terrain in [false, true] {
                // Both storage layouts ship without alpha-to-coverage; keep the
                // existing unpacked alpha-to-coverage case for shader safety.
                if packed_terrain && coverage {
                    continue;
                }
                let mut definitions = variant.definitions.clone();
                definitions.push("ENHANCED_SHADOW");
                let suffix = if packed_terrain {
                    definitions.push("PACKED_TERRAIN");
                    "packed-shadow"
                } else {
                    "shadow"
                };
                additional.push(Variant {
                    name: format!("{}-{suffix}", variant.name),
                    source: variant.source.clone(),
                    definitions,
                    packed: true,
                });
            }
        }
        if variant.name.starts_with("transparent-") {
            let mut definitions = variant.definitions.clone();
            definitions.push("MOD_BLOCKS");
            additional.push(Variant {
                name: format!("{}-blocks", variant.name),
                source: variant.source.clone(),
                definitions,
                packed: true,
            });
        }
    }
    variants.extend(additional);
    for compact in [false, true] {
        let shader = shader_safety::from_actor_wesl(
            include_str!("../src/hand_rig.wesl"),
            "hand_rig.wesl",
            render_model::ACTOR_GPU_INSTANCE_WORDS,
            render_model::ACTOR_RIG_VERTEX_WORDS,
        );
        let bevy::shader::Source::Wesl(source) = shader.source else {
            panic!("hand WESL")
        };
        variants.push(Variant {
            name: format!("hand-{compact}"),
            source: source.to_string(),
            definitions: if compact {
                vec!["COMPACT_BONES"]
            } else {
                Vec::new()
            },
            packed: false,
        });
    }
    for multisampled in [false, true] {
        variants.push(Variant {
            name: format!("entity-shadow-{multisampled}"),
            source: material_shader::source(include_str!("../src/entity_shadow.wesl")),
            definitions: if multisampled {
                vec!["MULTISAMPLED"]
            } else {
                Vec::new()
            },
            packed: false,
        });
    }
    variants
}

/// Parses linked WGSL, never source-text approximations of its interface.
fn linked(variant: &Variant, compatibility: bool) -> (String, naga::Module) {
    let mut definitions = variant.definitions.clone();
    if compatibility {
        definitions.push("WEBGPU_COMPATIBILITY");
    }
    let source = shader_source::link_prepared(&variant.source, &definitions)
        .unwrap_or_else(|error| panic!("{}: {error}", variant.name));
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

/// Collects reconstructed struct members from the linked unpacker's real Naga stores.
fn reconstructed_members(
    function: &naga::Function,
    block: &naga::Block,
    output: naga::Handle<naga::Type>,
    written: &mut std::collections::BTreeSet<u32>,
) {
    for statement in block.iter() {
        match statement {
            naga::Statement::Store { pointer, .. } => {
                if let naga::Expression::AccessIndex { base, index } =
                    function.expressions[*pointer]
                    && let naga::Expression::LocalVariable(local) = function.expressions[base]
                    && function.local_variables[local].ty == output
                {
                    written.insert(index);
                }
            }
            naga::Statement::Block(inner) => {
                reconstructed_members(function, inner, output, written)
            }
            naga::Statement::If { accept, reject, .. } => {
                reconstructed_members(function, accept, output, written);
                reconstructed_members(function, reject, output, written);
            }
            _ => {}
        }
    }
}

/// Checks both directions against resolved types, reusing the interface test's composition.
fn assert_stage_member_coverage(variant: &Variant, module: &naga::Module) {
    if !variant.packed {
        return;
    }
    let mut packers = 0;
    let mut unpackers = 0;
    for (_, function) in module.functions.iter() {
        let name = function.name.as_deref().unwrap_or_default();
        let unpacking = name.contains("unpack_stage");
        if !unpacking && !name.contains("pack_stage") {
            continue;
        }
        let output = function
            .result
            .as_ref()
            .expect("stage adapter returns a struct")
            .ty;
        let naga::TypeInner::Struct { members, .. } = &module.types[output].inner else {
            panic!("stage adapter output must remain a struct");
        };
        let mut written = std::collections::BTreeSet::new();
        reconstructed_members(function, &function.body, output, &mut written);
        assert_eq!(
            written,
            (0..members.len() as u32).collect(),
            "{} {name} silently left an output member at zero",
            variant.name
        );
        if unpacking {
            unpackers += 1;
            continue;
        }
        packers += 1;
        let naga::TypeInner::Struct { members, .. } = &module.types[function.arguments[0].ty].inner
        else {
            panic!("stage packer input must remain the internal struct");
        };
        let read: std::collections::BTreeSet<_> = function
            .expressions
            .iter()
            .filter_map(|(_, expression)| {
                if let naga::Expression::AccessIndex { base, index } = expression
                    && matches!(
                        function.expressions[*base],
                        naga::Expression::FunctionArgument(0)
                    )
                {
                    Some(*index)
                } else {
                    None
                }
            })
            .collect();
        let raw_overlay = members
            .iter()
            .position(|member| member.name.as_deref() == Some("overlay_bits"));
        for (index, member) in members.iter().enumerate() {
            // Overlay is a derived UNORM view of the transported original instance word.
            let derived_overlay = member.name.as_deref() == Some("overlay")
                && raw_overlay.is_some_and(|raw| read.contains(&(raw as u32)));
            assert!(
                read.contains(&(index as u32)) || derived_overlay,
                "{} {name} dropped internal member {:?}",
                variant.name,
                member.name
            );
        }
    }
    assert!(
        packers > 0 && unpackers > 0,
        "{} lost its real stage adapters",
        variant.name
    );
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
        assert_stage_member_coverage(&variant, &module);
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
                        location,
                        interpolation,
                        sampling,
                        ..
                    } => {
                        if *interpolation == Some(naga::Interpolation::Flat) {
                            assert_eq!(
                                *sampling,
                                Some(naga::Sampling::Either),
                                "{} {} location {location}: compatibility requires explicit either sampling for flat interpolation",
                                variant.name,
                                entry.name
                            );
                        }
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

/// Reads the manifest the public export seam wrote; a missing or empty artifact is not a pass.
fn exported_manifest(output: &std::path::Path) -> serde_json::Value {
    let path = output.join("shaders.json");
    let bytes =
        std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    assert!(!bytes.is_empty(), "{} is empty", path.display());
    let manifest: serde_json::Value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let variants = manifest["variants"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: variants", path.display()));
    assert!(!variants.is_empty(), "{} declares no variants", path.display());
    manifest
}

/// Reads one described production group by the bind group index the pass binds it at.
fn described_group(manifest: &serde_json::Value, entry: &str, group: u64) -> serde_json::Value {
    let exported = manifest["variants"]
        .as_array()
        .expect("exported variants")
        .iter()
        .find(|item| item["name"].as_str() == Some(entry))
        .unwrap_or_else(|| panic!("{entry} is not exported"));
    let groups = exported["layoutGroups"].as_array().unwrap_or_else(|| {
        panic!("{entry}: missing layoutGroups; production group {group} is not described")
    });
    groups
        .iter()
        .find(|described| described["group"].as_u64() == Some(group))
        .unwrap_or_else(|| panic!("{entry}: group {group} is not described"))
        .clone()
}

/// Projects a described group into binding number, kind and stage usage, ascending by binding.
fn described_bindings(group: &serde_json::Value, entry: &str) -> Vec<(u64, String, String)> {
    group["bindings"]
        .as_array()
        .unwrap_or_else(|| panic!("{entry}: described group has no bindings"))
        .iter()
        .map(|binding| {
            let number = binding["binding"]
                .as_u64()
                .unwrap_or_else(|| panic!("{entry}: described binding without a number"));
            let kind = binding["kind"]
                .as_str()
                .unwrap_or_else(|| panic!("{entry}: binding {number} without a kind"));
            let mut visibility: Vec<&str> = binding["visibility"]
                .as_array()
                .unwrap_or_else(|| panic!("{entry}: binding {number} without a visibility"))
                .iter()
                .filter_map(|stage| stage.as_str())
                .collect();
            visibility.sort_unstable();
            (number, kind.to_owned(), visibility.join("+"))
        })
        .collect()
}

/// Reads one described binding for the interface fields a pipeline link depends on.
fn described_binding<'a>(
    group: &'a serde_json::Value,
    entry: &str,
    binding: u64,
) -> &'a serde_json::Value {
    group["bindings"]
        .as_array()
        .expect("described bindings")
        .iter()
        .find(|described| described["binding"].as_u64() == Some(binding))
        .unwrap_or_else(|| panic!("{entry}: binding {binding} is not described"))
}

/// Asserts a described group repeats the production actor dissolve descriptor.
///
/// `crates/render/src/actor_render/masks.rs:61` builds binding 0 as a two-dimensional depth
/// texture whose sample count follows the mask variant, and binding 1 as the compatibility
/// sampler that makes a depth texture samplable without filtering.
fn assert_dissolve_group(
    entry: &str,
    group: &serde_json::Value,
    multisampled: bool,
    compatibility: bool,
) {
    let mut expected = vec![(0_u64, "depthTexture".to_owned(), "fragment".to_owned())];
    if compatibility {
        expected.push((1, "sampler".to_owned(), "fragment".to_owned()));
    }
    assert_eq!(
        described_bindings(group, entry),
        expected,
        "{entry}: described dissolve group differs from the production descriptor"
    );
    let depth = described_binding(group, entry, 0);
    assert_eq!(
        depth["viewDimension"].as_str(),
        Some("d2"),
        "{entry}: dissolve depth must stay two-dimensional"
    );
    assert_eq!(
        depth["multisampled"].as_bool(),
        Some(multisampled && !compatibility),
        "{entry}: dissolve depth samples disagree with the mask variant and device profile"
    );
    if compatibility {
        let sampler = described_binding(group, entry, 1);
        assert_eq!(
            sampler["sampler"].as_str(),
            Some("nonFiltering"),
            "{entry}: compatibility must keep the nearest dissolve sampler"
        );
    }
}

/// Asserts a described group repeats the production entity-shadow descriptor.
///
/// `crates/render/src/entity_shadow_render/gpu.rs:44` builds the dynamic view uniform at binding 0,
/// the scene depth at binding 1, the compatibility sampler at binding 2, and the caster and
/// parameter buffers at bindings 3 and 4.
fn assert_shadow_group(
    entry: &str,
    group: &serde_json::Value,
    multisampled: bool,
    compatibility: bool,
) {
    let mut expected: Vec<(u64, String, String)> = vec![
        (0, "uniformBuffer".to_owned(), "fragment+vertex".to_owned()),
        (1, "depthTexture".to_owned(), "fragment".to_owned()),
        (3, "storageBuffer".to_owned(), "fragment+vertex".to_owned()),
        (4, "uniformBuffer".to_owned(), "fragment+vertex".to_owned()),
    ];
    if compatibility {
        expected.insert(2, (2, "sampler".to_owned(), "fragment".to_owned()));
    }
    assert_eq!(
        described_bindings(group, entry),
        expected,
        "{entry}: described shadow group differs from the production descriptor"
    );
    let depth = described_binding(group, entry, 1);
    assert_eq!(
        depth["viewDimension"].as_str(),
        Some("d2"),
        "{entry}: shadow depth must stay two-dimensional"
    );
    assert_eq!(
        depth["multisampled"].as_bool(),
        Some(multisampled && !compatibility),
        "{entry}: shadow depth samples disagree with the shadow variant and device profile"
    );
    if compatibility {
        let sampler = described_binding(group, entry, 2);
        assert_eq!(
            sampler["sampler"].as_str(),
            Some("nonFiltering"),
            "{entry}: compatibility must keep the nearest shadow sampler"
        );
    }
}

/// The exported manifest must describe the private groups the dissolve and shadow passes bind.
///
/// The mask colour pass binds its own companion depth at group 3 and the entity-shadow pass binds
/// scene depth at group 0, so each exported variant of either pass must carry the group index, the
/// production label and the real descriptor entries, with the sample count and the compatibility
/// sampler agreeing with the exported profile.
#[test]
fn exported_manifest_describes_production_dissolve_and_shadow_bind_groups() {
    let Some(output) = std::env::var_os("CINNABAR_SHADER_FIXTURE_OUT") else {
        eprintln!("missing-fixture: CINNABAR_SHADER_FIXTURE_OUT; layout description not checked");
        return;
    };
    let output = std::path::PathBuf::from(output);
    let manifest = exported_manifest(&output);
    let exported: Vec<(String, String, String)> = manifest["variants"]
        .as_array()
        .expect("exported variants")
        .iter()
        .filter_map(|item| {
            Some((
                item["name"].as_str()?.to_owned(),
                item["mode"].as_str()?.to_owned(),
                item["fragment"].as_str().unwrap_or_default().to_owned(),
            ))
        })
        .collect();
    let mut dissolve_variants = 0;
    let mut shadow_variants = 0;
    for variant in variants() {
        let dissolve = variant.name.starts_with("actor-mask-dissolve");
        let shadow = variant.name.starts_with("entity-shadow-");
        let fragment = match (dissolve, shadow) {
            (true, _) => "actor_mask_fragment",
            (_, true) => "shadow_fragment",
            _ => continue,
        };
        if dissolve {
            dissolve_variants += 1;
        }
        if shadow {
            shadow_variants += 1;
        }
        let multisampled = if dissolve {
            variant.definitions.contains(&"MASK_DISSOLVE_MSAA")
        } else {
            variant.definitions.contains(&"MULTISAMPLED")
        };
        let prefix = format!("{}-", variant.name);
        for (mode, compatibility) in [("core", false), ("compatibility", true)] {
            let entries: Vec<&str> = exported
                .iter()
                .filter(|(name, profile, exported_fragment)| {
                    profile == mode && exported_fragment == fragment && name.starts_with(&prefix)
                })
                .map(|(name, _, _)| name.as_str())
                .collect();
            assert_eq!(
                entries.len(),
                1,
                "{}: expected one exported {mode} {fragment} entry, found {entries:?}",
                variant.name
            );
            let entry = entries[0];
            let group = described_group(&manifest, entry, if dissolve { 3 } else { 0 });
            let label = if dissolve {
                "mask dissolve depth"
            } else {
                "entity shadow bind group layout"
            };
            assert_eq!(
                group["label"].as_str(),
                Some(label),
                "{entry}: described group label differs from the production descriptor"
            );
            if dissolve {
                assert_dissolve_group(entry, &group, multisampled, compatibility);
            } else {
                assert_shadow_group(entry, &group, multisampled, compatibility);
            }
        }
    }
    assert!(
        dissolve_variants > 0 && shadow_variants > 0,
        "the exported inventory lost a bind-group-describing variant family"
    );
}
