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
    setViewport(...args) { this.viewport = args; }
    setScissorRect(...args) { this.scissor = args; }
  }
  class Encoder {
    copyBufferToBuffer(...args) { this.args = args; return 13; }
    beginRenderPass(descriptor) { this.descriptor = descriptor; return new Pass(); }
  }
  return {
    GPUDevice: Device, GPUQueue: Queue, GPURenderPassEncoder: Pass,
    GPUCommandEncoder: Encoder,
    console: { error() {}, warn() {}, info() {}, log() {} },
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

test("Rust tracing errors through console.log reach the report without unrelated logs", () => {
  const env = fixture(), diagnostic = installTerrainDiagnostics(env);
  env.console.log("ordinary account message");
  env.console.log("%cERROR%c shader creation failed", "color:red", "color:inherit");
  const report = diagnostic.snapshot();
  assert.ok(report.messages.some(message => message.text.includes("shader creation failed")));
  assert.ok(!JSON.stringify(report).includes("ordinary account message"));
  diagnostic.restore();
});

test("report distinguishes copy calls and actual draw arguments without changing them", () => {
  const env = fixture(), diagnostic = installTerrainDiagnostics(env);
  const device = new env.GPUDevice(), encoder = new env.GPUCommandEncoder();
  const source = { label: "staging" }, target = device.createBuffer({ label: "packed chunk quads", size: 4096 });
  assert.equal(encoder.copyBufferToBuffer(source, 8, target, 16, 64), 13);
  assert.deepEqual(encoder.args, [source, 8, target, 16, 64]);
  const pass = encoder.beginRenderPass({ label: "terrain pass", depthStencilAttachment: { depthLoadOp: "clear", depthClearValue: 0 } });
  pass.setPipeline(device.createRenderPipeline({ label: "terrain opaque", depthStencil: { depthCompare: "greater-equal" } }));
  pass.setViewport(0, 0, 320, 200, 0, 1);
  pass.setScissorRect(0, 0, 320, 200);
  assert.equal(pass.drawIndexed(384, 3, 0, 0, 0), 17);
  const report = diagnostic.snapshot();
  assert.deepEqual(report.copies["packed chunk quads"], { calls: 1, bytes: 64 });
  const draw = report.last_draws["terrain opaque:drawIndexed"];
  assert.deepEqual(draw.args, [384, 3, 0, 0, 0]);
  assert.deepEqual(draw.viewport, [0, 0, 320, 200, 0, 1]);
  assert.equal(draw.pass.depth_clear, 0);
  assert.equal(draw.pipeline.depth_compare, "greater-equal");
  diagnostic.restore();
});
