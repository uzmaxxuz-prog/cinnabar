import { test } from "node:test";
import assert from "node:assert/strict";
import { checkWebGpu, credentialsFor, extractReleaseId, REQUIRED_LIMITS } from "./webgpu.mjs";

function makeAdapter({
  core = true,
  fallback = false,
  limits = {},
  requestDeviceError = null,
} = {}) {
  const fullLimits = {
    ...REQUIRED_LIMITS,
    maxTextureArrayLayers: 2048,
    maxStorageBuffersInVertexStage: 16,
    maxStorageBuffersInFragmentStage: 16,
    ...limits,
  };
  let destroyed = false;
  let lastDescriptor = null;
  const adapter = {
    info: { isFallbackAdapter: fallback },
    features: new Set(core ? ["core-features-and-limits"] : []),
    limits: fullLimits,
    async requestDevice(descriptor) {
      lastDescriptor = descriptor;
      if (requestDeviceError) throw requestDeviceError;
      return { destroy() { destroyed = true; } };
    },
    get destroyed() { return destroyed; },
    get lastDescriptor() { return lastDescriptor; },
  };
  return adapter;
}

test("checkWebGpu rejects insecure context and missing navigator.gpu", async () => {
  const insecure = await checkWebGpu({ navigator: { gpu: {} }, isSecureContext: false });
  assert.equal(insecure.ok, false);
  assert.equal(insecure.reason, "insecure_context");

  const missing = await checkWebGpu({ navigator: {}, isSecureContext: true });
  assert.equal(missing.ok, false);
  assert.equal(missing.reason, "no_webgpu");
});

test("checkWebGpu rejects null adapter without throwing", async () => {
  let receivedOptions = null;
  const result = await checkWebGpu({
    isSecureContext: true,
    navigator: {
      gpu: {
        async requestAdapter(options) {
          receivedOptions = options;
          return null;
        },
      },
    },
  });
  assert.equal(receivedOptions?.featureLevel, "compatibility");
  assert.equal(result.ok, false);
  assert.equal(result.reason, "no_adapter");
});

test("checkWebGpu rejects SwiftShader 256-layer adapter before boot", async () => {
  const adapter = makeAdapter({ fallback: true, limits: { maxTextureArrayLayers: 256 } });
  const result = await checkWebGpu({
    isSecureContext: true,
    navigator: { gpu: { async requestAdapter() { return adapter; } } },
  });
  assert.equal(result.ok, false);
  assert.equal(result.reason, "insufficient_limits");
  assert.equal(result.fallback, true);
  assert.match(result.message, /1024/);
  assert.match(result.message, /256/);
  assert.match(result.message, /software\/fallback/);
});

test("checkWebGpu accepts hardware compatibility adapter (POCO limits) and destroys probe device", async () => {
  const adapter = makeAdapter({ core: false, fallback: false, limits: { maxTextureArrayLayers: 2048 } });
  const result = await checkWebGpu({
    isSecureContext: true,
    navigator: { gpu: { async requestAdapter() { return adapter; } } },
  });
  assert.equal(result.ok, true);
  assert.equal(result.mode, "compatibility");
  assert.equal(adapter.destroyed, true);
  assert.equal(adapter.lastDescriptor.requiredFeatures, undefined);
  assert.equal(adapter.lastDescriptor.requiredLimits.maxStorageBuffersInVertexStage, 4);
  assert.equal(adapter.lastDescriptor.requiredLimits.maxTextureArrayLayers, 1024);
});

test("checkWebGpu accepts core hardware adapter and requests core-features-and-limits", async () => {
  const adapter = makeAdapter({ core: true, fallback: false });
  const result = await checkWebGpu({
    isSecureContext: true,
    navigator: { gpu: { async requestAdapter() { return adapter; } } },
  });
  assert.equal(result.ok, true);
  assert.equal(result.mode, "core");
  assert.equal(adapter.destroyed, true);
  assert.deepEqual(adapter.lastDescriptor.requiredFeatures, ["core-features-and-limits"]);
});

test("extractReleaseId and credentialsFor enforce Stage 1 versioned namespace and same-origin cookies", () => {
  const id = "a".repeat(64);
  assert.equal(extractReleaseId(`/releases/${id}/index.html`), id);
  assert.equal(extractReleaseId("/releases/short/index.html"), null);
  assert.equal(extractReleaseId("/index.html"), null);

  assert.equal(credentialsFor("https://play.example/releases/x/assets/", "https://play.example"), "same-origin");
  assert.equal(credentialsFor("https://cdn.example/assets/", "https://play.example"), "omit");
});
