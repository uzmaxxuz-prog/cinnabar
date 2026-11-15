# Browser shader compatibility regression

The Rust fixture exports production actor WESL through the existing shader constructor and WESL resolver. Shared test view declarations are supplied by `it/support/shader_source.rs`; shader operations and interpolation qualifiers are not rewritten. Naga validates both generated modules before export. This is not a pixel or gameplay acceptance test.

Run the exporter in an isolated checkout of the recorded commit, inside the project's pinned build environment:

```sh
CINNABAR_SHADER_FIXTURE_OUT=/isolated/output/actor \
  cargo test --locked -p render --test webgpu_compatibility \
  export_actor_browser_fixture -- --exact --nocapture
```

Serve that output directory on loopback with `.html`, `.mjs` and `.wgsl` support. Open `index.html?mode=core` and `index.html?mode=compatibility` in the approved browser environment. The default suite is `actor`; `suite=scene-depth` instead consumes `scene-depth-${mode}.wgsl` from the depth fixture exporter, selecting `vertex`/`single`, no colour targets and a writable `depth32float` attachment.

The page requests a real device of the selected mode, verifies `core-features-and-limits`, fetches generated WGSL, records shader compilation messages and awaits real render pipeline creation. A missing adapter, failed fetch or device mode mismatch is FAIL, not a skipped successful run. `document.body.dataset.verdict` and the JSON in `#record` expose the result to a browser driver. Inspect the JSON, not just process completion.

Expected first regression: compatibility rejects the actor's implicit first-vertex flat interpolation; core creates the same production pipeline. This expected failure remains an actual FAIL in the record. The fixture does not relax qualifiers or add compatibility shims. Missing `CINNABAR_SHADER_FIXTURE_OUT` prints a named missing-fixture message and does not claim browser validation.

## Linked interface and pixel fixtures

The broader exporter writes a `shaders.json` manifest for the shared browser executor:

```sh
CINNABAR_SHADER_FIXTURE_OUT=/isolated/output/shaders \
  cargo test --locked -p render --test shader_interfaces \
  export_linked_browser_fixtures -- --exact --nocapture
```

It resolves actor colour, motion and mask feature combinations, plus model, liquid, transparent and chunk interfaces. The separate `linked_compatibility_interfaces_have_dense_locations_and_fit_the_device` test reflects Naga entry-point bindings, rejects First and sample_index, and counts `front_facing` in the twelve-variable budget. Exporting does not run that assertion, so an incompatible baseline can still reach the browser.

The hand raster witnesses call the unchanged production vertex operations. Only entry-point annotations and builtin argument annotations are removed to make that vertex callable. Core uses ordinary First interpolation; the compatibility witness deliberately repeats the last corner's computed shade across the triangle. Distinct-normal and distinct-bone fixtures independently expect ambient shade 0.4 (RGBA8 102), making a nonconstant producer fail even on drivers that happen to select the first corner for either interpolation. Triangle positions remain identical between those cases.

Actor material witnesses use the actual packed Rust instance/geometry layouts and production vertex/fragment functions with original one-texel artwork. Independent expectations cover red tint and partial green overlay, including byte precision. Their uniform view declaration remains the test resolver's documented view fixture, not Bevy's complete application view ABI. These witnesses test shader semantics, not the application's resource allocator or full gameplay frame.

The manifest derives required variant names and semantic cases from its exported inventory. The browser must create real pipelines, draw, copy pixels to a buffer and compare independent expected colours; a missing case is not a pass. Pipeline creation alone cannot close target-GPU drawing, deliberately failing varying-limit controls, full application core before/after captures, or gameplay acceptance.
