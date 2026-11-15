//! Export production actor WGSL for the browser's real compatibility validator.
#![allow(
    dead_code,
    reason = "reuse production shader constructors in an export fixture"
)]
#![expect(
    clippy::print_stderr,
    reason = "name the missing optional browser fixture output"
)]

#[path = "../src/material_shader.rs"]
mod material_shader;
#[path = "../src/shader_safety.rs"]
mod shader_safety;
#[path = "it/support/shader_source.rs"]
mod shader_source;

/// Emits both device modes without replacing any production shader operations.
#[test]
fn export_actor_browser_fixture() {
    let Some(output) = std::env::var_os("CINNABAR_SHADER_FIXTURE_OUT") else {
        eprintln!(
            "missing-fixture: CINNABAR_SHADER_FIXTURE_OUT; browser shader validation not run"
        );
        return;
    };
    let output = std::path::PathBuf::from(output);
    std::fs::create_dir_all(&output).expect("fixture output directory");
    let shader = shader_safety::from_actor_wesl(
        include_str!("../src/actor.wesl"),
        "actor.wesl",
        render_model::ACTOR_GPU_INSTANCE_WORDS,
        render_model::ACTOR_RIG_VERTEX_WORDS,
    );
    let bevy::shader::Source::Wesl(source) = shader.source else {
        panic!("actor constructor must produce WESL");
    };
    for (mode, definitions) in [
        ("core", &[][..]),
        ("compatibility", &["WEBGPU_COMPATIBILITY"][..]),
    ] {
        let wgsl = shader_source::standalone_prepared(&source, definitions);
        let module = naga::front::wgsl::parse_str(&wgsl).expect("production actor WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("production actor WGSL validates before browser restrictions");
        std::fs::write(output.join(format!("actor-{mode}.wgsl")), wgsl)
            .expect("write generated production actor WGSL");
    }
    std::fs::write(
        output.join("shaders.json"),
        r#"{"variants":[{"name":"actor","mode":"core","file":"actor-core.wgsl","vertex":"actor_vertex","fragment":"actor_fragment"},{"name":"actor","mode":"compatibility","file":"actor-compatibility.wgsl","vertex":"actor_vertex","fragment":"actor_fragment"}]}"#,
    )
    .expect("write shader fixture manifest");
    std::fs::write(
        output.join("index.html"),
        include_str!("webgpu_compatibility/index.html"),
    )
    .expect("write browser fixture page");
    std::fs::write(
        output.join("probe.mjs"),
        include_str!("webgpu_compatibility/probe.mjs"),
    )
    .expect("write browser fixture runner");
}
