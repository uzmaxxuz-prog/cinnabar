export async function validateShader(device, code, { vertex = "actor_vertex", fragment = "actor_fragment", targets = [{ format: "rgba8unorm" }], depthStencil } = {}) {
  const result = { ok: false, errors: [], compilation: [] };
  device.pushErrorScope("validation");
  try {
    const module = device.createShaderModule({ label: "production shader", code });
    const info = await module.getCompilationInfo();
    result.compilation = info.messages.map(({ type, message, lineNum, linePos }) => ({ type, message, lineNum, linePos }));
    await device.createRenderPipelineAsync({
      label: "production shader compatibility regression",
      layout: "auto",
      vertex: { module, entryPoint: vertex },
      fragment: { module, entryPoint: fragment, targets },
      ...(depthStencil ? { depthStencil } : {}),
      primitive: { topology: "triangle-list" },
    });
  } catch (error) {
    result.errors.push(String(error?.message ?? error));
  } finally {
    const error = await device.popErrorScope();
    if (error) result.errors.push(error.message);
  }
  result.ok = !result.errors.length && !result.compilation.some(({ type }) => type === "error");
  return result;
}

async function run() {
  const params = new URL(location.href).searchParams;
  const mode = params.get("mode") ?? "compatibility";
  const suite = params.get("suite") ?? "actor";
  if (!["actor", "scene-depth"].includes(suite)) throw new Error("invalid shader suite");
  if (!["core", "compatibility"].includes(mode)) throw new Error("invalid device mode");
  const record = { mode, userAgent: navigator.userAgent, tests: [], uncaptured: [], deviceLost: null, renderedPixels: false };
  if (!navigator.gpu) throw new Error("navigator.gpu unavailable");
  const adapter = await navigator.gpu.requestAdapter({ featureLevel: mode });
  if (!adapter) throw new Error(`no ${mode} adapter`);
  const requiredLimits = {};
  for (const [key, value] of Object.entries({ maxStorageBuffersPerShaderStage: 8, maxStorageBuffersInVertexStage: 8, maxStorageBuffersInFragmentStage: 8, maxInterStageShaderVariables: 15 })) {
    if (typeof adapter.limits[key] === "number") requiredLimits[key] = value;
  }
  const requiredFeatures = mode === "core" ? ["core-features-and-limits"] : [];
  const device = await adapter.requestDevice({ requiredLimits, requiredFeatures });
  let finished = false;
  device.addEventListener("uncapturederror", (event) => record.uncaptured.push(event.error.message));
  device.lost.then(({ reason, message }) => { if (!finished) record.deviceLost = { reason, message }; });
  try {
    record.deviceCore = device.features.has("core-features-and-limits");
    if (record.deviceCore !== (mode === "core")) throw new Error(`requested ${mode} but deviceCore=${record.deviceCore}`);
    record.adapter = { vendor: adapter.info?.vendor, architecture: adapter.info?.architecture };
    const response = await fetch(`./${suite}-${mode}.wgsl`, { cache: "no-store" });
    if (!response.ok) throw new Error(`production shader fetch: HTTP ${response.status}`);
    const options = suite === "scene-depth" ? { vertex: "vertex", fragment: "single", targets: [], depthStencil: { format: "depth32float", depthWriteEnabled: true, depthCompare: "always" } } : {};
    record.tests.push({ id: `${suite}-production-pipeline`, ...await validateShader(device, await response.text(), options) });
    record.verdict = record.tests.every(({ ok }) => ok) && !record.uncaptured.length && !record.deviceLost ? "PASS" : "FAIL";
    return record;
  } finally {
    finished = true;
    device.destroy();
  }
}

run().catch((error) => ({ verdict: "FAIL", error: String(error?.message ?? error) })).then((record) => {
  document.getElementById("record").textContent = JSON.stringify(record, null, 2);
  document.getElementById("verdict").textContent = record.verdict;
  document.body.dataset.verdict = record.verdict;
});
