const parameters = new URL(location.href).searchParams;
const mode = parameters.get("mode") ?? "compat";
const suite = parameters.get("suite") ?? "all";
const record = { mode, suite, userAgent: navigator.userAgent, tests: [], textures: [], adapters: [], devices: [], uncaptured: [], deviceLost: [] };
let finished = false;
const restorations = [];
const liveDevices = new Set();
function check(id, ok, details = {}) {
    record.tests.push({ ...details, id, ok: Boolean(ok) });
}
function wrap(prototype, name, replacement) {
  const original = prototype[name];
  prototype[name] = replacement(original);
  restorations.push(() => { prototype[name] = original; });
}
function observeBackend() {
  const featuresDescriptor = Object.getOwnPropertyDescriptor(GPUAdapter.prototype, "features");
  const originalFeatures = (adapter) => Reflect.apply(featuresDescriptor.get, adapter, []);
  if (mode === "compat") {
    Object.defineProperty(GPUAdapter.prototype, "features", { ...featuresDescriptor, get() { return new Set([...originalFeatures(this)].filter((feature) => feature !== "core-features-and-limits")); } });
    restorations.push(() => Object.defineProperty(GPUAdapter.prototype, "features", featuresDescriptor));
  }
  wrap(GPU.prototype, "requestAdapter", (original) => async function (options) {
    const adapter = await Reflect.apply(original, this, [options]);
    record.adapters.push({ options, available: Boolean(adapter), originalFeatures: adapter ? [...originalFeatures(adapter)].sort() : [], presentedFeatures: adapter ? [...adapter.features].sort() : [], diagnosticAdapterFeatureFilter: mode === "compat", info: adapter ? { vendor: adapter.info?.vendor, architecture: adapter.info?.architecture, device: adapter.info?.device, description: adapter.info?.description } : null });
    return adapter;
  });
  wrap(GPUAdapter.prototype, "requestDevice", (original) => async function (descriptor) {
    const requestedFeatures = [...(descriptor?.requiredFeatures ?? [])];
    const effectiveFeatures = mode === "compat" ? requestedFeatures.filter((feature) => feature !== "core-features-and-limits") : requestedFeatures;
    const effectiveDescriptor = mode === "compat" ? { ...descriptor, requiredFeatures: effectiveFeatures } : descriptor;
    record.modeInjection = mode === "compat" ? "diagnostic GPUAdapter.features and requestDevice requiredFeatures omit core-features-and-limits; original device.features are recorded unchanged" : "none";
    const device = await Reflect.apply(original, this, [effectiveDescriptor]);
    liveDevices.add(device);
    record.devices.push({ requestedFeatures, effectiveFeatures, features: [...device.features].sort(), core: device.features.has("core-features-and-limits") });
    device.addEventListener("uncapturederror", (event) => record.uncaptured.push(event.error.message));
    device.lost.then(({ reason, message }) => { if (!finished && reason !== "destroyed") record.deviceLost.push({ reason, message }); });
    return device;
  });
  wrap(GPUDevice.prototype, "createTexture", (original) => function (descriptor) {
    record.textures.push({ label: descriptor.label ?? "", hasBindingViewDimension: Object.hasOwn(descriptor, "textureBindingViewDimension"), bindingViewDimension: descriptor.textureBindingViewDimension ?? null, descriptor: structuredClone(descriptor) });
    return Reflect.apply(original, this, [descriptor]);
  });
}
async function fetchJson(path) {
  const response = await fetch(path, { cache: "no-store" });
  if (!response.ok) throw new Error(path + ": HTTP " + response.status);
  return response.json();
}
async function textures() {
  const module = await import("./pkg/texture_compat_fixture.js");
  await module.default();
  const value = await module.run_texture_fixture(mode);
  const result = typeof value === "string" ? JSON.parse(value) : value;
  record.textureFixture = result;
  check("rust-texture-tests-present", Array.isArray(result.tests) && result.tests.length > 0);
  for (const test of result.tests ?? []) record.tests.push({ ...test, id: "rust/" + test.id });
  for (const id of ["dropped-item-one-layer", "ui-one-layer", "dropped-item-one-layer-sample", "ui-one-layer-sample", "none-array-negative-control"]) check("required/" + id, result.tests?.some((test) => test.id === id && test.ok));
  const negative = result.tests?.find(({ id }) => id === "none-array-negative-control");
  check("negative-control-mode", negative?.expectedValidationError === (mode === "compat") && (mode === "compat" ? typeof negative.error === "string" && negative.error.length > 0 : negative?.error == null), { observed: negative });
  check("rust-descriptor-expectations-present", result.descriptorExpectations?.length > 0);
  check("rust-actual-capabilities", typeof result.adapter?.viewFormats === "boolean" && typeof result.adapter?.cubeArrayTextures === "boolean" && (mode === "core" || (!result.adapter.viewFormats && !result.adapter.cubeArrayTextures)), { adapter: result.adapter });
  for (const expected of result.descriptorExpectations ?? []) {
    const seen = record.textures.filter(({ label }) => label === expected.label);
    check("descriptor/" + expected.label, seen.length > 0 && seen.every((entry) => expected.viewDimension === null ? !entry.hasBindingViewDimension : entry.hasBindingViewDimension && entry.bindingViewDimension === expected.viewDimension), { expected, observed: seen });
  }
}
export async function validateShader(device, code, options) {
  const result = { ok: false, errors: [], compilation: [] };
  for (const filter of ["internal", "out-of-memory", "validation"]) device.pushErrorScope(filter);
  try {
    const module = device.createShaderModule({ label: options.name, code });
    result.compilation = (await module.getCompilationInfo()).messages.map(({ type, message, lineNum, linePos }) => ({ type, message, lineNum, linePos }));
    const pipeline = await device.createRenderPipelineAsync({ label: options.name, layout: "auto", vertex: { module, entryPoint: options.vertex, ...(options.buffers ? { buffers: options.buffers } : {}) }, ...(options.fragment ? { fragment: { module, entryPoint: options.fragment, targets: options.targets ?? [{ format: "rgba8unorm" }] } } : {}), primitive: options.primitive ?? { topology: "triangle-list" }, ...(options.depthStencil ? { depthStencil: options.depthStencil } : {}), ...(options.multisample ? { multisample: options.multisample } : {}) });
  if (options.semantic) result.semantic = await renderSemantic(device, pipeline, { id: options.name, ...options.semantic, format: options.targets?.[0]?.format ?? "rgba8unorm" });
  } catch (error) {
    result.errors.push(String(error?.message ?? error));
  } finally {
    for (let scope = 0; scope < 3; scope++) {
      const error = await device.popErrorScope();
      if (error) result.errors.push(error.message);
    }
  }
  result.ok = !result.errors.length && !result.compilation.some(({ type }) => type === "error") && (!options.semantic || result.semantic?.ok === true);
  return result;
}
async function shaders() {
  const manifest = await fetchJson("./shaders.json");
  const variants = manifest.variants.filter((entry) => entry.mode === (mode === "compat" ? "compatibility" : "core"));
  check("production-shader-variants-present", variants.length > 0);
  check("production-shader-required-variants-declared", manifest.coverage?.requiredVariants?.length > 0);
  check("production-shader-required-semantics-declared", manifest.coverage?.requiredSemanticCases?.length > 0);
  for (const name of manifest.coverage?.requiredVariants ?? []) check("required-shader/" + name, variants.some((variant) => variant.name === name));
  for (const name of manifest.coverage?.requiredSemanticCases ?? []) check("required-shader-semantic/" + name, variants.some((variant) => variant.name === name && variant.semantic?.expectedPixels?.length));
  record.shaderCoverage = { pipelineOnly: !variants.some((variant) => variant.semantic?.expectedPixels?.length), semanticCases: variants.filter((variant) => variant.semantic).map(({ name }) => name) };
  check("production-shader-semantic-coverage", !record.shaderCoverage.pipelineOnly);
  const adapter = await navigator.gpu.requestAdapter({ featureLevel: mode === "compat" ? "compatibility" : "core" });
  if (!adapter) throw new Error("no shader adapter for " + mode);
  const requiredLimits = {};
  for (const [name, value] of Object.entries(manifest.requiredLimits ?? { maxStorageBuffersPerShaderStage: 8, maxStorageBuffersInVertexStage: 8, maxStorageBuffersInFragmentStage: 8, maxInterStageShaderVariables: 15 })) {
    if (typeof adapter.limits[name] === "number") requiredLimits[name] = value;
  }
  const device = await adapter.requestDevice({ requiredFeatures: mode === "core" ? ["core-features-and-limits"] : [], requiredLimits });
  await shaderControls(device);
  for (const variant of variants) {
    const response = await fetch(new URL(variant.file, location.href), { cache: "no-store" });
    if (!response.ok) throw new Error("shader " + variant.file + ": HTTP " + response.status);
    record.tests.push({ id: "shader/" + variant.name, ...await validateShader(device, await response.text(), variant) });
  }
}
async function run() {
  record.unhandledRejections = [];
  window.addEventListener("unhandledrejection", (event) => record.unhandledRejections.push(String(event.reason?.stack ?? event.reason)));
  try {
    if (!["core", "compat"].includes(mode)) throw new Error("mode must be core or compat");
    if (!["all", "textures", "shaders"].includes(suite)) throw new Error("unknown suite");
    record.provenance = await fetchJson("./provenance.json");
    if (!navigator.gpu) throw new Error("navigator.gpu unavailable");
    observeBackend();
    if (suite !== "shaders") await textures();
    if (suite !== "textures") await shaders();
    check("actual-device-mode", record.devices.length > 0 && record.devices.every(({ core }) => core === (mode === "core")), { devices: record.devices });
    for (const device of liveDevices) await device.queue.onSubmittedWorkDone();
  } catch (error) {
    check("fixture-execution", false, { error: String(error?.stack ?? error) });
  } finally {
    finished = true;
    for (const restore of restorations.reverse()) restore();
    for (const device of liveDevices) device.destroy();
  }
  record.verdict = record.tests.length && record.tests.every(({ ok }) => ok) && !record.uncaptured.length && !record.deviceLost.length && !record.unhandledRejections.length ? "PASS" : "FAIL";
  document.getElementById("record").textContent = JSON.stringify(record, null, 2);
  document.getElementById("verdict").textContent = record.verdict;
  document.body.dataset.verdict = record.verdict;
  return record;
}
if (typeof document !== "undefined") await run();
async function renderSemantic(device, pipeline, semantic) {
  const width = semantic.width ?? 4;
  const height = semantic.height ?? 4;
  const bytesPerRow = Math.ceil(width * 4 / 256) * 256;
  const resources = [];
  try {
    const target = device.createTexture({ label: "shader semantic readback", size: [width, height], format: semantic.format, usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC });
    const readback = device.createBuffer({ size: bytesPerRow * height, usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ });
    resources.push(target, readback);
    const groups = new Map();
    for (const binding of semantic.bindings ?? []) {
      let resource;
      if (binding.texture) {
        const input = binding.texture;
        const texture = device.createTexture({ label: "shader semantic artwork", size: input.size, format: input.format, usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST, ...(input.viewDimension ? { textureBindingViewDimension: input.viewDimension } : {}) });
        resources.push(texture);
        device.queue.writeTexture({ texture }, new Uint8Array(input.rgba), { bytesPerRow: input.size[0] * 4, rowsPerImage: input.size[1] }, input.size);
        resource = texture.createView({ dimension: input.viewDimension ?? "2d" });
      } else if (binding.sampler) {
        resource = device.createSampler(binding.sampler);
      } else {
        const buffer = device.createBuffer({ size: Math.max(4, Math.ceil(binding.bytes.length / 4) * 4), usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.STORAGE, mappedAtCreation: true });
        new Uint8Array(buffer.getMappedRange()).set(binding.bytes);
        buffer.unmap();
        resources.push(buffer);
        resource = { buffer };
      }
      const entries = groups.get(binding.group) ?? [];
      entries.push({ binding: binding.binding, resource });
      groups.set(binding.group, entries);
    }
    const encoder = device.createCommandEncoder();
    const pass = encoder.beginRenderPass({ colorAttachments: [{ view: target.createView(), loadOp: "clear", storeOp: "store", clearValue: [0, 0, 0, 0] }] });
    pass.setPipeline(pipeline);
    for (const [group, entries] of groups) pass.setBindGroup(group, device.createBindGroup({ layout: pipeline.getBindGroupLayout(group), entries }));
    for (const vertex of semantic.vertexBuffers ?? []) {
      const buffer = device.createBuffer({ size: Math.max(4, Math.ceil(vertex.bytes.length / 4) * 4), usage: GPUBufferUsage.VERTEX, mappedAtCreation: true });
      new Uint8Array(buffer.getMappedRange()).set(vertex.bytes);
      buffer.unmap();
      resources.push(buffer);
      pass.setVertexBuffer(vertex.slot, buffer);
    }
    pass.draw(semantic.vertexCount ?? 3);
    pass.end();
    encoder.copyTextureToBuffer({ texture: target }, { buffer: readback, bytesPerRow }, { width, height });
    device.queue.submit([encoder.finish()]);
    await readback.mapAsync(GPUMapMode.READ);
    const bytes = new Uint8Array(readback.getMappedRange());
    const pixels = (semantic.expectedPixels ?? []).map(({ x, y, rgba, tolerance = 1 }) => {
      const actual = [...bytes.slice(y * bytesPerRow + x * 4, y * bytesPerRow + x * 4 + 4)];
      return { x, y, expected: rgba, actual, ok: actual.length === 4 && rgba.length === 4 && actual.every((value, index) => Math.abs(value - rgba[index]) <= tolerance) };
    });
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    canvas.style.cssText = "width:128px;height:128px;image-rendering:pixelated";
    const image = canvas.getContext("2d").createImageData(width, height);
    for (let y = 0; y < height; y++) image.data.set(bytes.slice(y * bytesPerRow, y * bytesPerRow + width * 4), y * width * 4);
    canvas.getContext("2d").putImageData(image, 0, 0);
    document.getElementById("verdict").before(canvas);
    readback.unmap();
    return { id: semantic.id, ok: pixels.length > 0 && pixels.every(({ ok }) => ok), pixels };
  } finally {
    for (const resource of resources) resource.destroy();
  }
}
async function shaderControls(device) {
  const vertex = "@vertex fn vertex(@builtin(vertex_index) i:u32)->@builtin(position) vec4<f32>{let xy=vec2<f32>(f32((i<<1u)&2u),f32(i&2u));return vec4<f32>(xy*2.0-vec2<f32>(1.0),0.0,1.0);}";
  const controls = [
    { id: "flat-first", pattern: /interpolation sampling.*First/i, code: "struct V{@builtin(position) p:vec4<f32>,@location(0) @interpolate(flat) value:u32,};@vertex fn vertex(@builtin(vertex_index) i:u32)->V{let xy=vec2<f32>(f32((i<<1u)&2u),f32(i&2u));return V(vec4<f32>(xy*2.0-vec2<f32>(1.0),0.0,1.0),7u);}@fragment fn fragment(v:V)->@location(0) vec4<f32>{return vec4<f32>(f32(v.value)/7.0,0.0,0.0,1.0);}" },
    { id: "sample-index", pattern: /sample[_ ]?index/i, code: vertex + "@fragment fn fragment(@builtin(sample_index) sample:u32)->@location(0) vec4<f32>{return vec4<f32>(f32(sample),1.0,0.0,1.0);}" },
  ];
  for (const control of controls) {
    const validation = await validateShader(device, control.code, { name: "synthetic " + control.id, vertex: "vertex", fragment: "fragment" });
    const errors = [...validation.errors, ...validation.compilation.filter(({ type }) => type === "error").map(({ message }) => message)].join("\n");
    check("control/" + control.id, mode === "core" ? validation.ok : !validation.ok && control.pattern.test(errors), { scope: "synthetic validator control, not production behavior proof", observedVerdict: validation.ok ? "PASS" : "FAIL", expectedValidationError: mode === "compat", validation });
  }
    record.hardwareControlSource = { repository: "maxxcraft", commit: "97e24282ea41f51d171829dcc006e4f994fd803b", path: "tests/stage1/compat-probe.mjs", generator: "varyingShader unchanged" };
      record.hardwareControls = [];
      const adapter = record.adapters.at(-1)?.info;
      const poco = mode === "compat" && adapter?.vendor === "img-tec" && adapter?.architecture === "rogue";
      for (const [count, flat, rgba] of [[13, true, [99, 0, 200, 255]], [13, false, [116, 0, 0, 255]], [15, true, [134, 0, 200, 255]], [15, false, [153, 0, 0, 255]]]) {
        const id = `varyings-${count}-${flat ? "flat" : "float"}`;
        const validation = await validateShader(device, varyingShader(count, flat), { name: id, vertex: "vs", fragment: "fs", semantic: { width: 1, height: 1, expectedPixels: [{ x: 0, y: 0, rgba, tolerance: 2 }] } });
        const expectedRed = poco && count === 15;
        const error = validation.errors.join("\n");
        const observed = { id, scope: "original synthetic hardware packing probe, not production proof", observedVerdict: validation.ok ? "PASS" : "FAIL", expectedPocoVerdict: count === 15 ? "FAIL" : "PASS", pocoAdapter: poco, observationOnly: count === 15 && !poco, validation };
        record.hardwareControls.push(observed);
        const ok = expectedRed ? !validation.ok && /Could not pack varying|Program link failed|inter.?stage.*(limit|maximum|exceed)/i.test(error) : count === 15 ? true : validation.ok;
        check("control/" + id, ok, observed);
      }
}
function varyingShader(count, flat) {
  const floats = flat ? count - 1 : count;
  const outputs = Array.from({ length: floats }, (_, k) => `@location(${k}) v${k}: vec4f,`).join("\n  ");
  const assigns = Array.from({ length: floats }, (_, k) => `o.v${k} = vec4f(${(k + 1) / 100});`).join("\n  ");
  const sum = Array.from({ length: floats }, (_, k) => `i.v${k}.x`).join(" + ");
  return `
struct Out {
  @builtin(position) position: vec4f,
  ${outputs}
  ${flat ? `@location(${floats}) @interpolate(flat, either) tag: u32,` : ""}
};
@vertex fn vs(@builtin(vertex_index) index: u32) -> Out {
  var p = array<vec2f, 3>(vec2f(-1.0, -1.0), vec2f(3.0, -1.0), vec2f(-1.0, 3.0));
  var o: Out;
  o.position = vec4f(p[index], 0.0, 1.0);
  ${assigns}
  ${flat ? "o.tag = 200u;" : ""}
  return o;
}
@fragment fn fs(i: Out) -> @location(0) vec4f {
  return vec4f((${sum}) / 2.0, 0.0, ${flat ? "f32(i.tag) / 255.0" : "0.0"}, 1.0);
}`;
}
