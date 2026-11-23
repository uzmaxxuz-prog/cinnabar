/** Opt-in observations only: never changes requested GPU features, limits or draw arguments. */
export function installTerrainDiagnostics(env = globalThis) {
  const undo = [], buffers = Object.create(null), draws = Object.create(null), messages = [];
  const pipelines = new WeakMap(), active = new WeakMap();
  const started = env.performance.now();
  let deviceInfo = null, restored = false;
  const scrub = value => String(value).replace(/https?:\/\/[^\s)]+/gi, "<url>")
    .replace(/\bBearer\s+[^\s,;"']+/gi, "Bearer <redacted>")
    .replace(/\b(password|token|cookie|authorization|session[_-]?id|secret)["']?\s*[:=]\s*(?:"[^"]*"|'[^']*'|[^\s,;]+)/gi, "$1=<redacted>")
    .replaceAll("%c", "").slice(0, 600);
  const note = (kind, value) => {
    if (restored) return;
    const text = scrub(value), last = messages.at(-1);
    if (last?.kind === kind && last.text === text) { last.count++; return; }
    if (messages.length === 32) messages.shift();
    messages.push({ kind, text, count: 1 });
  };
  const safe = fn => { try { fn(); } catch {} };
  const wrap = (target, name, observe) => {
    if (typeof target?.[name] !== "function") return;
    const original = target[name];
    const replacement = function (...args) {
      const result = original.apply(this, args);
      safe(() => observe(this, args, result));
      return result;
    };
    target[name] = replacement;
    undo.push(() => { if (target[name] === replacement) target[name] = original; });
  };
  const boundedKey = (table, key) => Object.hasOwn(table, key) || Object.keys(table).length < 48;
  const label = value => scrub(value || "unlabelled").slice(0, 100);
  const recordDevice = device => {
    safe(() => {
      deviceInfo = {
        core: device.features.has("core-features-and-limits"),
        indirect_first_instance: device.features.has("indirect-first-instance"),
        limits: Object.fromEntries(["maxTextureArrayLayers", "maxBufferSize", "maxStorageBufferBindingSize"].map(key => [key, device.limits[key]])),
      };
      const error = event => note("gpu", event.error?.message || "GPU validation error");
      device.addEventListener("uncapturederror", error);
      undo.push(() => device.removeEventListener("uncapturederror", error));
      device.lost.then(info => { if (info.reason !== "destroyed") note("device-lost", info.message); }, () => {});
    });
  };
  wrap(env.GPUAdapter?.prototype, "requestDevice", (_self, _args, promise) => {
    promise.then(recordDevice, error => note("device-request", error?.message || error));
  });
  wrap(env.GPUDevice?.prototype, "createBuffer", (_self, [descriptor]) => {
    if (!/chunk|terrain/i.test(descriptor.label || "")) return;
    const key = label(descriptor.label);
    if (boundedKey(buffers, key)) buffers[key] = { bytes: descriptor.size, writes: buffers[key]?.writes || 0 };
  });
  wrap(env.GPUQueue?.prototype, "writeBuffer", (_self, [buffer]) => {
    const key = label(buffer.label);
    if (buffers[key]) buffers[key].writes++;
  });
  wrap(env.GPUDevice?.prototype, "createRenderPipeline", (_self, [descriptor], pipeline) => pipelines.set(pipeline, label(descriptor.label)));
  wrap(env.GPUDevice?.prototype, "createRenderPipelineAsync", (_self, [descriptor], promise) => {
    promise.then(pipeline => pipelines.set(pipeline, label(descriptor.label)), error => note("pipeline", error?.message || error));
  });
  for (const type of [env.GPURenderPassEncoder, env.GPURenderBundleEncoder]) {
    wrap(type?.prototype, "setPipeline", (pass, [pipeline]) => active.set(pass, pipelines.get(pipeline) || label(pipeline.label)));
    for (const name of ["draw", "drawIndexed", "drawIndirect", "drawIndexedIndirect"]) {
      wrap(type?.prototype, name, pass => {
        const key = `${active.get(pass) || "unlabelled"}:${name}`;
        if (boundedKey(draws, key)) draws[key] = (draws[key] || 0) + 1;
      });
    }
  }
  for (const kind of ["error", "warn", "info", "log"]) {
    wrap(env.console, kind, (_self, args) => {
      const text = args.filter(value => typeof value === "string").join(" ").replaceAll("%c", "");
      if (kind === "error" || /\b(?:ERROR|WARN)\b|terrain|chunk|pipeline|GPU|renderer/i.test(text)) note(kind, text);
    });
  }
  for (const name of ["error", "unhandledrejection"]) {
    const listener = event => note(name, event.message || event.reason?.message || String(event.reason));
    env.addEventListener(name, listener);
    undo.push(() => env.removeEventListener(name, listener));
  }

  async function canvasHistogram() {
    const canvas = env.document?.getElementById("game");
    if (!canvas) return null;
    try {
      // Prefer the standard canvas snapshot path; WebGPU canvases support toDataURL in Chrome,
      // and this avoids swapchain/present-layout timing issues with copyTextureToTexture.
      const dataUrl = canvas.toDataURL("image/png");
      const blob = await (await fetch(dataUrl)).blob();
      const bitmap = await env.createImageBitmap(blob);
      const width = Math.min(bitmap.width, 64);
      const height = Math.min(bitmap.height, 64);
      const scratch = new env.OffscreenCanvas(width, height);
      const ctx = scratch.getContext("2d");
      if (!ctx) return "offscreen-2d-unavailable";
      ctx.drawImage(bitmap, 0, 0);
      const imageData = ctx.getImageData(0, 0, width, height);
      const summary = summarizeHistogram(imageData.data, width, height);
      bitmap.close();
      scratch.width = 0;
      return summary;
    } catch (err) {
      return String(err?.message || err);
    }
  }

  function summarizeHistogram(data, width, height) {
    if (!data || data.length === 0) return null;
    const buckets = { dark: 0, green: 0, blue: 0, bright: 0, other: 0 };
    for (let y = 0; y < height; ++y) {
      const row = y * width * 4;
      for (let x = 0; x < width; ++x) {
        const i = row + x * 4;
        const r = data[i], g = data[i + 1], b = data[i + 2];
        if (g > 60 && g > r * 1.2 && g > b * 1.2) buckets.green++;
        else if (b > 60 && b > r * 1.2 && b > g * 2.0) buckets.blue++;
        else if (r + g + b < 60) buckets.dark++;
        else if (r + g + b > 500) buckets.bright++;
        else buckets.other++;
      }
    }
    const total = width * height;
    return {
      sample_px: { width, height },
      total_pixels: total,
      buckets,
      dominant: Object.entries(buckets).sort((a, b) => b[1] - a[1])[0][0],
    };
  }

  return {
    snapshot() {
      const canvas = env.document?.getElementById("game");
      return JSON.parse(JSON.stringify({
        version: 1, release: env.location.pathname.match(/\/releases\/([0-9a-f]{64})/)?.[1] || null,
        elapsed_seconds: Math.round((env.performance.now() - started) / 1000),
        browser: scrub(env.navigator.userAgent), device: deviceInfo,
        status: scrub(env.document?.getElementById("status")?.textContent || ""),
        canvas: canvas ? { width: canvas.width, height: canvas.height, css_width: canvas.clientWidth, css_height: canvas.clientHeight, dpr: env.devicePixelRatio } : null,
        histogram: null,
        buffers, draws, messages,
      }));
    },
    async fullReport() {
      const [base, histogram] = await Promise.all([this.snapshot(), canvasHistogram()]);
      base.histogram = histogram;
      return base;
    },
    restore() { restored = true; undo.reverse().forEach(remove => remove()); },
  };
}

/** Adds a local, manual report button only for a diagnostic link; no report is transmitted. */
export function mountTerrainDiagnostics(env = globalThis) {
  if (new URLSearchParams(env.location.search).get("diagnostics") !== "terrain") return;
  const diagnostic = installTerrainDiagnostics(env), doc = env.document;
  const panel = doc.createElement("details"), title = doc.createElement("summary");
  title.textContent = "Диагностика местности";
  const button = doc.createElement("button"), output = doc.createElement("textarea"), result = doc.createElement("p");
  button.textContent = "Скопировать отчёт";
  output.readOnly = true; output.rows = 12; output.style.width = "100%";
  output.setAttribute("aria-label", "Диагностический отчёт");
  button.addEventListener("click", async () => {
    output.value = JSON.stringify(await diagnostic.fullReport());
    try {
      await env.navigator.clipboard.writeText(output.value);
      result.textContent = "Отчёт скопирован. Вставьте его в чат.";
    } catch {
      output.focus(); output.select();
      result.textContent = "Скопируйте выделенный отчёт и вставьте его в чат.";
    }
  });
  panel.append(title, button, result, output);
  doc.getElementById("game").before(panel);
  panel.open = true;
  return diagnostic;
}
