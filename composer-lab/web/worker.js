/* Web Worker running Griff Studio Python generator via Pyodide. */
const PYODIDE_VERSION = "v0.27.7";
const PYODIDE_BASE = "https://cdn.jsdelivr.net/pyodide/v0.27.7/full/";
const PY_BASE = "./py/";
const PY_FILES = [
  "composer.py",
  "fretboard.py",
  "midiio.py",
  "musicxml.py",
  "studio/engine.py",
  "web_api.py",
];

let pyodide = null;
let handleJson = null;

async function init() {
  const t0 = performance.now();
  self.postMessage({ type: "progress", text: `Загрузка Python (Pyodide ${PYODIDE_VERSION})...` });
  importScripts(PYODIDE_BASE + "pyodide.js");
  pyodide = await loadPyodide({ indexURL: PYODIDE_BASE });

  self.postMessage({ type: "progress", text: "Загрузка модулей генератора..." });
  const fetched = await Promise.all(
    PY_FILES.map(async (rel) => {
      const res = await fetch(PY_BASE + rel);
      if (!res.ok) {
        throw new Error(`Failed to fetch ${PY_BASE + rel}: ${res.status}`);
      }
      return [rel, await res.text()];
    })
  );

  pyodide.FS.mkdirTree("/app/studio");
  pyodide.FS.writeFile("/app/studio/__init__.py", "");
  for (const [rel, src] of fetched) {
    pyodide.FS.writeFile("/app/" + rel, src);
  }

  pyodide.runPython(
    "import sys\nif '/app' not in sys.path:\n    sys.path.insert(0, '/app')\nimport web_api\n"
  );
  handleJson = pyodide.globals.get("web_api").handle_json;

  const loadMs = Math.round(performance.now() - t0);
  self.postMessage({ type: "ready", version: PYODIDE_VERSION, loadMs });
}

self.onmessage = async (ev) => {
  const msg = ev.data || {};
  if (msg.type === "init") {
    try {
      await init();
    } catch (err) {
      self.postMessage({ type: "init_error", error: String(err && err.message ? err.message : err) });
    }
    return;
  }

  if (msg.type === "call") {
    const { id, cmd, payload } = msg;
    const t0 = performance.now();
    try {
      if (!handleJson) {
        throw new Error("Python runtime is not initialized");
      }
      const raw = handleJson(JSON.stringify({ cmd, payload }));
      const result = JSON.parse(raw);
      const elapsedMs = Math.round(performance.now() - t0);
      self.postMessage({ type: "result", id, ok: true, result, elapsedMs });
    } catch (err) {
      const elapsedMs = Math.round(performance.now() - t0);
      self.postMessage({
        type: "result",
        id,
        ok: false,
        error: String(err && err.message ? err.message : err),
        elapsedMs,
      });
    }
  }
};
