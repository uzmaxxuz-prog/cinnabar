import { test } from "node:test";
import assert from "node:assert/strict";
import { canvasEdges, pageLifecycle } from "./edges.mjs";

function fixture(t, primaryTouch = true) {
  const saved = [globalThis.ResizeObserver, globalThis.requestAnimationFrame];
  globalThis.ResizeObserver = class { observe() {} disconnect() {} };
  globalThis.requestAnimationFrame = () => 1;
  const state = { capable: undefined, touch: false, mouse: false, releases: 0, locks: 0 };
  const input = {
    set_touch_capable(value) { state.capable = value; },
    begin_touch() { state.touch = true; },
    focus(value) { state.mouse = value; },
    release() { state.touch = false; state.mouse = false; state.releases++; },
    key() {}, mouse() {}, motion() {},
  };
  const view = Object.assign(new EventTarget(), {
    devicePixelRatio: 2,
    navigator: { maxTouchPoints: primaryTouch ? 5 : 0 },
    matchMedia: () => ({ matches: primaryTouch }),
  });
  const doc = Object.assign(new EventTarget(), { visibilityState: "visible", pointerLockElement: null });
  const canvas = Object.assign(new EventTarget(), {
    clientWidth: 320, clientHeight: 240, style: {}, focus() {},
    requestPointerLock() { state.locks++; return Promise.resolve(); },
  });
  const audio = { resume: async () => {}, suspend: async () => {} };
  const stop = canvasEdges(canvas, input, audio, view, doc);
  t.after(() => { stop(); [globalThis.ResizeObserver, globalThis.requestAnimationFrame] = saved; });
  const pointer = (name, type, id = 1) => {
    const event = Object.assign(new Event(name, { cancelable: true }), { pointerType: type, pointerId: id });
    canvas.dispatchEvent(event);
    return event;
  };
  return { state, input, view, doc, canvas, audio, pointer };
}

test("primary touch capability is reported before the game starts", t => {
  const f = fixture(t);
  assert.equal(f.state.capable, true);
});

test("touch owns the canvas without requesting mouse lock", t => {
  const f = fixture(t);
  const down = f.pointer("pointerdown", "touch");
  assert.equal(f.state.touch, true);
  assert.equal(down.defaultPrevented, true);
  f.pointer("click", "touch");
  assert.equal(f.state.locks, 0);
});

test("a real touch enables controls on a fine-pointer hybrid device", t => {
  const f = fixture(t, false);
  f.pointer("pointerdown", "touch");
  assert.equal(f.state.capable, true);
  assert.equal(f.state.touch, true);
});

test("mouse-lock loss does not cancel the touch that took ownership", t => {
  const f = fixture(t);
  f.pointer("pointerdown", "touch");
  f.doc.dispatchEvent(new Event("pointerlockchange"));
  assert.equal(f.state.touch, true);
  assert.equal(f.state.releases, 0);
});

test("hiding the document releases touch ownership", t => {
  const f = fixture(t);
  f.pointer("pointerdown", "touch");
  f.doc.visibilityState = "hidden";
  f.doc.dispatchEvent(new Event("visibilitychange"));
  assert.equal(f.state.touch, false);
  assert.equal(f.state.releases, 1);
});

test("cancelled contact remains in the native stream without releasing another finger", t => {
  const f = fixture(t);
  const native = [];
  f.canvas.addEventListener("pointercancel", e => native.push(e.pointerId));
  f.pointer("pointerdown", "touch", 1);
  f.pointer("pointerdown", "touch", 2);
  f.pointer("pointercancel", "touch", 2);
  assert.deepEqual(native, [2]);
  assert.equal(f.state.touch, true);
  assert.equal(f.state.releases, 0);
});

test("desktop mouse keeps its existing pointer-lock gesture", t => {
  const f = fixture(t, false);
  f.pointer("pointerdown", "mouse");
  f.pointer("click", "mouse");
  assert.equal(f.state.locks, 1);
  f.doc.pointerLockElement = f.canvas;
  f.doc.dispatchEvent(new Event("pointerlockchange"));
  assert.equal(f.state.mouse, true);
});

test("cached pagehide releases touches and preserves the one game owner", t => {
  const f = fixture(t);
  let disposed = 0;
  pageLifecycle(f.input, f.audio, () => null, () => disposed++, f.view);
  f.pointer("pointerdown", "touch");
  f.view.dispatchEvent(Object.assign(new Event("pagehide"), { persisted: true }));
  assert.equal(f.state.touch, false);
  assert.equal(disposed, 0);
});
