/* Griff Studio Web application: worker bridge, IndexedDB store, Web Audio sampler, UI. */
(() => {
  "use strict";

  const DB_NAME = "griff-studio-web";
  const DB_VERSION = 1;
  const SAMPLES_BASE = "./samples/";
  const KNOWN_SECTIONS = new Set(["intro", "A", "A2", "B", "A3", "end"]);
  const SHORT_SECTION_LABELS = { intro: "Вст.", A: "A", A2: "A2", B: "B", A3: "Кульм.", end: "Конц." };
  const SLIDER_DEBOUNCE_MS = 600;
  const LONG_PRESS_MS = 500;
  const MARK_TEXT_MAX = 80;
  const MARK_GUITARS = [["lead", "Соло"], ["rhythm", "Ритм"], ["both", "Обе"]];
  const MARK_GUITAR_LABELS = { lead: "соло", rhythm: "ритм", both: "обе" };
  const MARK_HINT =
    "Отметьте место, где звучит не по-человечески: кнопка «Тут не так», клавиша M или Shift+клик по таймлайну. Музыку это не меняет.";
  const MODE_LABELS = {
    ionian: "мажор (ионийский)",
    aeolian: "минор (эолийский)",
    dorian: "дорийский",
    phrygian: "фригийский",
    lydian: "лидийский",
    mixolydian: "миксолидийский",
    locrian: "локрийский",
  };
  const modeLabel = (mode) => MODE_LABELS[mode] || mode;
  const KEY_TONICS = ["E", "F#", "G", "A", "B", "C#", "D"];
  const KEY_MODES = [
    ["ionian", "мажор"],
    ["dorian", "дорийский"],
    ["phrygian", "фригийский"],
    ["lydian", "лидийский"],
    ["mixolydian", "миксолидийский"],
    ["aeolian", "натуральный минор"],
    ["locrian", "локрийский"],
  ];

  const appState = {
    pendingSliders: [],
    markEdit: null,
    pieces: [],
    current: null,
    currentId: null,
    currentState: null,
    currentEvents: [],
    busyCount: 0,
    chipCount: 0,
    errorText: "",
    navSeq: 0,
    navPending: false,
    pendingNavId: null,
    pyReady: false,
    samplesReady: false,
  };

  let resolveReadyPromise = null;
  const debugInfo = {
    ready: false,
    pyodideVersion: null,
    pyodideLoadMs: null,
    samplesLoadMs: null,
    lastRerollMs: null,
    readyPromise: new Promise((resolve) => {
      resolveReadyPromise = resolve;
    }),
  };
  window.__studioDebug = debugInfo;

  function maybeMarkReady() {
    if (appState.pyReady && appState.samplesReady && !debugInfo.ready) {
      debugInfo.ready = true;
      updateLoadBanner();
      if (resolveReadyPromise) resolveReadyPromise(debugInfo);
    }
  }

  // ------------------------------------------------------------------ canonical equality
  function canonicalValue(val) {
    if (Array.isArray(val)) return val.map(canonicalValue);
    if (val && typeof val === "object") {
      const out = {};
      for (const k of Object.keys(val).sort()) {
        out[k] = canonicalValue(val[k]);
      }
      return out;
    }
    return val;
  }

  function samePieceState(a, b) {
    if (!a || !b) return false;
    return JSON.stringify(canonicalValue({ recipe: a.recipe, undo: a.undo || [], redo: a.redo || [], marks: a.marks || [] })) ===
      JSON.stringify(canonicalValue({ recipe: b.recipe, undo: b.undo || [], redo: b.redo || [], marks: b.marks || [] }));
  }

  function cloneRecord(rec) {
    return JSON.parse(JSON.stringify(rec));
  }

  // ------------------------------------------------------------------ storage
  const Storage = (() => {
    let dbPromise = null;
    let useMemory = false;
    let storageWarningText = null;
    let logsHydrated = false;
    const memPieces = new Map();
    const memLogs = [];
    let memSeq = 1;

    function updateStorageBanner() {
      const el = document.getElementById("storageBanner");
      if (!el) return;
      if (!storageWarningText) {
        el.hidden = true;
        el.textContent = "";
        return;
      }
      el.hidden = false;
      el.className = "load-banner warn";
      el.textContent = storageWarningText;
    }

    function switchToMemory(reason) {
      useMemory = true;
      storageWarningText = `Локальное хранилище недоступно (${reason}); изменения сохраняются только в памяти до перезагрузки страницы.`;
      updateStorageBanner();
    }

    function sortPieces(rows) {
      return rows.sort((a, b) => (b.updatedAt || "").localeCompare(a.updatedAt || ""));
    }

    function makeLogRow(entry) {
      return {
        t: new Date().toISOString().replace(/\.\d{3}Z$/, "+00:00"),
        action: entry.action,
        layer: entry.layer ?? null,
        section: entry.section ?? null,
        salt: entry.salt ?? null,
        value: entry.value ?? null,
        piece_id: entry.piece_id ?? null,
      };
    }

    async function hydrateLogs(db) {
      if (logsHydrated || !db) return;
      try {
        const rows = await new Promise((resolve, reject) => {
          const tx = db.transaction("logs", "readonly");
          const req = tx.objectStore("logs").getAll();
          req.onsuccess = () => resolve(req.result || []);
          req.onerror = () => reject(req.error || new Error("logs read error"));
          tx.onabort = () => reject(tx.error || new Error("logs read aborted"));
        });
        if (!logsHydrated) {
          const existingKeys = new Set(memLogs.map((r) => `${r.t}|${r.action}|${r.piece_id}|${JSON.stringify(r.salt)}|${JSON.stringify(r.value)}`));
          const merged = [];
          for (const { seq, ...rest } of rows) {
            merged.push({ ...rest, seq: memSeq++ });
          }
          for (const r of memLogs) {
            const k = `${r.t}|${r.action}|${r.piece_id}|${JSON.stringify(r.salt)}|${JSON.stringify(r.value)}`;
            if (!existingKeys.has(k)) merged.push(r);
          }
          memLogs.length = 0;
          memLogs.push(...merged);
          logsHydrated = true;
        }
      } catch (_) {}
    }

    function openDb() {
      if (useMemory) return Promise.resolve(null);
      if (dbPromise) return dbPromise;
      dbPromise = new Promise((resolve) => {
        try {
          if (!window.indexedDB) {
            switchToMemory("IndexedDB отсутствует");
            resolve(null);
            return;
          }
          const req = window.indexedDB.open(DB_NAME, DB_VERSION);
          req.onupgradeneeded = () => {
            const db = req.result;
            if (!db.objectStoreNames.contains("pieces")) {
              db.createObjectStore("pieces", { keyPath: "id" });
            }
            if (!db.objectStoreNames.contains("logs")) {
              db.createObjectStore("logs", { keyPath: "seq", autoIncrement: true });
            }
          };
          req.onsuccess = () => {
            const db = req.result;
            hydrateLogs(db);
            resolve(db);
          };
          req.onerror = () => {
            switchToMemory(req.error?.message || "ошибка открытия");
            resolve(null);
          };
        } catch (err) {
          switchToMemory(err?.message || "исключение IndexedDB");
          resolve(null);
        }
      });
      return dbPromise;
    }

    async function listPieces() {
      try {
        const db = await openDb();
        if (!db || useMemory) {
          return sortPieces(Array.from(memPieces.values()).map(cloneRecord));
        }
        const rows = await new Promise((resolve, reject) => {
          const tx = db.transaction("pieces", "readonly");
          const req = tx.objectStore("pieces").getAll();
          req.onsuccess = () => resolve(req.result || []);
          req.onerror = () => reject(req.error || new Error("listPieces error"));
          tx.onabort = () => reject(tx.error || new Error("listPieces aborted"));
        });
        for (const r of rows) {
          memPieces.set(r.id, cloneRecord(r));
        }
        return sortPieces(rows);
      } catch (err) {
        switchToMemory(err?.message || "ошибка чтения списка");
        return sortPieces(Array.from(memPieces.values()).map(cloneRecord));
      }
    }

    async function getPiece(id) {
      try {
        const db = await openDb();
        if (!db || useMemory) {
          const hit = memPieces.get(id);
          return hit ? cloneRecord(hit) : null;
        }
        const row = await new Promise((resolve, reject) => {
          const tx = db.transaction("pieces", "readonly");
          const req = tx.objectStore("pieces").get(id);
          req.onsuccess = () => resolve(req.result || null);
          req.onerror = () => reject(req.error || new Error("getPiece error"));
          tx.onabort = () => reject(tx.error || new Error("getPiece aborted"));
        });
        if (row) memPieces.set(id, cloneRecord(row));
        return row;
      } catch (err) {
        switchToMemory(err?.message || "ошибка чтения пьесы");
        const hit = memPieces.get(id);
        return hit ? cloneRecord(hit) : null;
      }
    }

    async function savePieceAndLog(record, logEntry = null) {
      const recCopy = cloneRecord(record);
      const row = logEntry ? makeLogRow(logEntry) : null;
      const db = await openDb();
      if (!db || useMemory) {
        memPieces.set(recCopy.id, recCopy);
        if (row) memLogs.push({ ...row, seq: memSeq++ });
        return;
      }
      await hydrateLogs(db);
      try {
        await new Promise((resolve, reject) => {
          let settled = false;
          const finish = (err) => {
            if (settled) return;
            settled = true;
            if (err) reject(err);
            else resolve();
          };
          const stores = row ? ["pieces", "logs"] : ["pieces"];
          const tx = db.transaction(stores, "readwrite");
          tx.oncomplete = () => finish();
          tx.onerror = () => finish(tx.error || new Error("IndexedDB transaction failed"));
          tx.onabort = () => finish(tx.error || new Error("IndexedDB transaction aborted"));
          const reqP = tx.objectStore("pieces").put(recCopy);
          reqP.onerror = () => finish(reqP.error || new Error("pieces put failed"));
          if (row) {
            const reqL = tx.objectStore("logs").add(row);
            reqL.onerror = () => finish(reqL.error || new Error("logs add failed"));
          }
        });
        memPieces.set(recCopy.id, recCopy);
        if (row) memLogs.push({ ...row, seq: memSeq++ });
      } catch (err) {
        switchToMemory(err?.name || err?.message || "ошибка записи");
        memPieces.set(recCopy.id, recCopy);
        if (row) memLogs.push({ ...row, seq: memSeq++ });
      }
    }

    async function putPiece(record) {
      return savePieceAndLog(record, null);
    }

    async function appendLog(entry) {
      const row = makeLogRow(entry);
      const db = await openDb();
      if (!db || useMemory) {
        memLogs.push({ ...row, seq: memSeq++ });
        return;
      }
      await hydrateLogs(db);
      try {
        await new Promise((resolve, reject) => {
          let settled = false;
          const finish = (err) => {
            if (settled) return;
            settled = true;
            if (err) reject(err);
            else resolve();
          };
          const tx = db.transaction("logs", "readwrite");
          tx.oncomplete = () => finish();
          tx.onerror = () => finish(tx.error || new Error("logs tx failed"));
          tx.onabort = () => finish(tx.error || new Error("logs tx aborted"));
          const req = tx.objectStore("logs").add(row);
          req.onerror = () => finish(req.error || new Error("logs add failed"));
        });
        memLogs.push({ ...row, seq: memSeq++ });
      } catch (err) {
        switchToMemory(err?.name || err?.message || "ошибка журнала");
        memLogs.push({ ...row, seq: memSeq++ });
      }
    }

    function insertMemoryUnique(baseSnap, buildLogEntry) {
      const baseId = baseSnap.state.id;
      let n = 1;
      while (true) {
        const candidateId = n === 1 ? baseId : `${baseId}_${n}`;
        if (!memPieces.has(candidateId)) {
          const snap = cloneRecord(baseSnap);
          snap.state.id = candidateId;
          snap.detail.id = candidateId;
          const rec = {
            id: candidateId,
            state: snap.state,
            detail: snap.detail,
            updatedAt: new Date().toISOString(),
          };
          memPieces.set(candidateId, rec);
          memLogs.push({ ...makeLogRow(buildLogEntry(candidateId)), seq: memSeq++ });
          return { snap, rec };
        }
        n += 1;
      }
    }

    async function insertUniquePieceAndLog(baseSnap, buildLogEntry) {
      const db = await openDb();
      if (!db || useMemory) {
        return insertMemoryUnique(baseSnap, buildLogEntry);
      }
      await hydrateLogs(db);
      const baseId = baseSnap.state.id;
      let n = 1;
      while (true) {
        const candidateId = n === 1 ? baseId : `${baseId}_${n}`;
        const snap = cloneRecord(baseSnap);
        snap.state.id = candidateId;
        snap.detail.id = candidateId;
        const rec = {
          id: candidateId,
          state: snap.state,
          detail: snap.detail,
          updatedAt: new Date().toISOString(),
        };
        const logRow = makeLogRow(buildLogEntry(candidateId));
        try {
          const outcome = await new Promise((resolve, reject) => {
            let settled = false;
            let sawConstraint = false;
            const finish = (res, err) => {
              if (settled) return;
              settled = true;
              if (err) reject(err);
              else resolve(res);
            };
            const tx = db.transaction(["pieces", "logs"], "readwrite");
            tx.oncomplete = () => finish("ok");
            tx.onerror = () => {
              if (sawConstraint || tx.error?.name === "ConstraintError") finish("conflict");
              else finish(null, tx.error || new Error("insert tx failed"));
            };
            tx.onabort = () => {
              if (sawConstraint || tx.error?.name === "ConstraintError") finish("conflict");
              else finish(null, tx.error || new Error("insert tx aborted"));
            };
            const reqAdd = tx.objectStore("pieces").add(rec);
            reqAdd.onsuccess = () => {
              const reqLog = tx.objectStore("logs").add(logRow);
              reqLog.onerror = () => finish(null, reqLog.error || new Error("log add failed"));
            };
            reqAdd.onerror = () => {
              if (reqAdd.error?.name === "ConstraintError") {
                sawConstraint = true;
                finish("conflict");
              } else {
                finish(null, reqAdd.error || new Error("piece add failed"));
              }
            };
          });
          if (outcome === "ok") {
            memPieces.set(candidateId, cloneRecord(rec));
            memLogs.push({ ...logRow, seq: memSeq++ });
            return { snap, rec };
          }
          n += 1;
        } catch (err) {
          switchToMemory(err?.name || err?.message || "ошибка вставки");
          return insertMemoryUnique(baseSnap, buildLogEntry);
        }
      }
    }

    function importMemoryUnique(baseSnap, updatedAt) {
      const baseId = baseSnap.state.id;
      let n = 1;
      while (true) {
        const candidateId = n === 1 ? baseId : `${baseId}_${n}`;
        const existing = memPieces.get(candidateId);
        if (existing) {
          if (samePieceState(existing.state, baseSnap.state)) {
            return { id: candidateId, status: "existing" };
          }
          n += 1;
          continue;
        }
        const snap = cloneRecord(baseSnap);
        snap.state.id = candidateId;
        snap.detail.id = candidateId;
        memPieces.set(candidateId, {
          id: candidateId,
          state: snap.state,
          detail: snap.detail,
          updatedAt: updatedAt || new Date().toISOString(),
        });
        return { id: candidateId, status: "inserted" };
      }
    }

    async function importUniquePiece(baseSnap, updatedAt) {
      const db = await openDb();
      if (!db || useMemory) {
        return importMemoryUnique(baseSnap, updatedAt);
      }
      const baseId = baseSnap.state.id;
      let n = 1;
      while (true) {
        const candidateId = n === 1 ? baseId : `${baseId}_${n}`;
        const snap = cloneRecord(baseSnap);
        snap.state.id = candidateId;
        snap.detail.id = candidateId;
        const rec = {
          id: candidateId,
          state: snap.state,
          detail: snap.detail,
          updatedAt: updatedAt || new Date().toISOString(),
        };
        try {
          const outcome = await new Promise((resolve, reject) => {
            let settled = false;
            let sawConstraint = false;
            let resolvedStatus = null;
            const finish = (res, err) => {
              if (settled) return;
              settled = true;
              if (err) reject(err);
              else resolve(res);
            };
            const tx = db.transaction(["pieces"], "readwrite");
            const store = tx.objectStore("pieces");
            tx.oncomplete = () => finish(resolvedStatus || { id: candidateId, status: "inserted" });
            tx.onerror = () => {
              if (sawConstraint || tx.error?.name === "ConstraintError") finish({ status: "conflict" });
              else finish(null, tx.error || new Error("import tx failed"));
            };
            tx.onabort = () => {
              if (sawConstraint || tx.error?.name === "ConstraintError") finish({ status: "conflict" });
              else finish(null, tx.error || new Error("import tx aborted"));
            };
            const reqGet = store.get(candidateId);
            reqGet.onsuccess = () => {
              const existing = reqGet.result;
              if (existing) {
                if (samePieceState(existing.state, baseSnap.state)) {
                  resolvedStatus = { id: candidateId, status: "existing" };
                } else {
                  resolvedStatus = { status: "conflict" };
                }
                return;
              }
              const reqAdd = store.add(rec);
              reqAdd.onerror = () => {
                if (reqAdd.error?.name === "ConstraintError") {
                  sawConstraint = true;
                  finish({ status: "conflict" });
                } else {
                  finish(null, reqAdd.error || new Error("import add failed"));
                }
              };
            };
            reqGet.onerror = () => finish(null, reqGet.error || new Error("import get failed"));
          });
          if (outcome.status === "conflict") {
            n += 1;
            continue;
          }
          if (outcome.status === "inserted") {
            memPieces.set(candidateId, cloneRecord(rec));
          }
          return outcome;
        } catch (err) {
          switchToMemory(err?.name || err?.message || "ошибка импорта");
          return importMemoryUnique(baseSnap, updatedAt);
        }
      }
    }

    async function getAllLogs() {
      try {
        const db = await openDb();
        if (!db || useMemory) {
          return memLogs.map(({ seq, ...rest }) => rest);
        }
        const rows = await new Promise((resolve, reject) => {
          const tx = db.transaction("logs", "readonly");
          const req = tx.objectStore("logs").getAll();
          req.onsuccess = () => resolve(req.result || []);
          req.onerror = () => reject(req.error || new Error("getAllLogs error"));
          tx.onabort = () => reject(tx.error || new Error("getAllLogs aborted"));
        });
        logsHydrated = true;
        memLogs.length = 0;
        for (const r of rows) {
          memLogs.push({ ...r, seq: memSeq++ });
        }
        return rows.map(({ seq, ...rest }) => rest);
      } catch (err) {
        switchToMemory(err?.message || "ошибка чтения журнала");
        return memLogs.map(({ seq, ...rest }) => rest);
      }
    }

    return {
      listPieces,
      getPiece,
      putPiece,
      appendLog,
      savePieceAndLog,
      insertUniquePieceAndLog,
      importUniquePiece,
      getAllLogs,
    };
  })();

  // ------------------------------------------------------------------ worker
  let worker = null;
  let callSeq = 1;
  const pendingCalls = new Map();
  let pyProgressText = "Загрузка Python (Pyodide)...";
  let sampleProgressText = "Загрузка сэмплов...";
  let pyErrorText = null;
  let samplesErrorText = null;

  let pyReadyState = null;
  let pyReadyPromise = null;

  function resetPyReadyPromise() {
    pyReadyState = { settled: false, resolve: null, reject: null };
    pyReadyPromise = new Promise((resolve, reject) => {
      pyReadyState.resolve = (val) => {
        if (pyReadyState.settled) return;
        pyReadyState.settled = true;
        resolve(val);
      };
      pyReadyState.reject = (err) => {
        if (pyReadyState.settled) return;
        pyReadyState.settled = true;
        reject(err);
      };
    });
    pyReadyPromise.catch(() => {});
  }

  function failWorker(message) {
    const err = new Error(message || "Worker error");
    pyErrorText = `Ошибка Python/Worker: ${err.message}`;
    appState.pyReady = false;
    pyReadyState.reject(err);
    for (const [, entry] of pendingCalls.entries()) {
      entry.reject(err);
    }
    pendingCalls.clear();
    updateLoadBanner();
  }

  function startWorker() {
    if (worker && typeof worker.terminate === "function") {
      try {
        worker.terminate();
      } catch (_) {}
    }
    pyErrorText = null;
    appState.pyReady = false;
    pyProgressText = "Загрузка Python (Pyodide)...";
    resetPyReadyPromise();
    updateLoadBanner();

    const w = new Worker("./worker.js");
    worker = w;

    w.onmessage = (ev) => {
      if (w !== worker) return;
      const msg = ev.data || {};
      if (msg.type === "progress") {
        pyProgressText = msg.text;
        updateLoadBanner();
        return;
      }
      if (msg.type === "ready") {
        appState.pyReady = true;
        pyErrorText = null;
        debugInfo.pyodideVersion = msg.version;
        debugInfo.pyodideLoadMs = msg.loadMs;
        pyProgressText = `Python готов (${msg.version}, ${msg.loadMs} мс)`;
        updateLoadBanner();
        pyReadyState.resolve();
        maybeMarkReady();
        return;
      }
      if (msg.type === "init_error") {
        failWorker(msg.error || "Pyodide initialization failed");
        return;
      }
      if (msg.type === "result") {
        const entry = pendingCalls.get(msg.id);
        if (!entry) return;
        pendingCalls.delete(msg.id);
        if (msg.ok) {
          entry.resolve({ result: msg.result, elapsedMs: msg.elapsedMs });
        } else {
          entry.reject(new Error(msg.error || "Worker error"));
        }
      }
    };

    w.onerror = (err) => {
      if (w !== worker) return;
      failWorker(err?.message || "Web Worker crash");
    };
    w.onmessageerror = () => {
      if (w !== worker) return;
      failWorker("Web Worker message error");
    };

    w.postMessage({ type: "init" });
  }

  function setBusy(delta) {
    appState.busyCount = Math.max(0, appState.busyCount + delta);
    const badge = document.getElementById("busy");
    if (badge) badge.classList.toggle("show", appState.busyCount > 0);
  }

  function setChip(delta) {
    appState.chipCount = Math.max(0, appState.chipCount + delta);
    const chip = document.getElementById("statusChip");
    if (chip) chip.textContent = appState.chipCount > 0 ? "Пересчёт…" : "";
  }

  function paintError() {
    const banner = document.getElementById("errorBanner");
    if (!banner) return;
    const text = document.getElementById("errorText");
    banner.hidden = !appState.errorText;
    if (text) text.textContent = appState.errorText;
  }

  function showError(err, pieceId) {
    const msg = err && err.message ? err.message : String(err || "");
    let where = "";
    if (pieceId && pieceId !== appState.currentId) {
      const p = appState.pieces.find((x) => x.id === pieceId);
      where = ` (пьеса #${p ? (p.detail || p).seed : pieceId})`;
    }
    appState.errorText = `Ошибка${where}: ${msg}`;
    paintError();
  }

  async function flushPendingSliders() {
    for (const flush of appState.pendingSliders.splice(0)) await flush();
  }

  function dropPendingSlider(flush) {
    const i = appState.pendingSliders.indexOf(flush);
    if (i >= 0) appState.pendingSliders.splice(i, 1);
  }

  async function callPy(cmd, payload = {}) {
    await pyReadyPromise;
    setBusy(1);
    const id = callSeq++;
    try {
      return await new Promise((resolve, reject) => {
        pendingCalls.set(id, { resolve, reject });
        worker.postMessage({ type: "call", id, cmd, payload });
      });
    } finally {
      setBusy(-1);
    }
  }

  startWorker();

  // ------------------------------------------------------------------ sampler
  const Sampler = (() => {
    let manifest = null;
    const rawBuffers = new Map();
    const decodedBuffers = new Map();
    const decodingPromises = new Map();

    let ctx = null;
    let panLeft = null;
    let panRight = null;
    let events = [];
    let duration = 0;
    let playing = false;
    let starting = false;
    let playGen = 0;
    let startCtxTime = 0;
    let playOffset = 0;
    let nextIdx = 0;
    let schedTimer = null;
    let rafId = null;
    let stopAt = null;
    let audioErrorText = null;
    const activeSources = new Set();

    function updateAudioBanner() {
      const el = document.getElementById("audioBanner");
      if (!el) return;
      el.textContent = "";
      if (!audioErrorText) {
        el.hidden = true;
        return;
      }
      el.hidden = false;
      el.className = "load-banner error";
      const span = document.createElement("span");
      span.textContent = audioErrorText;
      el.appendChild(span);
      const btn = document.createElement("button");
      btn.id = "retryAudioBtn";
      btn.className = "banner-retry";
      btn.textContent = "Повторить";
      btn.onclick = () => play();
      el.appendChild(btn);
    }

    function ensureContext() {
      if (!ctx) {
        const AC = window.AudioContext || window.webkitAudioContext;
        ctx = new AC();
        const master = ctx.createDynamicsCompressor();
        master.threshold.value = -10;
        master.ratio.value = 3;
        master.connect(ctx.destination);

        const dry = ctx.createGain();
        dry.gain.value = 0.9;
        dry.connect(master);

        // Lightweight convolution-free reverb using a feedback delay pair.
        const wet = ctx.createGain();
        wet.gain.value = 0.14;
        const d1 = ctx.createDelay(0.5);
        d1.delayTime.value = 0.043;
        const d2 = ctx.createDelay(0.5);
        d2.delayTime.value = 0.067;
        const fb = ctx.createGain();
        fb.gain.value = 0.25;
        const lp = ctx.createBiquadFilter();
        lp.type = "lowpass";
        lp.frequency.value = 3200;
        d1.connect(lp);
        lp.connect(d2);
        d2.connect(fb);
        fb.connect(d1);
        d1.connect(wet);
        d2.connect(wet);
        wet.connect(master);

        panLeft = ctx.createStereoPanner();
        panLeft.pan.value = -0.47;
        panLeft.connect(dry);
        panLeft.connect(d1);

        panRight = ctx.createStereoPanner();
        panRight.pan.value = 0.47;
        panRight.connect(dry);
        panRight.connect(d1);

        const onStateChange = () => {
          if (playing && ctx && ctx.state !== "running") {
            pause(true);
            audioErrorText = `Воспроизведение приостановлено (${ctx.state})`;
            updateAudioBanner();
          }
        };
        if (typeof ctx.addEventListener === "function") {
          ctx.addEventListener("statechange", onStateChange);
        } else {
          ctx.onstatechange = onStateChange;
        }
      }
      return ctx;
    }

    function findZeroCrossing(pcm, sr, targetSec, searchSec = 0.03) {
      const target = Math.max(1, Math.min(pcm.length - 1, Math.round(targetSec * sr)));
      const rad = Math.max(1, Math.round(searchSec * sr));
      const lo = Math.max(1, target - rad);
      const hi = Math.min(pcm.length - 1, target + rad);
      let bestIdx = target;
      let bestDist = Infinity;
      for (let i = lo; i <= hi; i++) {
        if (pcm[i - 1] <= 0 && pcm[i] > 0) {
          const d = Math.abs(i - target);
          if (d < bestDist) {
            bestDist = d;
            bestIdx = i;
          }
        }
      }
      return bestIdx / sr;
    }

    function computeLoopRegion(buf) {
      const bufDur = Number(buf && buf.duration) || 2.5;
      if (bufDur <= 0.2) return { loopStart: 0, loopEnd: bufDur };
      let loopStart = bufDur * 0.45;
      let loopEnd = bufDur * 0.85;
      if (typeof buf.getChannelData === "function" && buf.sampleRate > 0 && buf.length > 4) {
        const pcm = buf.getChannelData(0);
        if (pcm && pcm.length > 4) {
          const zStart = findZeroCrossing(pcm, buf.sampleRate, loopStart);
          const zEnd = findZeroCrossing(pcm, buf.sampleRate, loopEnd);
          if (zEnd > zStart + 0.1 && zEnd < bufDur - 0.05) {
            loopStart = zStart;
            loopEnd = zEnd;
          }
        }
      }
      return { loopStart, loopEnd };
    }

    async function preload() {
      const t0 = performance.now();
      samplesErrorText = null;
      appState.samplesReady = false;
      sampleProgressText = "Загрузка сэмплов...";
      updateLoadBanner();
      updatePlayButton();
      try {
        const res = await fetch(SAMPLES_BASE + "manifest.json");
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        manifest = await res.json();
        const tasks = [];
        for (const [pitchStr, vels] of Object.entries(manifest.files || {})) {
          for (const [velStr, fname] of Object.entries(vels)) {
            tasks.push({ key: `${pitchStr}:${velStr}`, fname });
          }
        }
        let done = 0;
        const total = tasks.length;
        sampleProgressText = `Загрузка сэмплов: 0 / ${total}...`;
        updateLoadBanner();

        const concurrency = 8;
        let cursor = 0;
        async function workerTask() {
          while (cursor < tasks.length) {
            const item = tasks[cursor++];
            const r = await fetch(SAMPLES_BASE + item.fname);
            if (!r.ok) throw new Error(`Sample ${item.fname}: HTTP ${r.status}`);
            rawBuffers.set(item.key, await r.arrayBuffer());
            done += 1;
            if (done % 6 === 0 || done === total) {
              sampleProgressText = `Загрузка сэмплов: ${done} / ${total}...`;
              updateLoadBanner();
            }
          }
        }
        await Promise.all(Array.from({ length: concurrency }, workerTask));
        appState.samplesReady = true;
        debugInfo.samplesLoadMs = Math.round(performance.now() - t0);
        sampleProgressText = `Сэмплы готовы (${total} файлов, ${debugInfo.samplesLoadMs} мс)`;
        updateLoadBanner();
        updatePlayButton();
        maybeMarkReady();
      } catch (err) {
        appState.samplesReady = false;
        samplesErrorText = `Ошибка загрузки сэмплов: ${err?.message || err}`;
        updateLoadBanner();
        updatePlayButton();
      }
    }

    function sampleKeyForEvent(ev) {
      const clampedPitch = Math.max(40, Math.min(86, ev.pitch));
      const layerVel = ev.vel <= 86 ? 72 : 100;
      return { clampedPitch, layerVel, key: `${clampedPitch}:${layerVel}` };
    }

    function getDecodedBuffer(pitch, velLayer) {
      const clampedPitch = Math.max(40, Math.min(86, pitch));
      const key = `${clampedPitch}:${velLayer}`;
      if (decodedBuffers.has(key)) {
        return Promise.resolve(decodedBuffers.get(key));
      }
      if (decodingPromises.has(key)) return decodingPromises.get(key);
      const raw = rawBuffers.get(key);
      if (!raw || !ctx) {
        return Promise.reject(new Error(`Отсутствует сэмпл ${key}`));
      }
      const p = ctx
        .decodeAudioData(raw.slice(0))
        .then((buf) => {
          const { loopStart, loopEnd } = computeLoopRegion(buf);
          const entry = { buf, basePitch: clampedPitch, loopStart, loopEnd };
          decodedBuffers.set(key, entry);
          decodingPromises.delete(key);
          return entry;
        })
        .catch((err) => {
          decodingPromises.delete(key);
          throw err instanceof Error ? err : new Error(String(err || `Ошибка декодирования ${key}`));
        });
      decodingPromises.set(key, p);
      return p;
    }

    function stopActiveSources() {
      for (const src of activeSources) {
        try {
          src.stop();
        } catch (_) {}
      }
      activeSources.clear();
    }

    function noteAudibleDur(ev) {
      return ev.dead ? 0.04 : Math.max(0.02, ev.dur);
    }

    function scheduleWithEntry(ev, when, entry, offsetSec = 0, playDur = null) {
      const now = ctx.currentTime;
      const startAt = Math.max(when, now);
      const effectiveDur = playDur !== null ? Math.max(0.02, playDur) : Math.max(0.02, ev.dur);
      const layerVel = ev.vel <= 86 ? 72 : 100;
      const baseGain = (ev.vel / layerVel) * 0.55;

      const src = ctx.createBufferSource();
      src.buffer = entry.buf;
      const playbackRate = ev.pitch !== entry.basePitch ? Math.pow(2, (ev.pitch - entry.basePitch) / 12) : 1;
      if (playbackRate !== 1) {
        src.playbackRate.value = playbackRate;
      }

      const gain = ctx.createGain();
      const peakGain = ev.dead ? baseGain * 0.35 : baseGain;
      gain.gain.setValueAtTime(peakGain, startAt);

      const bufDur = Number(entry.buf && entry.buf.duration) || 2.5;
      let bufOffset = Math.max(0, offsetSec * playbackRate);

      if (ev.dead) {
        const clickDur = Math.max(0.01, 0.04 - offsetSec);
        bufOffset = Math.min(bufOffset, Math.max(0, bufDur - 0.01));
        gain.gain.linearRampToValueAtTime(0.0001, startAt + clickDur);
        src.connect(gain);
        gain.connect(ev.guitar === 1 ? panLeft : panRight);
        src.start(startAt, bufOffset);
        src.stop(startAt + clickDur + 0.005);
      } else {
        const neededSampleTime = bufOffset + (effectiveDur + 0.035) * playbackRate;
        if (bufDur > 0.2 && neededSampleTime > bufDur - 0.04 && entry.loopEnd > entry.loopStart) {
          src.loop = true;
          src.loopStart = entry.loopStart;
          src.loopEnd = entry.loopEnd;
          if (bufOffset >= entry.loopEnd) {
            const loopLen = entry.loopEnd - entry.loopStart;
            bufOffset = entry.loopStart + ((bufOffset - entry.loopStart) % loopLen);
          }
        } else if (bufOffset > Math.max(0, bufDur - 0.005)) {
          bufOffset = Math.max(0, bufDur - 0.005);
        }
        const noteEnd = startAt + effectiveDur;
        gain.gain.setValueAtTime(peakGain, noteEnd);
        gain.gain.linearRampToValueAtTime(0.0001, noteEnd + 0.03);
        src.connect(gain);
        gain.connect(ev.guitar === 1 ? panLeft : panRight);
        src.start(startAt, bufOffset);
        src.stop(noteEnd + 0.035);
      }
      activeSources.add(src);
      src.onended = () => activeSources.delete(src);
    }

    function handleDecodeFailure(err, gen) {
      if (gen !== playGen) return;
      pause(true);
      audioErrorText = `Ошибка декодирования аудио: ${err?.message || err}`;
      updateAudioBanner();
    }

    function scheduleNote(ev, when, gen, offsetSec = 0, playDur = null) {
      if (!ctx || gen !== playGen || !playing) return;
      const { clampedPitch, layerVel, key } = sampleKeyForEvent(ev);
      if (decodedBuffers.has(key)) {
        scheduleWithEntry(ev, when, decodedBuffers.get(key), offsetSec, playDur);
        return;
      }
      getDecodedBuffer(clampedPitch, layerVel)
        .then((entry) => {
          if (gen !== playGen || !playing) return;
          scheduleWithEntry(ev, when, entry, offsetSec, playDur);
        })
        .catch((err) => handleDecodeFailure(err, gen));
    }

    async function prepareOpeningBuffers(offset) {
      const openingKeys = new Map();
      for (const ev of events) {
        const { clampedPitch, layerVel, key } = sampleKeyForEvent(ev);
        if (!rawBuffers.has(key)) {
          throw new Error(`Сэмпл ${key} не загружен`);
        }
        const end = ev.t + noteAudibleDur(ev);
        const overlaps = ev.t < offset - 0.005 && end > offset + 0.005;
        const upcoming = ev.t >= offset - 0.005 && ev.t <= offset + 1.0;
        if ((overlaps || upcoming) && !decodedBuffers.has(key)) {
          openingKeys.set(key, [clampedPitch, layerVel]);
        }
      }
      await Promise.all(
        Array.from(openingKeys.values()).map(([p, v]) => getDecodedBuffer(p, v))
      );
    }

    function tickScheduler() {
      if (!playing || !ctx) return;
      const gen = playGen;
      const now = ctx.currentTime;
      const elapsed = now - startCtxTime;
      const horizon = elapsed + 0.22;
      while (nextIdx < events.length && events[nextIdx].t <= horizon && (stopAt === null || events[nextIdx].t < stopAt)) {
        const ev = events[nextIdx++];
        if (ev.t >= playOffset - 0.005) {
          scheduleNote(ev, startCtxTime + ev.t, gen);
        }
      }
      if (stopAt !== null && elapsed >= stopAt) {
        const end = stopAt;
        pause(false);
        playOffset = end;
        syncTransportUI(end);
        return;
      }
      if (elapsed >= duration && activeSources.size === 0) {
        pause(false);
        playOffset = 0;
        syncTransportUI(0);
      }
    }

    function currentTime() {
      if (!playing || !ctx) return playOffset;
      return Math.min(duration, Math.max(0, ctx.currentTime - startCtxTime));
    }

    function startLoop() {
      if (rafId) cancelAnimationFrame(rafId);
      const step = () => {
        const t = currentTime();
        syncTransportUI(t);
        if (playing) {
          rafId = requestAnimationFrame(step);
        } else {
          rafId = null;
        }
      };
      rafId = requestAnimationFrame(step);
    }

    async function play() {
      if (!events.length || !appState.samplesReady) return false;
      if (playing || starting) return true;
      const gen = ++playGen;
      starting = true;
      audioErrorText = null;
      updateAudioBanner();
      updatePlayButton();

      try {
        ensureContext();
        const resumePromise = ctx.state !== "running" ? ctx.resume() : Promise.resolve();
        await resumePromise;
        if (gen !== playGen) return false;
        if (ctx.state !== "running") {
          throw new Error(`аудиоконтекст в состоянии "${ctx.state}"`);
        }

        if (playOffset >= duration) playOffset = 0;
        await prepareOpeningBuffers(playOffset);
        if (gen !== playGen || ctx.state !== "running") return false;

        starting = false;
        playing = true;
        startCtxTime = ctx.currentTime - playOffset;

        for (const ev of events) {
          const end = ev.t + noteAudibleDur(ev);
          if (ev.t < playOffset - 0.005 && end > playOffset + 0.005) {
            const elapsedInNote = playOffset - ev.t;
            const remainingDur = end - playOffset;
            scheduleNote(ev, startCtxTime + playOffset, gen, elapsedInNote, remainingDur);
          }
        }

        nextIdx = events.findIndex((e) => e.t >= playOffset - 0.005);
        if (nextIdx === -1) nextIdx = events.length;
        if (schedTimer) clearInterval(schedTimer);
        schedTimer = setInterval(tickScheduler, 50);
        tickScheduler();
        startLoop();
        updatePlayButton();
        return true;
      } catch (err) {
        if (gen === playGen) {
          starting = false;
          playing = false;
          stopActiveSources();
          audioErrorText = `Не удалось запустить звук: ${err?.message || err}`;
          updateAudioBanner();
          updatePlayButton();
        }
        return false;
      }
    }

    function pause(keepPos = true) {
      if (keepPos) {
        playOffset = currentTime();
      }
      playGen += 1;
      starting = false;
      playing = false;
      stopAt = null;
      if (schedTimer) {
        clearInterval(schedTimer);
        schedTimer = null;
      }
      if (rafId) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
      stopActiveSources();
      updatePlayButton();
      syncTransportUI(playOffset);
    }

    function seek(sec, autoplay = false) {
      const wasActive = playing || starting;
      pause(false);
      playOffset = Math.max(0, Math.min(duration, Number(sec) || 0));
      syncTransportUI(playOffset);
      if ((wasActive || autoplay) && appState.samplesReady) {
        return play();
      }
      return Promise.resolve(false);
    }

    async function playRange(from, to) {
      const ok = await seek(from, true);
      if (ok) stopAt = to;
      return ok;
    }

    function setPiece(nextEvents, nextDuration, preservePos = false) {
      const wasActive = playing || starting;
      const prevTime = currentTime();
      pause(false);
      events = Array.isArray(nextEvents) ? nextEvents : [];
      duration = Math.max(0.1, Number(nextDuration) || 0.1);
      playOffset = preservePos ? Math.min(prevTime, duration) : 0;
      syncTransportUI(playOffset);
      updatePlayButton();
      if (wasActive && appState.samplesReady) {
        return play();
      }
      return Promise.resolve(false);
    }

    return {
      preload,
      play,
      pause,
      seek,
      playRange,
      setPiece,
      isPlaying: () => playing,
      isBusy: () => playing || starting,
      currentTime,
    };
  })();

  // ------------------------------------------------------------------ helpers
  function fmtClock(sec) {
    const s = Math.max(0, Math.floor(sec));
    const m = Math.floor(s / 60);
    const rem = s % 60;
    return `${m}:${String(rem).padStart(2, "0")}`;
  }

  function fmtSec(s) {
    return `${Number(s).toFixed(1)} с`;
  }

  function downloadBlob(filename, mime, content) {
    const blob = content instanceof Blob ? content : new Blob([content], { type: mime });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  function base64ToBytes(b64) {
    const bin = atob(b64);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  }

  function updateLoadBanner() {
    const el = document.getElementById("loadBanner");
    if (!el) return;
    el.textContent = "";
    if (pyErrorText || samplesErrorText) {
      el.className = "load-banner error";
      const msgSpan = document.createElement("span");
      msgSpan.textContent = [pyErrorText, samplesErrorText].filter(Boolean).join(" | ");
      el.appendChild(msgSpan);
      if (pyErrorText) {
        const retryPy = document.createElement("button");
        retryPy.id = "retryPyBtn";
        retryPy.className = "banner-retry";
        retryPy.textContent = "Повторить Python";
        retryPy.onclick = () => startWorker();
        el.appendChild(retryPy);
      }
      if (samplesErrorText) {
        const retryS = document.createElement("button");
        retryS.id = "retrySamplesBtn";
        retryS.className = "banner-retry";
        retryS.textContent = "Повторить сэмплы";
        retryS.onclick = () => Sampler.preload();
        el.appendChild(retryS);
      }
      return;
    }
    if (appState.pyReady && appState.samplesReady) {
      el.className = "load-banner ready";
      el.textContent = `Готово: ${pyProgressText} · ${sampleProgressText}`;
      return;
    }
    el.className = "load-banner";
    el.textContent = `${pyProgressText} | ${sampleProgressText}`;
  }

  function updatePlayButton() {
    const btn = document.getElementById("playBtn");
    if (!btn) return;
    const isPlaying = Sampler.isPlaying();
    btn.disabled = !appState.samplesReady || !appState.currentEvents.length;
    btn.textContent = isPlaying ? "Пауза" : "Играть";
    btn.classList.toggle("primary", !isPlaying);
    btn.classList.toggle("active", isPlaying);
  }

  function syncTransportUI(t) {
    const curEl = document.getElementById("curTime");
    const seekEl = document.getElementById("seekBar");
    const tl = document.getElementById("timeline");
    const dur = appState.current ? appState.current.length : 0;
    if (curEl) curEl.textContent = `${fmtClock(t)} / ${fmtClock(dur)}`;
    if (seekEl && !seekEl.dataset.dragging) {
      seekEl.value = String(Math.min(dur, t));
      if (curEl) seekEl.setAttribute("aria-valuetext", curEl.textContent);
    }
    if (tl) {
      for (const blk of tl.children) {
        const a = Number(blk.dataset.start);
        const b = Number(blk.dataset.end);
        blk.classList.toggle("playing", t >= a && t < b);
      }
    }
  }

  function setWorkspaceDisabled(disabled) {
    const main = document.getElementById("workspace");
    if (!main) return;
    for (const el of main.querySelectorAll("button, input")) {
      if (el.id === "errorCloseBtn") continue;
      if (disabled) {
        if (el.dataset.wasDisabled === undefined) {
          el.dataset.wasDisabled = el.disabled ? "1" : "0";
        }
        el.disabled = true;
      } else if (el.dataset.wasDisabled !== undefined) {
        el.disabled = el.dataset.wasDisabled === "1";
        delete el.dataset.wasDisabled;
      }
    }
  }

  // ------------------------------------------------------------------ rendering
  function renderList() {
    const el = document.getElementById("pieceList");
    if (!el) return;
    el.textContent = "";
    for (const p of appState.pieces) {
      const d = p.detail || p;
      const btn = document.createElement("button");
      const selected = Boolean(appState.current && appState.current.id === p.id);
      btn.className = "piece-item" + (selected ? " selected" : "");
      if (selected) btn.setAttribute("aria-current", "true");

      const top = document.createElement("div");
      const strong = document.createElement("strong");
      strong.textContent = `#${d.seed}`;
      top.appendChild(strong);
      top.appendChild(document.createTextNode(` · ${d.meter} · ${d.key} ${modeLabel(d.mode)}`));

      const meta = document.createElement("div");
      meta.className = "piece-meta";
      meta.textContent = `${d.tempo} BPM · ${d.length} с`;

      btn.appendChild(top);
      btn.appendChild(meta);
      btn.onclick = () => selectPiece(p.id);
      el.appendChild(btn);
    }
  }

  function makeLayerControl(pieceId, l) {
    const row = document.createElement("div");
    row.className = "layer-row";
    const btn = document.createElement("button");
    btn.className = "reroll-btn";
    btn.dataset.layer = l.layer;
    btn.dataset.section = l.section;
    btn.disabled = Boolean(l.locked);
    if (l.locked) btn.title = "Слой закреплён";

    const lbl = document.createElement("span");
    lbl.textContent = l.label;
    const badge = document.createElement("span");
    badge.className = "salt-badge";
    badge.title = "Сколько раз перегенерировали";
    badge.hidden = !(l.salt > 0);
    badge.textContent = `#${l.salt}`;
    btn.appendChild(lbl);
    btn.appendChild(badge);

    btn.onclick = () =>
      runMutation(
        "reroll",
        { layer: l.layer, section: l.section },
        pieceId,
        (snap) => {
          const whole = l.layer === "all" && l.section === "all";
          const salts = snap.state.recipe.studio.salts || {};
          const salt = whole ? { ...salts } : salts[`${l.layer}@${l.section}`] || 0;
          return { action: "reroll", layer: l.layer, section: l.section, salt };
        },
        btn
      );

    const lockBtn = document.createElement("button");
    lockBtn.className = l.locked ? "locked" : "";
    lockBtn.title = l.locked ? "Открепить" : "Закрепить";
    lockBtn.textContent = l.locked ? "🔒 Закреплено" : "Закрепить";
    lockBtn.setAttribute("aria-pressed", l.locked ? "true" : "false");
    lockBtn.onclick = () =>
      runMutation(
        "lock",
        { layer: l.layer, section: l.section, locked: !l.locked },
        pieceId,
        () => ({ action: "lock", layer: l.layer, section: l.section, value: !l.locked }),
        lockBtn
      );

    row.appendChild(btn);
    row.appendChild(lockBtn);
    return row;
  }

  function makeExportButton(id, label, cmd, filename, mime, logVal) {
    const btn = document.createElement("button");
    btn.id = id;
    btn.textContent = label;
    btn.onclick = () =>
      enqueueForPiece(appState.currentId, async () => {
        const pid = appState.currentId;
        if (!pid) return;
        const stored = await Storage.getPiece(pid);
        const st = (stored && stored.state) || appState.currentState;
        if (!st) return;
        const { result } = await callPy(cmd, { state: st });
        const content = cmd === "export_midi" ? base64ToBytes(result) : result;
        downloadBlob(filename(pid), mime, content);
        await Storage.appendLog({ action: "export", value: logVal, piece_id: pid });
      }).catch((err) => showError(err, appState.currentId));
    return btn;
  }

  let pressTimer = null;
  let swallowClick = false;

  function renderWorkspace() {
    const main = document.getElementById("workspace");
    if (!main) return;
    for (const c of Array.from(main.children)) {
      if (c.id !== "errorBanner") c.remove();
    }
    paintError();
    const d = appState.current;
    if (!d) {
      const empty = document.createElement("div");
      empty.className = "empty";
      empty.textContent = "Нажмите «Новая пьеса» или выберите готовую из списка.";
      main.appendChild(empty);
      return;
    }

    const total = Math.max(1, d.length);

    // Top player & export card
    const playerCard = document.createElement("div");
    playerCard.className = "card";

    const hbar = document.createElement("div");
    hbar.className = "header-bar";

    const titleEl = document.createElement("div");
    titleEl.className = "piece-title";
    const strongTitle = document.createElement("strong");
    strongTitle.textContent = `Пьеса #${d.seed}`;
    titleEl.appendChild(strongTitle);
    titleEl.appendChild(document.createTextNode(` · ${d.meter} · ${d.key} ${modeLabel(d.mode)} · ${d.tempo} BPM · ${d.length} с`));

    const actionRow = document.createElement("div");
    actionRow.className = "row";

    function makeHistoryButton(id, verb, enabled, label, cmd) {
      const btn = document.createElement("button");
      btn.id = id;
      btn.textContent = verb;
      btn.title = enabled && label ? `${verb}: ${label}` : verb;
      if (enabled && label) {
        const hint = document.createElement("span");
        hint.className = "btn-hint";
        hint.textContent = `: ${label}`;
        btn.appendChild(hint);
      }
      btn.disabled = !enabled;
      btn.onclick = () => runMutation(cmd, {}, d.id, () => ({ action: cmd }), btn);
      return btn;
    }

    actionRow.appendChild(makeHistoryButton("undoBtn", "Отменить", d.can_undo, d.undo_label, "undo"));
    actionRow.appendChild(makeHistoryButton("redoBtn", "Вернуть", d.can_redo, d.redo_label, "redo"));
    actionRow.appendChild(makeExportButton("dlMidiBtn", "Скачать MIDI", "export_midi", (id) => `${id}.mid`, "audio/midi", "piece.mid"));
    actionRow.appendChild(makeExportButton("dlMxmlBtn", "Скачать табы (MusicXML)", "export_musicxml", (id) => `${id}.musicxml`, "application/vnd.recordare.musicxml+xml;charset=utf-8", "piece.musicxml"));
    actionRow.appendChild(makeExportButton("dlTabBtn", "ASCII-таб", "export_tab", (id) => `${id}.tab.txt`, "text/plain;charset=utf-8", "piece.tab.txt"));

    hbar.appendChild(titleEl);
    hbar.appendChild(actionRow);
    playerCard.appendChild(hbar);

    const transport = document.createElement("div");
    transport.className = "transport";

    const playBtn = document.createElement("button");
    playBtn.id = "playBtn";
    playBtn.className = "primary";
    playBtn.textContent = "Играть";
    playBtn.onclick = () => (Sampler.isBusy() ? Sampler.pause(true) : Sampler.play());

    const curTime = document.createElement("span");
    curTime.id = "curTime";
    curTime.className = "time-readout";
    curTime.textContent = `0:00 / ${fmtClock(d.length)}`;

    const seekBar = document.createElement("input");
    seekBar.id = "seekBar";
    seekBar.type = "range";
    seekBar.min = "0";
    seekBar.max = String(d.length);
    seekBar.step = "0.05";
    seekBar.value = "0";
    seekBar.setAttribute("aria-label", "Позиция воспроизведения");
    seekBar.setAttribute("aria-valuetext", curTime.textContent);
    seekBar.oninput = () => {
      seekBar.dataset.dragging = "1";
      curTime.textContent = `${fmtClock(Number(seekBar.value))} / ${fmtClock(d.length)}`;
      seekBar.setAttribute("aria-valuetext", curTime.textContent);
    };
    seekBar.onchange = () => {
      delete seekBar.dataset.dragging;
      Sampler.seek(Number(seekBar.value), false);
    };

    transport.appendChild(playBtn);
    transport.appendChild(curTime);
    transport.appendChild(seekBar);
    playerCard.appendChild(transport);

    const tl = document.createElement("div");
    tl.id = "timeline";
    tl.className = "timeline";
    for (const s of d.sections) {
      const dur = Math.max(0.5, s.end_sec - s.start_sec);
      const blk = document.createElement("button");
      const secClass = KNOWN_SECTIONS.has(s.section) ? ` sec-${s.section}` : "";
      blk.className = `sec-block${secClass}`;
      blk.style.flexGrow = String(dur / total);
      blk.dataset.start = String(s.start_sec);
      blk.dataset.end = String(s.end_sec);
      blk.title = s.label;
      blk.setAttribute("aria-label", `${s.label}, ${s.start_sec.toFixed(1)}–${s.end_sec.toFixed(1)} с`);

      const nameEl = document.createElement("div");
      nameEl.className = "name";
      const fullEl = document.createElement("span");
      fullEl.className = "name-full";
      fullEl.textContent = s.label;
      const shortEl = document.createElement("span");
      shortEl.className = "name-short";
      shortEl.textContent = SHORT_SECTION_LABELS[s.section] || s.label;
      nameEl.appendChild(fullEl);
      nameEl.appendChild(shortEl);

      const timeEl = document.createElement("div");
      timeEl.className = "time";
      timeEl.textContent = `${s.start_sec.toFixed(1)}–${s.end_sec.toFixed(1)} с`;

      blk.appendChild(nameEl);
      blk.appendChild(timeEl);
      blk.onclick = (ev) => {
        if (swallowClick) {
          swallowClick = false;
          return;
        }
        if (ev && ev.shiftKey) addMark(timelineTime(blk, s, ev));
        else Sampler.seek(s.start_sec, true);
      };
      blk.onpointerdown = (ev) => {
        swallowClick = false;
        clearTimeout(pressTimer);
        pressTimer = null;
        if (!ev || ev.pointerType !== "touch") return;
        pressTimer = setTimeout(() => {
          pressTimer = null;
          swallowClick = true;
          addMark(timelineTime(blk, s, ev));
        }, LONG_PRESS_MS);
      };
      blk.onpointerup = () => {
        clearTimeout(pressTimer);
        pressTimer = null;
      };
      blk.onpointercancel = blk.onpointerup;
      blk.oncontextmenu = (ev) => {
        if (swallowClick || pressTimer) ev.preventDefault();
      };
      tl.appendChild(blk);
    }
    playerCard.appendChild(tl);

    const markBar = document.createElement("div");
    markBar.className = "row mark-bar";
    const markBtn = document.createElement("button");
    markBtn.id = "markBtn";
    markBtn.textContent = "Тут не так";
    markBtn.title = "Отметить место (клавиша M)";
    markBtn.setAttribute("aria-keyshortcuts", "M");
    markBtn.onclick = () => addMark(Sampler.currentTime());
    markBar.appendChild(markBtn);
    playerCard.appendChild(markBar);

    const markTrack = document.createElement("div");
    markTrack.id = "markTrack";
    markTrack.className = "mark-track";
    playerCard.appendChild(markTrack);

    const marksBox = document.createElement("div");
    marksBox.className = "marks-box";
    const marksTitle = document.createElement("div");
    marksTitle.id = "marksTitle";
    marksTitle.className = "marks-title";
    const editorHost = document.createElement("div");
    editorHost.id = "markEditorHost";
    const markList = document.createElement("div");
    markList.id = "markList";
    markList.className = "mark-list";
    const marksHint = document.createElement("div");
    marksHint.id = "marksHint";
    marksHint.className = "marks-hint";
    marksHint.textContent = MARK_HINT;
    const marksDl = document.createElement("button");
    marksDl.id = "marksDlBtn";
    marksDl.textContent = "Скачать отметки";
    marksDl.onclick = () => exportMarksOf(d.id);
    marksBox.appendChild(marksTitle);
    marksBox.appendChild(editorHost);
    marksBox.appendChild(markList);
    marksBox.appendChild(marksHint);
    marksBox.appendChild(marksDl);
    playerCard.appendChild(marksBox);
    main.appendChild(playerCard);
    renderMarks();

    updatePlayButton();
    syncTransportUI(Sampler.currentTime());

    // Whole piece card
    const wholeCard = document.createElement("div");
    wholeCard.className = "card";
    const wholeH2 = document.createElement("h2");
    wholeH2.textContent = "Вся пьеса";
    const wholeRow = document.createElement("div");
    wholeRow.id = "wholeRow";
    wholeRow.className = "row";
    for (const l of d.whole_layers) {
      wholeRow.appendChild(makeLayerControl(d.id, l));
    }
    const allBtn = document.createElement("button");
    allBtn.id = "rerollAllBtn";
    allBtn.className = "primary";
    allBtn.textContent = "Перегенерировать всё незакреплённое";
    allBtn.onclick = () =>
      runMutation(
        "reroll",
        { layer: "all", section: "all" },
        d.id,
        (snap) => ({
          action: "reroll",
          layer: "all",
          section: "all",
          salt: { ...(snap.state.recipe.studio.salts || {}) },
        }),
        allBtn
      );
    wholeRow.appendChild(allBtn);
    wholeCard.appendChild(wholeH2);
    wholeCard.appendChild(wholeRow);
    main.appendChild(wholeCard);

    // Sections card
    const secCard = document.createElement("div");
    secCard.className = "card";
    const secH2 = document.createElement("h2");
    secH2.textContent = "Секции";
    const sGrid = document.createElement("div");
    sGrid.id = "sectionsGrid";
    sGrid.className = "sections-grid";
    for (const s of d.sections) {
      if (!s.layers || s.layers.length === 0) continue;
      const box = document.createElement("div");
      box.className = "sec-card";
      const head = document.createElement("div");
      head.className = "row";
      head.style.justifyContent = "space-between";
      const sStrong = document.createElement("strong");
      sStrong.textContent = s.label;
      const sMeta = document.createElement("span");
      sMeta.className = "piece-meta";
      sMeta.textContent = `${fmtSec(s.start_sec)} – ${fmtSec(s.end_sec)}`;
      head.appendChild(sStrong);
      head.appendChild(sMeta);
      box.appendChild(head);
      for (const l of s.layers) {
        box.appendChild(makeLayerControl(d.id, l));
      }
      sGrid.appendChild(box);
    }
    secCard.appendChild(secH2);
    secCard.appendChild(sGrid);
    main.appendChild(secCard);

    // Options card
    const optCard = document.createElement("div");
    optCard.className = "card";
    const optH2 = document.createElement("h2");
    optH2.textContent = "Параметры пьесы";
    const optNote = document.createElement("div");
    optNote.className = "params-note";
    optNote.textContent = "Параметры действуют на всю пьесу, в том числе на закреплённые секции.";
    const oGrid = document.createElement("div");
    oGrid.id = "optionsGrid";
    oGrid.className = "options-grid";
    const advDetails = document.createElement("details");
    advDetails.className = "advanced-options";
    const advSummary = document.createElement("summary");
    advSummary.textContent = "Дополнительно";
    const advNote = document.createElement("div");
    advNote.className = "advanced-note";
    advNote.textContent =
      "Эти параметры действуют только при следующей перегенерации ритм-гитары («Перегенерировать ритм-гитару» или «Перегенерировать всё незакреплённое»).";
    const advGrid = document.createElement("div");
    advGrid.id = "advancedGrid";
    advGrid.className = "options-grid";
    advDetails.appendChild(advSummary);
    advDetails.appendChild(advNote);
    advDetails.appendChild(advGrid);
    const opts = d.options;

    function addOptionHint(parent, text) {
      const h = document.createElement("div");
      h.className = "option-hint";
      h.textContent = text;
      parent.appendChild(h);
    }

    function sendOption(name, value, ctl) {
      return runMutation(
        "option",
        { name, value },
        d.id,
        (snap) => ({ action: "option", value: { [name]: snap.detail.options[name] } }),
        ctl
      );
    }

    function bindSliderCommit(inp, onMove, commit) {
      let timer = null;
      let dragging = false;
      let dirty = false;
      const cancel = () => {
        clearTimeout(timer);
        timer = null;
        dirty = false;
        dropPendingSlider(flush);
      };
      const flush = () => {
        clearTimeout(timer);
        timer = null;
        dropPendingSlider(flush);
        if (!dirty) return undefined;
        dirty = false;
        return commit();
      };
      inp.oninput = () => {
        dirty = true;
        if (!appState.pendingSliders.includes(flush)) appState.pendingSliders.push(flush);
        onMove();
        clearTimeout(timer);
        timer = dragging ? null : setTimeout(flush, SLIDER_DEBOUNCE_MS);
      };
      inp.onpointerdown = () => {
        dragging = true;
        clearTimeout(timer);
        timer = null;
      };
      inp.onpointerup = () => {
        dragging = false;
        flush();
      };
      inp.onpointercancel = inp.onpointerup;
      inp.onchange = () => {
        if (!dragging) return;
        dragging = false;
        flush();
      };
      return cancel;
    }

    function makeSliderInput(name, label, min, max, step, value) {
      const inp = document.createElement("input");
      inp.type = "range";
      inp.dataset.option = name;
      inp.min = String(min);
      inp.max = String(max);
      inp.step = String(step);
      inp.value = String(value);
      inp.setAttribute("aria-label", label);
      return inp;
    }

    function addSliderControl(grid, name, label, min, max, step, hint) {
      const wrap = document.createElement("div");
      wrap.className = "slider-row";
      const valText = document.createElement("span");
      valText.textContent = String(opts[name] ?? 0);
      const head = document.createElement("div");
      head.className = "slider-head";
      const labelSpan = document.createElement("span");
      labelSpan.textContent = label;
      head.appendChild(labelSpan);
      head.appendChild(valText);
      const inp = makeSliderInput(name, label, min, max, step, opts[name] ?? 0);
      inp.setAttribute("aria-valuetext", valText.textContent);
      bindSliderCommit(
        inp,
        () => {
          valText.textContent = inp.value;
          inp.setAttribute("aria-valuetext", inp.value);
        },
        () => sendOption(name, step === 1 ? parseInt(inp.value, 10) : parseFloat(inp.value), inp)
      );
      wrap.appendChild(head);
      wrap.appendChild(inp);
      if (hint) addOptionHint(wrap, hint);
      grid.appendChild(wrap);
    }

    function makeChoiceRow(label, name, choices, current, pick, force) {
      const row = document.createElement("div");
      row.className = "row";
      row.setAttribute("role", "group");
      row.setAttribute("aria-label", label);
      for (const [val, txt] of choices) {
        const selected = current === val;
        const b = document.createElement("button");
        b.className = selected ? "active" : "";
        b.dataset.option = name;
        b.dataset.value = String(val);
        b.textContent = txt;
        b.setAttribute("aria-pressed", selected ? "true" : "false");
        b.onclick = () => {
          if (!selected || (force && force())) pick(val, b);
        };
        row.appendChild(b);
      }
      return row;
    }

    function addChoiceGroup(grid, title, name, choices, current, hint) {
      const box = document.createElement("div");
      const titleEl = document.createElement("div");
      titleEl.className = "piece-meta";
      titleEl.style.marginBottom = "6px";
      titleEl.textContent = title;
      box.appendChild(titleEl);
      box.appendChild(makeChoiceRow(title, name, choices, current, (val, b) => sendOption(name, val, b)));
      if (hint) addOptionHint(box, hint);
      grid.appendChild(box);
    }

    function pendingKey() {
      return appState.keyPending && appState.keyPending.id === d.id ? appState.keyPending.value : null;
    }

    function trackKey(value, request) {
      appState.keyPending = { id: d.id, value };
      appState.keyBusy = (appState.keyBusy || 0) + 1;
      return Promise.resolve(request()).finally(() => {
        if (--appState.keyBusy === 0) appState.keyPending = null;
      });
    }

    function sendKey(patch, ctl) {
      const pending = pendingKey();
      const next = Object.assign({}, pending || { tonic: d.key, mode: d.mode }, patch);
      return trackKey(next, () => sendOption("key", next, ctl));
    }

    function resetKey(ctl) {
      return trackKey(d.own_key || { tonic: d.key, mode: d.mode }, () => sendOption("key", null, ctl));
    }

    function addKeyControl(grid) {
      const title = "Тональность";
      const box = document.createElement("div");
      const titleEl = document.createElement("div");
      titleEl.className = "piece-meta";
      titleEl.style.marginBottom = "6px";
      titleEl.textContent = title;
      box.appendChild(titleEl);
      box.appendChild(
        makeChoiceRow(`${title}: основной тон`, "key_tonic", KEY_TONICS.map((t) => [t, t]), d.key, (t, b) =>
          sendKey({ tonic: t }, b)
        )
      );
      box.appendChild(
        makeChoiceRow(`${title}: лад`, "key_mode", KEY_MODES, d.mode, (m, b) => sendKey({ mode: m }, b))
      );
      box.appendChild(
        makeChoiceRow(`${title}: как в пьесе`, "key", [["auto", "Как в пьесе"]], opts.key ? null : "auto", (v, b) =>
          resetKey(b), pendingKey
        )
      );
      addOptionHint(box, "Переводит все ноты в выбранную тональность по ступеням; ритм и форма не меняются. Применяется сразу.");
      grid.appendChild(box);
    }

    addSliderControl(
      oGrid,
      "mute_share",
      "Глухие ноты (доля ударов ритм-гитары)",
      0,
      1,
      0.05,
      "Глушит долю ударов ритм-гитары в секциях A, A2, «Кульминация» и B; применяется сразу."
    );

    {
      const label = "Октавные аккорды (доля тактов)";
      const wrap = document.createElement("div");
      wrap.className = "slider-row";
      let auto = opts.octave_share === null || opts.octave_share === undefined;
      const valText = document.createElement("span");
      valText.textContent = auto ? "авто" : String(opts.octave_share);
      const head = document.createElement("div");
      head.className = "slider-head";
      const lbl = document.createElement("span");
      lbl.textContent = label;
      head.appendChild(lbl);
      head.appendChild(valText);
      const row = document.createElement("div");
      row.className = "row";
      row.setAttribute("role", "group");
      row.setAttribute("aria-label", label);
      const autoBtn = document.createElement("button");
      autoBtn.dataset.option = "octave_share";
      autoBtn.dataset.value = "auto";
      autoBtn.textContent = "Авто";
      const manualBtn = document.createElement("button");
      manualBtn.dataset.option = "octave_share";
      manualBtn.dataset.value = "manual";
      manualBtn.textContent = "Вручную";
      const inp = makeSliderInput("octave_share", label, 0, 1, 0.05, auto ? 0.5 : opts.octave_share);
      inp.style.flex = "1 1 120px";
      const paint = () => {
        autoBtn.className = auto ? "active" : "";
        manualBtn.className = auto ? "" : "active";
        autoBtn.setAttribute("aria-pressed", auto ? "true" : "false");
        manualBtn.setAttribute("aria-pressed", auto ? "false" : "true");
        inp.className = auto ? "slider-dim" : "";
        valText.textContent = auto ? "авто" : inp.value;
        inp.setAttribute("aria-valuetext", valText.textContent);
      };
      paint();
      const cancelSlider = bindSliderCommit(
        inp,
        () => {
          auto = false;
          paint();
        },
        () => sendOption("octave_share", parseFloat(inp.value), inp)
      );
      autoBtn.onclick = () => {
        if (auto) return;
        cancelSlider();
        sendOption("octave_share", null, autoBtn);
      };
      manualBtn.onclick = () => {
        if (!auto) return;
        sendOption("octave_share", parseFloat(inp.value), manualBtn);
      };
      row.appendChild(autoBtn);
      row.appendChild(manualBtn);
      row.appendChild(inp);
      wrap.appendChild(head);
      wrap.appendChild(row);
      addOptionHint(
        wrap,
        "Доля тактов A и A2 с октавами. «Авто» — как сложилось в пьесе; если сдвинуть ползунок, включится «Вручную». Применяется сразу."
      );
      oGrid.appendChild(wrap);
    }

    addChoiceGroup(
      oGrid,
      "Длина вступления (тактов)",
      "intro_bars",
      [["auto", "Авто"], [1, "1"], [2, "2"], [4, "4"], [8, "8"]],
      opts.intro_bars ?? "auto",
      "Меняет число тактов вступления по аккордам A; применяется сразу."
    );
    addChoiceGroup(
      oGrid,
      "Повтор квадрата в куплетах",
      "square_repeat",
      [[1, "1× (4 такта)"], [2, "2× (8 тактов)"]],
      opts.square_repeat ?? 1,
      "Повторяет 4-тактовый квадрат куплетов A и A2 дважды подряд; применяется сразу."
    );
    addKeyControl(oGrid);
    addChoiceGroup(
      oGrid,
      "Соло в середине B",
      "b_lead",
      [
        ["augmented", "Медленнее (удвоенные длительности)"],
        ["same_speed", "В темпе рифа"],
        ["rest", "Без соло"],
      ],
      opts.b_lead
    );
    addChoiceGroup(
      oGrid,
      "Конец такта в рифе",
      "trailing_rest",
      [[true, "Пауза в конце такта"], [false, "Без паузы"]],
      Boolean(opts.trailing_rest)
    );

    for (const [name, label, min, max, step] of [
      ["octave_piece", "Вероятность октав при перегенерации ритма", 0, 1, 0.05],
      ["octave_sixteenths", "Октавы шестнадцатыми (при перегенерации)", 0, 1, 0.05],
      ["dead_notes", "Глухие удары перед акцентами (только в октавных тактах, при перегенерации)", 0, 1, 0.05],
      ["slide_max", "Макс. слайд октав в ладах (при перегенерации)", 0, 12, 1],
    ]) {
      addSliderControl(advGrid, name, label, min, max, step, null);
    }

    optCard.appendChild(optH2);
    optCard.appendChild(optNote);
    optCard.appendChild(oGrid);
    optCard.appendChild(advDetails);
    main.appendChild(optCard);
  }

  // ------------------------------------------------------------------ queues & actions
  const pieceQueues = new Map();

  function enqueueForPiece(pieceId, fn) {
    const key = pieceId || "__global__";
    const prev = pieceQueues.get(key) || Promise.resolve();
    const next = prev.catch(() => {}).then(fn);
    const tail = next.catch(() => {});
    pieceQueues.set(key, tail);
    tail.finally(() => {
      if (pieceQueues.get(key) === tail) pieceQueues.delete(key);
    });
    return next;
  }

  async function waitForPieceQueue(pieceId) {
    const p = pieceQueues.get(pieceId);
    if (p) await p;
  }

  async function waitForAllQueues() {
    while (pieceQueues.size > 0) {
      await Promise.all(Array.from(pieceQueues.values()));
    }
  }

  function applyDisplayedState(snap, preserveAudioPos) {
    appState.currentId = snap.state.id;
    appState.currentState = snap.state;
    appState.current = snap.detail;
    appState.currentEvents = snap.events;
    Sampler.setPiece(snap.events, snap.detail.length, preserveAudioPos);
  }

  function commitDisplayedPiece(snap, preserveAudioPos = false) {
    if (appState.currentId !== snap.state.id) {
      appState.errorText = "";
      appState.markEdit = null;
    }
    appState.navPending = false;
    appState.pendingNavId = null;
    applyDisplayedState(snap, preserveAudioPos);
    renderList();
    renderWorkspace();
  }

  async function runMutation(cmd, payloadArgs, pieceId, logBuilder, ctl) {
    if (!pieceId) return;
    if (appState.navPending && appState.pendingNavId !== pieceId) {
      return;
    }
    if (ctl) ctl.disabled = true;
    try {
      if (appState.pendingSliders.length) await flushPendingSliders();
      await queueMutation(cmd, payloadArgs, pieceId, logBuilder);
    } finally {
      if (ctl) ctl.disabled = false;
    }
  }

  function queueMutation(cmd, payloadArgs, pieceId, logBuilder) {
    return enqueueForPiece(pieceId, async () => {
      if (appState.navPending && appState.pendingNavId !== pieceId) {
        return;
      }
      const stored = await Storage.getPiece(pieceId);
      const latestState = (stored && stored.state) || (appState.currentId === pieceId ? appState.currentState : null);
      if (!latestState) return;

      const tracked = appState.currentId === pieceId;
      if (tracked) setChip(1);
      let resp;
      try {
        resp = await callPy(cmd, { ...payloadArgs, state: latestState });
      } catch (err) {
        showError(err, pieceId);
        if (appState.currentId === pieceId && !appState.navPending) renderWorkspace();
        return;
      } finally {
        if (tracked) setChip(-1);
      }
      const { result: snap, elapsedMs } = resp;
      if (samePieceState(latestState, snap.state)) {
        if (appState.currentId === pieceId && !appState.navPending) {
          renderWorkspace();
        }
        return;
      }
      if (cmd === "reroll") {
        debugInfo.lastRerollMs = elapsedMs;
      }
      const rec = {
        id: snap.state.id,
        state: snap.state,
        detail: snap.detail,
        updatedAt: new Date().toISOString(),
      };
      const logEntry = logBuilder ? { ...logBuilder(snap), piece_id: pieceId } : null;
      await Storage.savePieceAndLog(rec, logEntry);
      appState.pieces = await Storage.listPieces();

      if (appState.currentId === pieceId && !appState.navPending) {
        commitDisplayedPiece(snap, true);
      } else {
        if (appState.currentId === pieceId) applyDisplayedState(snap, true);
        renderList();
      }
    });
  }

  // ------------------------------------------------------------------ marks
  function markSummary(m) {
    const who = MARK_GUITAR_LABELS[m.guitar] || m.guitar;
    return `${m.bar}:${m.sixteenth} · ${who}${m.text ? ` · ${m.text}` : ""}${m.stale ? " (до изменения)" : ""}`;
  }

  function findMark(id) {
    return ((appState.current && appState.current.marks) || []).find((m) => m.id === id) || null;
  }

  function markRequest(cmd, args, onDone) {
    const pieceId = appState.currentId;
    if (!pieceId) return Promise.resolve(null);
    return enqueueForPiece(pieceId, async () => {
      const stored = await Storage.getPiece(pieceId);
      const latest = (stored && stored.state) || (appState.currentId === pieceId ? appState.currentState : null);
      if (!latest) return null;
      let resp;
      try {
        resp = await callPy(cmd, { ...args, state: latest });
      } catch (err) {
        showError(err, pieceId);
        return null;
      }
      const { state, detail, mark } = resp.result;
      if (!samePieceState(latest, state)) {
        const rec = { id: state.id, state, detail, updatedAt: stored ? stored.updatedAt : new Date().toISOString() };
        await Storage.savePieceAndLog(rec, { action: cmd, value: mark, piece_id: pieceId });
      }
      if (appState.currentId === pieceId) {
        appState.currentState = state;
        appState.current = detail;
        if (onDone) onDone(mark);
        if (!appState.navPending) renderMarks();
      }
      return mark;
    });
  }

  function addMark(t) {
    if (!appState.currentId || appState.navPending || !appState.current) return Promise.resolve(null);
    const at = Math.round(Math.max(0, Math.min(appState.current.length, Number(t) || 0)) * 1000) / 1000;
    return finishMarkEdit(true).then(() =>
      markRequest("mark_add", { t: at }, (mark) => {
        appState.markEdit = { id: mark.id, fresh: true, guitar: mark.guitar, text: mark.text };
      })
    );
  }

  async function finishMarkEdit(commit) {
    const ed = appState.markEdit;
    if (!ed) return;
    const text = ed.input ? ed.input.value : ed.text;
    const mark = findMark(ed.id);
    appState.markEdit = null;
    if (commit && mark && (ed.guitar !== mark.guitar || text !== mark.text)) {
      await markRequest("mark_edit", { id: ed.id, guitar: ed.guitar, text });
    } else {
      renderMarks();
    }
  }

  function dropMark(id) {
    appState.markEdit = null;
    return markRequest("mark_delete", { id });
  }

  function cancelMarkEdit() {
    const ed = appState.markEdit;
    if (!ed) return Promise.resolve();
    if (ed.fresh) return dropMark(ed.id);
    appState.markEdit = null;
    renderMarks();
    return Promise.resolve();
  }

  function openMark(m) {
    Sampler.seek(m.from_sec, false);
    appState.markEdit = { id: m.id, fresh: false, guitar: m.guitar, text: m.text };
    renderMarks();
  }

  function timelineTime(blk, s, ev) {
    const r = blk.getBoundingClientRect ? blk.getBoundingClientRect() : null;
    if (!r || !r.width || !ev || typeof ev.clientX !== "number") return s.start_sec;
    const frac = Math.max(0, Math.min(1, (ev.clientX - r.left) / r.width));
    return s.start_sec + frac * (s.end_sec - s.start_sec);
  }

  function makeMarkEditor(m) {
    const ed = appState.markEdit;
    const box = document.createElement("div");
    box.id = "markEditor";
    box.className = "mark-editor";
    box.setAttribute("role", "group");
    box.setAttribute("aria-label", `Отметка ${m.bar}:${m.sixteenth}`);
    const guitars = [];
    for (const [val, txt] of MARK_GUITARS) {
      const b = document.createElement("button");
      b.dataset.markGuitar = val;
      b.textContent = txt;
      guitars.push([val, b]);
      b.onclick = () => {
        ed.guitar = val;
        for (const [v, other] of guitars) {
          other.className = v === val ? "active" : "";
          other.setAttribute("aria-pressed", v === val ? "true" : "false");
        }
      };
      box.appendChild(b);
    }
    for (const [v, b] of guitars) {
      b.className = v === ed.guitar ? "active" : "";
      b.setAttribute("aria-pressed", v === ed.guitar ? "true" : "false");
    }
    const input = document.createElement("input");
    input.id = "markText";
    input.type = "text";
    input.value = ed.text;
    input.setAttribute("maxlength", String(MARK_TEXT_MAX));
    input.setAttribute("aria-label", "Что не так");
    input.setAttribute("placeholder", "Что не так (необязательно)");
    ed.input = input;
    input.oninput = () => {
      ed.text = input.value;
    };
    input.onkeydown = (ev) => {
      if (ev.key === "Enter") finishMarkEdit(true);
    };
    const done = document.createElement("button");
    done.id = "markDone";
    done.className = "primary";
    done.textContent = "Готово";
    done.onclick = () => finishMarkEdit(true);
    const del = document.createElement("button");
    del.id = "markDelete";
    del.textContent = "Удалить";
    del.onclick = () => dropMark(m.id);
    box.appendChild(input);
    box.appendChild(done);
    box.appendChild(del);
    return box;
  }

  function renderMarks() {
    const d = appState.current;
    const title = document.getElementById("marksTitle");
    if (!d || !title) return;
    const marks = d.marks || [];
    const total = Math.max(1, d.length);
    title.textContent = `Отметки (${marks.length})`;

    const track = document.getElementById("markTrack");
    track.textContent = "";
    const list = document.getElementById("markList");
    list.textContent = "";
    for (const m of marks) {
      const flag = document.createElement("button");
      flag.className = `mark-flag${m.stale ? " stale" : ""}`;
      flag.dataset.markId = m.id;
      flag.style.left = `${Number(((m.t / total) * 100).toFixed(2))}%`;
      flag.title = markSummary(m);
      flag.setAttribute("aria-label", `Отметка ${markSummary(m)}`);
      flag.onclick = () => openMark(m);
      track.appendChild(flag);

      const row = document.createElement("div");
      row.className = `mark-row${m.stale ? " stale" : ""}`;
      row.dataset.markId = m.id;
      const open = document.createElement("button");
      open.className = "mark-open";
      open.textContent = markSummary(m);
      open.onclick = () => openMark(m);
      const listen = document.createElement("button");
      listen.className = "mark-audition";
      listen.textContent = "Послушать место";
      listen.onclick = () => Sampler.playRange(m.from_sec, m.to_sec);
      row.appendChild(open);
      row.appendChild(listen);
      list.appendChild(row);
    }

    const host = document.getElementById("markEditorHost");
    host.textContent = "";
    const editing = appState.markEdit && findMark(appState.markEdit.id);
    if (editing) host.appendChild(makeMarkEditor(editing));
    else appState.markEdit = null;
  }

  function exportMarksOf(pieceId) {
    return enqueueForPiece(pieceId, async () => {
      const stored = await Storage.getPiece(pieceId);
      const st = (stored && stored.state) || appState.currentState;
      if (!st) return;
      const { result } = await callPy("export_marks", { state: st });
      downloadBlob(`${pieceId}.marks.json`, "application/json;charset=utf-8", JSON.stringify(result, null, 2));
    }).catch((err) => showError(err, pieceId));
  }

  async function exportAllMarks() {
    try {
      await waitForAllQueues();
      const out = [];
      for (const p of await Storage.listPieces()) {
        const { result } = await callPy("export_marks", { state: p.state });
        out.push(result);
      }
      const payload = { version: 1, exported_at: new Date().toISOString(), pieces: out };
      downloadBlob("marks.json", "application/json;charset=utf-8", JSON.stringify(payload, null, 2));
    } catch (err) {
      showError(err);
    }
  }

  function isTextTarget(t) {
    const tag = t && t.tagName ? String(t.tagName).toUpperCase() : "";
    return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || Boolean(t && t.isContentEditable);
  }

  document.addEventListener("keydown", (ev) => {
    if (ev.key === "Escape") {
      if (appState.markEdit) cancelMarkEdit();
      return;
    }
    if (ev.ctrlKey || ev.metaKey || ev.altKey || ev.repeat) return;
    if (ev.code !== "KeyM" && String(ev.key).toLowerCase() !== "m") return;
    if (isTextTarget(ev.target) || !appState.current) return;
    ev.preventDefault();
    addMark(Sampler.currentTime());
  });

  async function selectPiece(id) {
    if (appState.pendingSliders.length) await flushPendingSliders();
    if (appState.currentId && pieceQueues.has(appState.currentId)) await waitForPieceQueue(appState.currentId);
    const seq = ++appState.navSeq;
    appState.navPending = true;
    appState.pendingNavId = id;
    setWorkspaceDisabled(true);

    await waitForPieceQueue(id);
    if (seq !== appState.navSeq) return;

    const stored = await Storage.getPiece(id);
    if (seq !== appState.navSeq) return;
    if (!stored) {
      appState.navPending = false;
      appState.pendingNavId = null;
      setWorkspaceDisabled(false);
      return;
    }

    let resp;
    try {
      resp = await callPy("snapshot", { state: stored.state });
    } catch (err) {
      if (seq === appState.navSeq) {
        appState.navPending = false;
        appState.pendingNavId = null;
        showError(err, id);
        renderList();
        renderWorkspace();
      }
      return;
    }
    if (seq !== appState.navSeq) return;
    commitDisplayedPiece(resp.result, false);
  }

  async function createNewPiece() {
    const raw = document.getElementById("seedInput").value.trim();
    const seed = raw === "" ? null : parseInt(raw, 10);
    if (appState.pendingSliders.length) await flushPendingSliders();
    const seq = ++appState.navSeq;
    appState.navPending = true;
    appState.pendingNavId = null;
    setWorkspaceDisabled(true);

    let resp;
    try {
      resp = await callPy("new_piece", { seed });
    } catch (err) {
      if (seq === appState.navSeq) {
        appState.navPending = false;
        appState.pendingNavId = null;
        setWorkspaceDisabled(false);
      }
      showError(err);
      return;
    }

    const { snap } = await Storage.insertUniquePieceAndLog(resp.result, (pieceId) => ({
      action: "new",
      value: resp.result.detail.seed,
      piece_id: pieceId,
    }));
    appState.pieces = await Storage.listPieces();
    if (seq === appState.navSeq) {
      commitDisplayedPiece(snap, false);
    } else {
      renderList();
    }
  }

  async function downloadHistoryLog() {
    await waitForAllQueues();
    const rows = await Storage.getAllLogs();
    const text = rows.map((r) => JSON.stringify(r)).join("\n") + (rows.length ? "\n" : "");
    downloadBlob("history.jsonl", "application/x-ndjson;charset=utf-8", text);
  }

  async function exportAllPieces() {
    await waitForAllQueues();
    const pieces = await Storage.listPieces();
    const payload = {
      version: 1,
      exported_at: new Date().toISOString(),
      pieces: pieces.map((p) => ({ id: p.id, state: p.state, updatedAt: p.updatedAt })),
    };
    downloadBlob("griff-studio-pieces.json", "application/json;charset=utf-8", JSON.stringify(payload, null, 2));
  }

  async function importPiecesFile(file) {
    if (!file) return;
    try {
      await waitForAllQueues();
      const parsed = JSON.parse(await file.text());
      const items = Array.isArray(parsed) ? parsed : parsed.pieces;
      if (!Array.isArray(items)) throw new Error("Неверный формат файла пьес");
      let firstId = null;
      for (const item of items) {
        const st = item.state || item;
        if (!st || !st.recipe) continue;
        const { result: snap } = await callPy("import_state", { state: st });
        const { id: importedId } = await Storage.importUniquePiece(snap, item.updatedAt);
        if (!firstId) firstId = importedId;
      }
      appState.pieces = await Storage.listPieces();
      renderList();
      if (firstId) await selectPiece(firstId);
    } catch (err) {
      showError(err);
    }
  }

  debugInfo.sampler = Sampler;
  debugInfo.callPy = callPy;
  debugInfo.getCurrentId = () => appState.currentId;
  debugInfo.setPieceForTest = (snap) => {
    commitDisplayedPiece(snap, false);
  };

  document.getElementById("newBtn").onclick = createNewPiece;
  document.getElementById("errorCloseBtn").onclick = () => {
    appState.errorText = "";
    paintError();
  };
  document.getElementById("logBtn").onclick = downloadHistoryLog;
  document.getElementById("exportPiecesBtn").onclick = exportAllPieces;
  document.getElementById("exportMarksAllBtn").onclick = exportAllMarks;
  const importInput = document.getElementById("importPiecesInput");
  document.getElementById("importPiecesBtn").onclick = () => importInput.click();
  importInput.onchange = async () => {
    const f = importInput.files && importInput.files[0];
    importInput.value = "";
    await importPiecesFile(f);
  };

  Sampler.preload();
  Storage.listPieces().then((pieces) => {
    appState.pieces = pieces;
    renderList();
    if (pieces.length > 0 && !appState.current && !appState.currentId && !appState.navPending) {
      selectPiece(pieces[0].id);
    }
  });
})();
