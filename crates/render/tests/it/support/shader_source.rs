//! Compose production WESL modules for shader validation and native pixel fixtures.
#![allow(
    dead_code,
    reason = "shared helpers serve different shader test targets"
)]
use crate::material_shader;

pub(crate) const VIEW: &str = "struct View { clip_from_world: mat4x4<f32>, unjittered_clip_from_world: mat4x4<f32>, view_from_world: mat4x4<f32>, world_from_view: mat4x4<f32>, clip_from_view: mat4x4<f32>, view_from_clip: mat4x4<f32>, world_position: vec3<f32>, exposure: f32, viewport: vec4<f32>, }";
const FULLSCREEN: &str = "struct FullscreenVertexOutput { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32>, }";
const FULLSCREEN_VERTEX: &str = "@vertex fn fullscreen(@builtin(vertex_index) index: u32) -> FullscreenVertexOutput { var out: FullscreenVertexOutput; out.uv = vec2(f32((index << 1u) & 2u), f32(index & 2u)); out.position = vec4(out.uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0); return out; }";

/// Reads the alpha cutoff guarding a fragment discard, including named WGSL constants.
pub fn alpha_discard_threshold(source: &str, function: &str) -> Option<f32> {
    let module = naga::front::wgsl::parse_str(&standalone(source, &[])).expect("shader parses");
    // An entry point, or the shading function an entry point delegates to.
    let function = module
        .entry_points
        .iter()
        .find(|point| point.name == function)
        .map(|point| &point.function)
        .or_else(|| {
            module
                .functions
                .iter()
                .map(|(_, candidate)| candidate)
                .find(|candidate| candidate.name.as_deref() == Some(function))
        })
        .expect("fragment function exists");
    discard_threshold(&module, function, &function.body)
}

/// Follows nested fragment guards to the comparison that actually discards its alpha.
fn discard_threshold(
    module: &naga::Module,
    function: &naga::Function,
    block: &naga::Block,
) -> Option<f32> {
    block.iter().find_map(|statement| match statement {
        naga::Statement::If {
            condition,
            accept,
            reject,
        } => {
            let threshold = accept
                .iter()
                .any(|statement| matches!(statement, naga::Statement::Kill))
                .then(|| alpha_comparison(module, function, *condition))
                .flatten();
            threshold
                .or_else(|| discard_threshold(module, function, accept))
                .or_else(|| discard_threshold(module, function, reject))
        }
        naga::Statement::Block(inner) => discard_threshold(module, function, inner),
        _ => None,
    })
}

/// Resolves the cutoff of an alpha-channel comparison inside a combined material guard.
fn alpha_comparison(
    module: &naga::Module,
    function: &naga::Function,
    condition: naga::Handle<naga::Expression>,
) -> Option<f32> {
    if let naga::Expression::Load { pointer } = function.expressions[condition] {
        return stored_alpha_comparison(module, function, &function.body, pointer, condition);
    }
    let naga::Expression::Binary { op, left, right } = function.expressions[condition] else {
        return None;
    };
    let alpha = match function.expressions[left] {
        naga::Expression::Load { pointer } => &function.expressions[pointer],
        _ => &function.expressions[left],
    };
    if matches!(
        op,
        naga::BinaryOperator::Less | naga::BinaryOperator::LessEqual
    ) && matches!(alpha, naga::Expression::AccessIndex { index: 3, .. })
    {
        let expression = match function.expressions[right] {
            naga::Expression::Constant(constant) => {
                &module.global_expressions[module.constants[constant].init]
            }
            _ => &function.expressions[right],
        };
        return match expression {
            naga::Expression::Literal(naga::Literal::F32(value)) => Some(*value),
            _ => None,
        };
    }
    alpha_comparison(module, function, left).or_else(|| alpha_comparison(module, function, right))
}

/// Follows Naga's temporary stores for short-circuit guards before the discard reads them.
fn stored_alpha_comparison(
    module: &naga::Module,
    function: &naga::Function,
    block: &naga::Block,
    pointer: naga::Handle<naga::Expression>,
    before: naga::Handle<naga::Expression>,
) -> Option<f32> {
    block.iter().find_map(|statement| match statement {
        naga::Statement::Store {
            pointer: target,
            value,
        } if value.index() < before.index()
            && (*target == pointer
                || matches!(
                    (&function.expressions[*target], &function.expressions[pointer]),
                    (naga::Expression::LocalVariable(left), naga::Expression::LocalVariable(right))
                        if left == right
                )) =>
        {
            alpha_comparison(module, function, *value)
        }
        naga::Statement::If { accept, reject, .. } => {
            stored_alpha_comparison(module, function, accept, pointer, before)
                .or_else(|| stored_alpha_comparison(module, function, reject, pointer, before))
        }
        naga::Statement::Block(inner) => {
            stored_alpha_comparison(module, function, inner, pointer, before)
        }
        _ => None,
    })
}

/// Resolves conditional attributes while retaining the module's imports and declaration names.
pub fn preprocess(source: &str, definitions: &[&str]) -> String {
    let source = material_shader::source(source);
    let mut module: wesl::syntax::TranslationUnit = source.parse().expect("WESL parses");
    wesl::pass::condcomp(&mut module, &features(definitions)).expect("shader flags resolve");
    module.to_string()
}

/// Preserves helper names so native fixtures can call production shading functions directly.
pub fn standalone(source: &str, definitions: &[&str]) -> String {
    compile(source, definitions, false)
}

/// Composes a production constructor's output without injecting constants into its root again.
pub fn standalone_prepared(source: &str, definitions: &[&str]) -> String {
    compile_prepared(source, definitions, false)
}

/// Uses Bevy's WESL composition and pruning options before validating the resulting WGSL.
pub fn composed(source: &str, definitions: &[&str]) -> String {
    let source = if source.contains("import bevy_core_pipeline::fullscreen_vertex_shader") {
        format!("{source}\n{FULLSCREEN_VERTEX}")
    } else {
        source.to_owned()
    };
    let source = compile(&source, definitions, true);
    let module = naga::front::wgsl::parse_str(&source).expect("composed WGSL parses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("composed module validates");
    source
}

/// Enables precisely the requested pipeline flags; unspecified flags stay disabled.
fn features(definitions: &[&str]) -> wesl::Features {
    let mut features = wesl::Features::default();
    for definition in definitions {
        features.set(*definition, true);
    }
    features
}

/// Resolves generated constants and all production imports through the WESL compiler.
fn compile(source: &str, definitions: &[&str], prune: bool) -> String {
    let source = material_shader::source(&meshing::cloud_viewport::shader_source(source));
    compile_prepared(&source, definitions, prune)
}

/// Links a prepared root with Bevy's mangling and pruning, leaving WGSL validation to the caller.
pub fn link_prepared(source: &str, definitions: &[&str]) -> Result<String, String> {
    compile_prepared_with(source, definitions, true, Vec::new())
}

/// Composes a prepared root with pruning, resolving `modules` beside the production modules.
pub fn composed_prepared_with(
    source: &str,
    definitions: &[&str],
    modules: Vec<(&'static str, String)>,
) -> String {
    let source = compile_prepared_with(source, definitions, true, modules)
        .unwrap_or_else(|error| panic!("{error}"));
    let module = naga::front::wgsl::parse_str(&source).expect("composed WGSL parses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("composed module validates");
    source
}

/// Resolves imports while preserving the already prepared root module verbatim.
fn compile_prepared(source: &str, definitions: &[&str], prune: bool) -> String {
    compile_prepared_with(source, definitions, prune, Vec::new())
        .unwrap_or_else(|error| panic!("{error}"))
}

fn compile_prepared_with(
    source: &str,
    definitions: &[&str],
    prune: bool,
    modules: Vec<(&'static str, String)>,
) -> Result<String, String> {
    let mut resolver = wesl::resolver::VirtualResolver::new();
    for (path, source) in [
        ("bevy_render::view", VIEW.to_owned()),
        (
            "render::terrain_tables",
            include_str!("../../../src/terrain_tables.wesl").to_owned(),
        ),
        (
            "render::chunk_records",
            material_shader::source(include_str!("../../../src/chunk_records.wesl")),
        ),
        (
            "bevy_render::globals",
            "struct Globals { time: f32, delta_time: f32, frame_count: u32 }".to_owned(),
        ),
        (
            "bevy_core_pipeline::fullscreen_vertex_shader",
            FULLSCREEN.to_owned(),
        ),
        (
            "render::material",
            material_shader::source(include_str!("../../../src/material.wesl")),
        ),
        (
            "render::lighting",
            material_shader::source(include_str!("../../../src/lighting.wesl")),
        ),
        (
            "render::glint",
            material_shader::source(include_str!("../../../src/glint.wesl")),
        ),
        (
            "render::actor_rig",
            include_str!("../../../src/actor_rig.wesl").to_owned(),
        ),
        (
            "render::biome_tint",
            material_shader::bind_biome_tables(&meshing::biome_lattice::shader_source(
                include_str!("../../../src/biome_tint.wesl"),
            )),
        ),
        (
            "render::world_projection",
            include_str!("../../../src/world_projection.wesl").to_owned(),
        ),
        (
            "render::chunk_bindings",
            material_shader::source(include_str!("../../../src/chunk_bindings.wesl")),
        ),
        (
            "render::enhanced::common",
            include_str!("../../../src/enhanced/common.wesl").to_owned(),
        ),
        (
            "render::enhanced::view",
            include_str!("../../../src/enhanced/view.wesl").to_owned(),
        ),
        (
            "render::enhanced::caster",
            include_str!("../../../src/enhanced/caster.wesl").to_owned(),
        ),
        (
            "render::liquid",
            material_shader::source(include_str!("../../../src/liquid.wesl")),
        ),
        (
            "render::model",
            material_shader::source(include_str!("../../../src/model.wesl")),
        ),
        (
            "render::chunk",
            material_shader::source(include_str!("../../../src/chunk.wesl")),
        ),
    ]
    .into_iter()
    .chain(modules)
    {
        resolver.add_module(path.parse().expect("module path"), source.into());
    }
    let root: wesl::syntax::ModulePath = "fixture::shader".parse().expect("root module path");
    resolver.add_module(root.clone(), source.to_owned().into());
    let options = wesl::CompileOptions {
        visibility: false,
        features: features(definitions),
        strip: prune,
        mangler: if prune {
            wesl::ManglerKind::Escape
        } else {
            wesl::ManglerKind::None
        },
        ..Default::default()
    };
    wesl::compile(&root, &options, &FixtureResolver(resolver))
        .map(|module| module.to_string())
        .map_err(|error| error.diagnostic().render_plain())
}

/// EXPERIMENTAL export-only link with a canonical module order; not a claimed fix.
///
/// Twin of `link_prepared`: identical fixture registry, options, mangling and pruning,
/// but the linked unit is rebuilt from the compiler's already-retargeted and mangled
/// modules in a canonical order (root first, then module identity) instead of the
/// HashMap-derived discovery order, so independent exporter runs agree byte for byte.
/// Native callers keep using `link_prepared`.
pub fn link_prepared_export(source: &str, definitions: &[&str]) -> Result<String, String> {
    compile_prepared_ordered(source, definitions, true)
}

/// EXPERIMENTAL export-only standalone compile with a canonical module order.
///
/// Twin of `standalone_prepared` for the opt-in browser fixture exporters; native
/// callers keep using `standalone_prepared`.
pub fn standalone_prepared_export(source: &str, definitions: &[&str]) -> String {
    compile_prepared_ordered(source, definitions, false)
        .unwrap_or_else(|error| panic!("{error}"))
}

/// Re-links the prepared root with canonical module ordering, preserving the exact
/// declarations, mangled names and strip filter of the original pipeline.
///
/// Mirrors `compile_prepared_with`'s registry and options verbatim so the native seam
/// stays byte-identical. `wesl::compile` already ran retargeting, mangling, linking
/// and validation, and `CompileResult::modules` holds the final per-module trees, so
/// re-running `wesl::pass::link` with the original `used_items` filter reproduces the
/// linked unit with only the module order changed. Fails closed when any global
/// directive survives linking, because reordering modules would move its scope.
fn compile_prepared_ordered(
    source: &str,
    definitions: &[&str],
    prune: bool,
) -> Result<String, String> {
    let mut resolver = wesl::resolver::VirtualResolver::new();
    for (path, source) in [
        ("bevy_render::view", VIEW.to_owned()),
        (
            "render::terrain_tables",
            include_str!("../../../src/terrain_tables.wesl").to_owned(),
        ),
        (
            "render::chunk_records",
            material_shader::source(include_str!("../../../src/chunk_records.wesl")),
        ),
        (
            "bevy_render::globals",
            "struct Globals { time: f32, delta_time: f32, frame_count: u32 }".to_owned(),
        ),
        (
            "bevy_core_pipeline::fullscreen_vertex_shader",
            FULLSCREEN.to_owned(),
        ),
        (
            "render::material",
            material_shader::source(include_str!("../../../src/material.wesl")),
        ),
        (
            "render::lighting",
            material_shader::source(include_str!("../../../src/lighting.wesl")),
        ),
        (
            "render::glint",
            material_shader::source(include_str!("../../../src/glint.wesl")),
        ),
        (
            "render::actor_rig",
            include_str!("../../../src/actor_rig.wesl").to_owned(),
        ),
        (
            "render::biome_tint",
            material_shader::bind_biome_tables(&meshing::biome_lattice::shader_source(
                include_str!("../../../src/biome_tint.wesl"),
            )),
        ),
        (
            "render::world_projection",
            include_str!("../../../src/world_projection.wesl").to_owned(),
        ),
        (
            "render::chunk_bindings",
            material_shader::source(include_str!("../../../src/chunk_bindings.wesl")),
        ),
        (
            "render::enhanced::common",
            include_str!("../../../src/enhanced/common.wesl").to_owned(),
        ),
        (
            "render::enhanced::view",
            include_str!("../../../src/enhanced/view.wesl").to_owned(),
        ),
        (
            "render::enhanced::caster",
            include_str!("../../../src/enhanced/caster.wesl").to_owned(),
        ),
        (
            "render::liquid",
            material_shader::source(include_str!("../../../src/liquid.wesl")),
        ),
        (
            "render::model",
            material_shader::source(include_str!("../../../src/model.wesl")),
        ),
        (
            "render::chunk",
            material_shader::source(include_str!("../../../src/chunk.wesl")),
        ),
    ]
    .into_iter()
    {
        resolver.add_module(path.parse().expect("module path"), source.into());
    }
    let root: wesl::syntax::ModulePath = "fixture::shader".parse().expect("root module path");
    resolver.add_module(root.clone(), source.to_owned().into());
    let options = wesl::CompileOptions {
        visibility: false,
        features: features(definitions),
        strip: prune,
        mangler: if prune {
            wesl::ManglerKind::Escape
        } else {
            wesl::ManglerKind::None
        },
        ..Default::default()
    };
    let wesl::CompileResult {
        syntax,
        mut modules,
        used_items,
        ..
    } = wesl::compile(&root, &options, &FixtureResolver(resolver))
        .map_err(|error| error.diagnostic().render_plain())?;
    if !syntax.global_directives.is_empty() {
        return Err(
            "export ordering refuses global directives: reordering moves their scope".to_owned(),
        );
    }
    modules.sort_by(|left, right| {
        let left_root = left.path == root;
        let right_root = right.path == root;
        right_root
            .cmp(&left_root)
            .then_with(|| export_module_rank(&left.path).cmp(&export_module_rank(&right.path)))
    });
    let mut unit = wesl::pass::link(&modules, options.strip.then_some(&used_items));
    if options.lower {
        wesl::pass::lower(&mut unit).map_err(|error| error.diagnostic().render_plain())?;
    }
    if options.validate {
        wesl::pass::validate_wgsl(&unit).map_err(|diagnostic| diagnostic.render_plain())?;
    }
    Ok(unit.to_string())
}

/// Orders module identities: origin kind, package name, then path components.
fn export_module_rank(path: &wesl::syntax::ModulePath) -> (u8, Option<&str>, usize, &[String]) {
    match &path.origin {
        wesl::syntax::PathOrigin::Absolute => (0, None, 0, path.components.as_slice()),
        wesl::syntax::PathOrigin::Relative(levels) => {
            (1, None, *levels, path.components.as_slice())
        }
        wesl::syntax::PathOrigin::Package(package) => {
            (2, Some(package.as_str()), 0, path.components.as_slice())
        }
    }
}

/// Resolves dependency packages with the same crate names used by Bevy's shader cache.
struct FixtureResolver(wesl::resolver::VirtualResolver<'static>);

impl wesl::Resolver for FixtureResolver {
    /// Reads a fixture module after removing the importing package's dependency prefix.
    fn resolve_source<'a>(
        &'a self,
        path: &wesl::syntax::ModulePath,
    ) -> Result<std::borrow::Cow<'a, str>, wesl::error::ResolveError> {
        self.0.resolve_source(&self.canonical_path(path))
    }

    /// Treats nested dependency paths as the registered crate's absolute module identity.
    fn canonical_path(&self, path: &wesl::syntax::ModulePath) -> wesl::syntax::ModulePath {
        let mut path = path.clone();
        if let wesl::syntax::PathOrigin::Package(package) = &mut path.origin
            && let Some((_, name)) = package.rsplit_once('/')
        {
            *package = name.to_owned();
        }
        path
    }
}
