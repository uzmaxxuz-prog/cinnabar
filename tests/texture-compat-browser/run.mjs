import { spawn, execFileSync } from "node:child_process";
import { createServer } from "node:http";
import { readFile, writeFile, mkdir, readdir, realpath, mkdtemp, rm } from "node:fs/promises";
import { extname, resolve, sep, join } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import { tmpdir } from "node:os";
import { createHash } from "node:crypto";
import { setTimeout as delay } from "node:timers/promises";

const TYPES = { ".html": "text/html; charset=utf-8", ".mjs": "text/javascript; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".wasm": "application/wasm", ".wgsl": "text/plain; charset=utf-8", ".json": "application/json; charset=utf-8" };
export async function serveDirectory(site) {
  const root = await realpath(site);
  const server = createServer(async (request, response) => {
    try {
      const pathname = decodeURIComponent(new URL(request.url, "http://127.0.0.1").pathname);
      const path = await realpath(resolve(root, "." + pathname));
      const type = TYPES[extname(path)];
      if (!path.startsWith(root + sep) || !type) throw new Error("not a served fixture");
      const body = await readFile(path);
      response.writeHead(200, { "content-type": type, "cache-control": "no-store", "Cross-Origin-Opener-Policy": "same-origin", "Cross-Origin-Embedder-Policy": "require-corp" }).end(body);
    } catch {
      response.writeHead(404).end();
    }
  });
  await new Promise((done, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", done); });
  return { origin: "http://127.0.0.1:" + server.address().port, close: () => new Promise((done) => { server.closeAllConnections(); server.close(done); }) };
}
async function connect(url, events) {
  const socket = new WebSocket(url);
  await new Promise((done, reject) => { socket.addEventListener("open", done, { once: true }); socket.addEventListener("error", reject, { once: true }); });
  let next = 0;
  const pending = new Map();
  socket.addEventListener("message", ({ data }) => {
    const message = JSON.parse(data);
    if (message.id) {
      const item = pending.get(message.id);
      if (!item) return;
      clearTimeout(item.timer);
      pending.delete(message.id);
      message.error ? item.reject(new Error(JSON.stringify(message.error))) : item.resolve(message.result);
    } else if (message.method === "Runtime.exceptionThrown" || message.method === "Runtime.consoleAPICalled") {
      events.push(message);
    }
  });
  return {
    send(method, params = {}) {
      return new Promise((resolve, reject) => {
        const id = ++next;
        const timer = setTimeout(() => { pending.delete(id); reject(new Error("CDP timeout: " + method)); }, 30000);
        pending.set(id, { resolve, reject, timer });
        socket.send(JSON.stringify({ id, method, params }));
      });
    },
    close() { for (const item of pending.values()) { clearTimeout(item.timer); item.reject(new Error("CDP closed")); } pending.clear(); socket.close(); },
  };
}
async function evaluate(session, expression) {
  const reply = await session.send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  if (reply.exceptionDetails) throw new Error(reply.exceptionDetails.exception?.description ?? reply.exceptionDetails.text);
  return reply.result?.value;
}
async function digestDirectory(site) {
  const files = {};
  async function walk(directory) {
    for (const entry of (await readdir(directory, { withFileTypes: true })).sort((a, b) => a.name.localeCompare(b.name))) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) await walk(path);
      else if (entry.isFile()) files[path.slice(site.length + 1)] = createHash("sha256").update(await readFile(path)).digest("hex");
      else throw new Error("fixture symlink/special file refused: " + path);
    }
  }
  await walk(site);
  return files;
}
export function comparePair(records) {
  if (records.length !== 2 || records.some((record, index) => record.mode !== ["core", "compat"][index])) return "missing paired modes";
  if (records.some((record) => record.verdict !== "PASS")) return "one or more browser modes failed";
  for (const record of records) {
    if (!record.tests?.length || record.tests.some(({ ok }) => ok !== true)) return "invalid test results";
    if (!record.devices?.length || record.devices.some(({ core }) => core !== (record.mode === "core"))) return "actual device mode not established";
    if (record.suite !== "textures" && (record.shaderCoverage?.pipelineOnly !== false || !record.shaderCoverage.semanticCases?.length)) return "shader pixel coverage missing";
    if (record.uncaptured?.length || record.deviceLost?.length || record.unhandledRejections?.length) return "uncaptured device failure";
  }
  if (records[0].userAgent !== records[1].userAgent) return "browser changed";
  if (JSON.stringify(records[0].provenance) !== JSON.stringify(records[1].provenance)) return "source provenance changed";
  const corePixels = records[0].textureFixture?.pixelComparisons;
  const compatPixels = records[1].textureFixture?.pixelComparisons;
  if (records[0].suite !== "shaders") {
    if (!corePixels?.length || !compatPixels?.length) return "paired readback evidence missing";
    if (JSON.stringify(corePixels.map(({ id, actual }) => ({ id, actual }))) !== JSON.stringify(compatPixels.map(({ id, actual }) => ({ id, actual })))) return "core/compat readback differs";
  }
  const semanticPixels = records.map((record) => record.tests.filter((test) => test.semantic).map(({ semantic }) => ({ id: semantic.id, pixels: semantic.pixels.map(({ x, y, actual }) => ({ x, y, actual })) })));
  if (JSON.stringify(semanticPixels[0]) !== JSON.stringify(semanticPixels[1])) return "production shader semantic pixels differ";
  return null;
}
async function main() {
  const [siteArgument, chrome, outputArgument, suite = "all"] = process.argv.slice(2);
  if (!siteArgument || !chrome || !outputArgument || !["all", "textures", "shaders"].includes(suite)) throw new Error("usage: node run.mjs <fixture-dir> <chrome> <output-dir> [all|textures|shaders]; T55_CHROME_ARGS is a JSON string array");
  const site = await realpath(siteArgument);
  const output = resolve(outputArgument);
  if (output === site || output.startsWith(site + sep)) throw new Error("output must be outside immutable fixture directory");
  await mkdir(output, { recursive: true });
  const provenance = JSON.parse(await readFile(join(site, "provenance.json"), "utf8"));
  for (const name of ["cinnabar", "wgpu", "bevy"]) if (!/^[0-9a-f]{40}$/.test(provenance[name] ?? "")) throw new Error("missing exact source SHA: " + name);
  const harnessDirectory = fileURLToPath(new URL(".", import.meta.url));
  const repository = execFileSync("git", ["rev-parse", "--show-toplevel"], { cwd: harnessDirectory, encoding: "utf8" }).trim();
  const harnessSha = execFileSync("git", ["rev-parse", "HEAD"], { cwd: harnessDirectory, encoding: "utf8" }).trim();
  const dirty = execFileSync("git", ["status", "--porcelain", "--", "tests/texture-compat-browser/index.html", "tests/texture-compat-browser/probe.mjs", "tests/texture-compat-browser/run.mjs"], { cwd: repository, encoding: "utf8" }).trim();
  if (dirty) throw new Error("runner must be delivered from a committed Git SHA");
  for (const name of ["index.html", "probe.mjs"]) {
    if (!(await readFile(join(site, name))).equals(await readFile(join(harnessDirectory, name)))) throw new Error("fixture harness differs from committed runner: " + name);
  }
  const inputs = await digestDirectory(site);
  const buildManifest = JSON.parse(await readFile(join(site, "build.json"), "utf8"));
  for (const name of ["cinnabar", "wgpu", "bevy"]) if (buildManifest.sources?.[name] !== provenance[name]) throw new Error("build/source SHA mismatch: " + name);
  if (!/^[0-9a-f]{64}$/.test(buildManifest.lockSha256 ?? "") || buildManifest.lockSha256 !== provenance.lockSha256) throw new Error("build/Cargo.lock hash mismatch");
  if (!buildManifest.tools?.rustc?.startsWith("rustc 1.99.0 ") || buildManifest.tools?.wasmBindgen !== "wasm-bindgen 0.2.127" || !buildManifest.commands?.length) throw new Error("missing source-locked build command/toolchain evidence");
  for (const [path, hash] of Object.entries(inputs)) {
    if (([".wasm", ".wgsl"].includes(extname(path)) || (path.startsWith("pkg/") && [".js", ".mjs"].includes(extname(path)))) && buildManifest.artifacts?.[path] !== hash) throw new Error("build/artifact digest mismatch: " + path);
  }
  const args = JSON.parse(process.env.T55_CHROME_ARGS ?? "[]");
  if (!Array.isArray(args) || args.some((value) => typeof value !== "string")) throw new Error("T55_CHROME_ARGS must be string array");
  const profile = await mkdtemp(join(tmpdir(), "t5355-browser-"));
  const server = await serveDirectory(site);
  const child = spawn(chrome, ["--headless=new", "--no-sandbox", "--no-first-run", "--remote-debugging-port=0", "--remote-debugging-address=127.0.0.1", "--user-data-dir=" + profile, ...args, "about:blank"], { stdio: ["ignore", "ignore", "pipe"] });
  let stderr = "";
  let spawnError;
  child.once("error", (error) => { spawnError = error; });
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  const events = [];
  const records = [];
  let session;
  const result = { verdict: "FAIL", diagnosticOnly: true, acceptance: "real POCO/Windows and full-client gates are not established by this fixture runner", harnessSha, provenance, buildManifest, attribution: "recorded build sources/lock/toolchains reconciled with artifact hashes; execution log remains required", inputs, browserArgs: args, records, events };
  try {
    let port;
    for (let attempt = 0; attempt < 200 && !port; attempt++) {
      try { port = (await readFile(join(profile, "DevToolsActivePort"), "utf8")).split("\n")[0]; } catch { await delay(100); }
      if (spawnError) throw spawnError;
      if (child.exitCode !== null) throw new Error("Chrome exited: " + child.exitCode);
    }
    if (!port) throw new Error("Chrome did not expose a debugger");
    const targets = await (await fetch("http://127.0.0.1:" + port + "/json/list")).json();
    session = await connect(targets.find(({ type }) => type === "page").webSocketDebuggerUrl, events);
    await session.send("Runtime.enable");
    await session.send("Page.enable");
    result.browser = await session.send("Browser.getVersion");
    for (const mode of ["core", "compat"]) {
      await session.send("Page.navigate", { url: server.origin + "/index.html?mode=" + mode + "&suite=" + suite });
      let record;
      for (let waited = 0; waited < 180000 && !record; waited += 100) {
        try { record = await evaluate(session, "document.body?.dataset.verdict ? JSON.parse(document.getElementById('record').textContent) : null"); } catch (error) { if (!String(error).includes("context")) throw error; }
        if (record?.mode !== mode) record = null;
        if (!record) await delay(100);
      }
      if (!record) throw new Error(mode + " fixture timeout");
      if (record.mode !== mode) throw new Error("stale page mode: " + record.mode);
      records.push(record);
      await writeFile(join(output, mode + ".json"), JSON.stringify(record, null, 2));
      const screenshot = await session.send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
      await writeFile(join(output, mode + ".png"), Buffer.from(screenshot.data, "base64"));
    }
    result.problem = comparePair(records);
    if (events.some(({ method }) => method === "Runtime.exceptionThrown")) result.problem = "unexpected uncaught browser exception";
    if (records.some((record) => JSON.stringify(record.provenance) !== JSON.stringify(provenance))) result.problem = "page provenance does not match immutable source metadata";
    if (JSON.stringify(inputs) !== JSON.stringify(await digestDirectory(site))) result.problem = "fixture inputs changed during paired run";
    result.verdict = result.problem ? "FAIL" : "PASS";
  } catch (error) {
    result.problem = String(error?.stack ?? error);
  } finally {
    session?.close();
    child.kill("SIGTERM");
    await Promise.race([new Promise((done) => child.once("exit", done)), delay(3000)]);
    if (child.exitCode === null) child.kill("SIGKILL");
    await server.close();
    await writeFile(join(output, "chrome.log"), stderr);
    await writeFile(join(output, "paired.json"), JSON.stringify(result, null, 2));
    await rm(profile, { recursive: true, force: true });
  }
  console.log(JSON.stringify({ verdict: result.verdict, diagnosticOnly: result.diagnosticOnly, problem: result.problem, output }));
  if (result.verdict !== "PASS") process.exitCode = 1;
}
if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) main().catch((error) => { console.error(String(error?.stack ?? error)); process.exitCode = 1; });
