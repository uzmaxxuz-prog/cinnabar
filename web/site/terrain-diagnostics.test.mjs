import { test } from "node:test";
import assert from "node:assert/strict";
import { installTerrainDiagnostics, mountTerrainDiagnostics } from "./terrain-diagnostics.mjs";

function fixture() {
  class Device extends EventTarget {
    createBuffer(descriptor) { return { ...descriptor }; }
    createRenderPipeline(descriptor) { return { ...descriptor }; }
  }
  class Queue { writeBuffer(...args) { this.args = args; return 42; } }
  class Pass {
    setPipeline(pipeline) { this.pipeline = pipeline; }
    drawIndexed(...args) { this.args = args; return 17; }
  }
  return {
    GPUDevice: Device, GPUQueue: Queue, GPURenderPassEncoder: Pass,
    console: { error() {}, warn() {}, info() {} },
    navigator: { userAgent: "test" }, location: { pathname: "/releases/test/index.html", search: "" },
    performance: { now: () => 100 }, addEventListener() {}, removeEventListener() {},
  };
}

test("ordinary page installs no diagnostic hooks", () => {
  const env = fixture(), original = env.GPUDevice.prototype.createBuffer;
  assert.equal(mountTerrainDiagnostics(env), undefined);
  assert.equal(env.GPUDevice.prototype.createBuffer, original);
});

test("terrain observations preserve GPU call arguments, returns and restoration", () => {
  const env = fixture(), original = env.GPUDevice.prototype.createBuffer;
  const diagnostic = installTerrainDiagnostics(env);
  const device = new env.GPUDevice(), queue = new env.GPUQueue(), pass = new env.GPURenderPassEncoder();
  const buffer = device.createBuffer({ label: "packed chunk quads", size: 2048 });
  const bytes = new Uint8Array([1, 2, 3]);
  assert.equal(queue.writeBuffer(buffer, 0, bytes), 42);
  assert.equal(queue.args[2], bytes);
  pass.setPipeline(device.createRenderPipeline({ label: "terrain opaque" }));
  assert.equal(pass.drawIndexed(36, 2), 17);
  assert.deepEqual(pass.args, [36, 2]);
  const report = diagnostic.snapshot();
  assert.equal(report.buffers["packed chunk quads"].bytes, 2048);
  assert.equal(report.buffers["packed chunk quads"].writes, 1);
  assert.equal(report.draws["terrain opaque:drawIndexed"], 1);
  diagnostic.restore();
  assert.equal(env.GPUDevice.prototype.createBuffer, original);
});

test("reports bound log volume and redact URLs and credentials", () => {
  const env = fixture(), diagnostic = installTerrainDiagnostics(env);
  for (let n = 0; n < 200; n++) env.console.error("GPU failure https://host/?token=private password=secret Bearer abcdef");
  const report = diagnostic.snapshot(), text = JSON.stringify(report);
  assert.ok(report.messages.length <= 32);
  for (const secret of ["host", "private", "secret", "abcdef"]) assert.ok(!text.includes(secret));
  diagnostic.restore();
});

test("diagnostics preserve original GPU exceptions", () => {
  const env = fixture(), expected = new Error("original failure");
  env.GPUDevice.prototype.createBuffer = () => { throw expected; };
  const diagnostic = installTerrainDiagnostics(env);
  assert.throws(() => new env.GPUDevice().createBuffer({}), error => error === expected);
  diagnostic.restore();
});
