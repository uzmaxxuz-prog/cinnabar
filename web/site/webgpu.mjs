/** Required WebGPU adapter limits for the Cinnabar terrain and shader renderer. */
export const REQUIRED_LIMITS = Object.freeze({
  maxTextureArrayLayers: 1024,
  maxSampledTexturesPerShaderStage: 10,
  maxSamplersPerShaderStage: 2,
  maxBindingsPerBindGroup: 27,
  maxStorageBuffersPerShaderStage: 4,
});

const COMPAT_STAGE_STORAGE_LIMITS = Object.freeze([
  "maxStorageBuffersInVertexStage",
  "maxStorageBuffersInFragmentStage",
]);

/** Extracts a 64-hex Stage 1 release ID from a versioned `/releases/<id>/...` path. */
export function extractReleaseId(pathname) {
  const match = /^\/releases\/([0-9a-f]{64})(?:\/|$)/.exec(pathname || "");
  return match ? match[1] : null;
}

/** Chooses `same-origin` credentials only for same-origin HTTPS requests and refuses redirects. */
export function credentialsFor(url, pageOrigin) {
  const parsed = url instanceof URL ? url : new URL(url);
  return pageOrigin && parsed.origin === pageOrigin ? "same-origin" : "omit";
}

/** Checks browser WebGPU availability, limits and device creation before downloading assets or booting WASM. */
export async function checkWebGpu({ navigator, isSecureContext } = globalThis) {
  if (!isSecureContext) {
    return { ok: false, reason: "insecure_context", message: "WebGPU requires a secure HTTPS context." };
  }
  const gpu = navigator?.gpu;
  if (!gpu || typeof gpu.requestAdapter !== "function") {
    return { ok: false, reason: "no_webgpu", message: "WebGPU is not available in this browser." };
  }
  let adapter;
  try {
    adapter = await gpu.requestAdapter({ powerPreference: "high-performance", featureLevel: "compatibility" });
  } catch (error) {
    return { ok: false, reason: "adapter_error", message: `WebGPU adapter request failed: ${error?.message || error}` };
  }
  if (!adapter) {
    return { ok: false, reason: "no_adapter", message: "No WebGPU adapter is available in this browser." };
  }
  const info = adapter.info ?? {};
  const fallback = Boolean(info.isFallbackAdapter ?? adapter.isFallbackAdapter);
  const core = Boolean(adapter.features?.has?.("core-features-and-limits"));
  const limits = adapter.limits ?? {};
  const failures = [];
  const requiredLimits = {};
  for (const [key, need] of Object.entries(REQUIRED_LIMITS)) {
    const have = limits[key];
    if (typeof have !== "number" || have < need) {
      failures.push(`${key} (required ${need}, supported ${have ?? "none"})`);
    } else {
      requiredLimits[key] = need;
    }
  }
  if (!core) {
    for (const key of COMPAT_STAGE_STORAGE_LIMITS) {
      const have = limits[key];
      if (typeof have === "number") {
        if (have < REQUIRED_LIMITS.maxStorageBuffersPerShaderStage) {
          failures.push(`${key} (required ${REQUIRED_LIMITS.maxStorageBuffersPerShaderStage}, supported ${have})`);
        } else {
          requiredLimits[key] = REQUIRED_LIMITS.maxStorageBuffersPerShaderStage;
        }
      }
    }
  }
  if (failures.length > 0) {
    const qualifier = fallback ? " (software/fallback adapter)" : "";
    return {
      ok: false,
      reason: "insufficient_limits",
      fallback,
      mode: core ? "core" : "compatibility",
      limits: { maxTextureArrayLayers: limits.maxTextureArrayLayers ?? null },
      message: `WebGPU adapter${qualifier} does not meet renderer limits: ${failures.join(", ")}.`,
    };
  }
  let device;
  try {
    const descriptor = { requiredLimits };
    if (core) descriptor.requiredFeatures = ["core-features-and-limits"];
    device = await adapter.requestDevice(descriptor);
    device?.destroy?.();
  } catch (error) {
    return {
      ok: false,
      reason: "device_error",
      fallback,
      mode: core ? "core" : "compatibility",
      message: `WebGPU device creation failed: ${error?.message || error}`,
    };
  }
  return {
    ok: true,
    fallback,
    mode: core ? "core" : "compatibility",
    limits: { maxTextureArrayLayers: limits.maxTextureArrayLayers },
  };
}
