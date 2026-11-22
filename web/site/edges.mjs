/** Samples the browser clock for relay capability expiry. */
export const milliseconds = () => performance.now();
/** Owns a timeout registration until the driver drops or completes its wait. */
export function startTimer(ms) {
  const handle = { id: undefined, promise: undefined, settle: undefined };
  handle.promise = new Promise((resolve) => {
    /** Releases the resolver after either expiry or cancellation completes the wait. */
    handle.settle = () => {
      handle.id = undefined;
      handle.settle = undefined;
      resolve();
    };
    handle.id = setTimeout(handle.settle, ms);
  });
  return handle;
}
/** Returns the existing promise without registering another timeout on each poll. */
export const timerPromise = (handle) => handle.promise;
/** Cancels a suspended, superseded or disposed driver's pending registration. */
export const cancelTimer = (handle) => {
  if (handle.id !== undefined) clearTimeout(handle.id);
  handle.settle?.();
};

/** Reads an exact, bounded response; cancels surplus bytes before retaining them. */
export async function boundedBytes(response, maximum, exact = true) {
  if (!response.ok || !response.body) throw new Error("Asset request failed");
  const reader = response.body.getReader();
  const bytes = new Uint8Array(maximum);
  let used = 0;
  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      if (value.length > maximum - used) throw new Error("Asset exceeds delivery size");
      bytes.set(value, used); used += value.length;
    }
    if (exact && used !== maximum) throw new Error("Asset is truncated");
    return bytes.subarray(0, used);
  } finally { await reader.cancel(); }
}

/** Keeps required delivery failures fatal while unavailable optional carriers use startup fallbacks. */
export async function downloadCarriers(carriers, manifest, base, fetcher = fetch, credentials = "omit") {
  for (const [name, path, required] of JSON.parse(carriers.entries())) {
    try {
      const entry = manifest.entries.find((entry) => entry.name === name);
      const response = await fetcher(new URL(path, base), { credentials, redirect: "error" });
      carriers.insert(name, await boundedBytes(response, entry.bytes));
    } catch (error) {
      if (required !== false) throw error;
    }
  }
}

/** Suspends cached pages and tears down resources only when navigation is final. */
export function pageLifecycle(input, audio, page, dispose, view = window) {
  let cached = false, disposed = false;
  /** Keeps wasm objects and gesture listeners alive while the page is cached. */
  const hide = (event) => {
    if (disposed) return;
    input.release();
    if (event.persisted) {
      cached = true;
      page()?.suspend();
      audio.suspend().catch(() => {});
    } else {
      disposed = true;
      view.removeEventListener("pagehide", hide);
      view.removeEventListener("pageshow", show);
      dispose();
    }
  };
  /** Reopens relay control; the next canvas gesture unlocks audio again. */
  const show = () => {
    if (cached && !disposed) {
      cached = false;
      page()?.resume();
    }
  };
  view.addEventListener("pagehide", hide);
  view.addEventListener("pageshow", show);
}

/** Requests fullscreen on the enabled button's own activation, retaining the browser gesture. */
export function bindFullscreen(button, canvas, onError) {
  const enter = async () => {
    if (button.disabled) return;
    try {
      await canvas.requestFullscreen();
    } catch (error) {
      onError(error);
    }
  };
  button.addEventListener("click", enter);
  return () => button.removeEventListener("click", enter);
}

/** Installs canvas gesture, focus and size edges, returning an explicit teardown. */
export function canvasEdges(canvas, input, audio, view = window, doc = document) {
  const listeners = [];
  /** Registers a listener owned by this canvas session. */
  const listen = (target, name, callback, capture = false) => {
    target.addEventListener(name, callback, capture);
    listeners.push(() => target.removeEventListener(name, callback, capture));
  };
  /** Reports whether this canvas owns pointer lock. */
  const locked = () => doc.pointerLockElement === canvas;
  let measured;
  /** A held touch owns geometry from its starting viewport, even if Winit rewrites backing pixels. */
  const resize = () => {
    const dpr = view.devicePixelRatio || 1;
    const cssWidth = canvas.clientWidth, cssHeight = canvas.clientHeight;
    if (!measured || measured.cssWidth !== cssWidth || measured.cssHeight !== cssHeight || measured.dpr !== dpr) {
      if (measured) input.touch_geometry_changed?.();
      measured = { cssWidth, cssHeight, dpr };
    }
    const width = Math.max(1, Math.round(cssWidth * dpr));
    const height = Math.max(1, Math.round(cssHeight * dpr));
    if (canvas.width !== width) canvas.width = width;
    if (canvas.height !== height) canvas.height = height;
  };
  const primaryTouch = view.navigator?.maxTouchPoints > 0 && Boolean(view.matchMedia?.("(pointer: coarse)")?.matches);
  input.set_touch_capable?.(primaryTouch);
  let lastPointerType;
  listen(canvas, "pointerdown", (event) => {
    lastPointerType = event.pointerType;
    if (event.pointerType !== "touch") return;
    event.preventDefault();
    input.set_touch_capable(true);
    if (locked()) return;
    canvas.focus({ preventScroll: true });
    input.begin_touch();
    audio.resume().catch(() => {});
  }, true);
  listen(canvas, "click", (event) => {
    // Older browsers deliver a compatibility MouseEvent without pointerType.
    if ((event.pointerType || lastPointerType) === "touch" || event.sourceCapabilities?.firesTouchEvents) return;
    audio.resume().catch(() => {});
    canvas.requestPointerLock()?.catch(() => {});
  });
  listen(doc, "pointerlockchange", () => {
    if (input.focus) input.focus(locked());
    else if (!locked()) input.release();
  });
  listen(doc, "visibilitychange", () => {
    if (doc.visibilityState === "hidden") input.release();
  });
  listen(view, "blur", () => input.release());
  for (const [name, down] of [["keydown", true], ["keyup", false]]) {
    listen(doc, name, (event) => { if (locked()) { event.preventDefault(); input.key(event.code, down); } });
  }
  for (const [name, down] of [["mousedown", true], ["mouseup", false]]) {
    listen(canvas, name, (event) => { if (locked()) input.mouse(event.button, down); });
  }
  listen(canvas, "mousemove", (event) => { if (locked()) input.motion(event.movementX, event.movementY); });
  listen(canvas, "contextmenu", (event) => event.preventDefault());
  listen(view, "resize", resize);
  const observer = new ResizeObserver(resize);
  observer.observe(canvas); resize();
  let stopped = false;
  /** Tracks DPR changes without resetting an unchanged drawing surface. */
  const frame = () => { if (!stopped) { resize(); requestAnimationFrame(frame); } };
  requestAnimationFrame(frame);
  return () => { stopped = true; listeners.forEach((remove) => remove()); observer.disconnect(); input.release(); };
}
