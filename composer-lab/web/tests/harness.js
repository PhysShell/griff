import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WEB_DIR = path.resolve(__dirname, "..");

function deepClone(v) {
  return v === undefined ? undefined : JSON.parse(JSON.stringify(v));
}

function fakeRev(rec) {
  return JSON.stringify((rec.studio && rec.studio.salts) || {});
}

function keepMarks(st) {
  return Array.isArray(st.marks) ? { marks: st.marks } : {};
}

function makeFakeState(seed = 801000, overrides = {}) {
  const recipe = {
    seed,
    intent: {
      meter: "4/4",
      grouping: [4, 4, 4, 4],
      mode: "lydian",
      tonic: "E",
      tempo: 140,
      b_meter: "4/4",
      b_grouping: [4, 4, 4, 4],
      lead: "g1",
      intro_bars: 1,
    },
    form: [
      { sec: "intro", chord: 0, prog: "intro" },
      { sec: "A", chord: 0, prog: "stmt", role: "chords" },
      { sec: "B", chord: 3, prog: "b1" },
      { sec: "end", chord: 0, prog: "end" },
    ],
    studio: {
      salts: {},
      locks: [],
      options: {
        b_lead: "augmented",
        trailing_rest: true,
        mute_share: 0,
        octave_share: null,
        intro_bars: "auto",
        square_repeat: 1,
        octave_piece: 0.25,
        octave_sixteenths: 0.3,
        dead_notes: 0.2,
        slide_max: 5,
        key: null,
      },
      comp_doses: {},
    },
  };
  return {
    id: `piece_${seed}`,
    recipe,
    undo: [],
    redo: [],
    ...overrides,
  };
}

function describeFakeChange(prevRec, curRec) {
  const po = (prevRec && prevRec.studio && prevRec.studio.options) || {};
  const co = (curRec && curRec.studio && curRec.studio.options) || {};
  const fmtNum = (v) => String(Number(v));
  for (const k of [
    "mute_share",
    "octave_share",
    "intro_bars",
    "square_repeat",
    "b_lead",
    "trailing_rest",
    "octave_piece",
    "octave_sixteenths",
    "dead_notes",
    "slide_max",
    "key",
  ]) {
    if (JSON.stringify(po[k] ?? null) !== JSON.stringify(co[k] ?? null)) {
      const v = co[k];
      if (k === "key") {
        const modes = { ionian: "мажор", dorian: "дорийский", phrygian: "фригийский", lydian: "лидийский", mixolydian: "миксолидийский", aeolian: "натуральный минор", locrian: "локрийский" };
        return v ? `тональность: ${v.tonic} ${modes[v.mode]}` : "тональность: как в пьесе";
      }
      if (k === "mute_share") return `глухие ноты ${fmtNum(v)}`;
      if (k === "octave_share") return v === null || v === undefined ? "октавные аккорды: авто" : `октавные аккорды ${fmtNum(v)}`;
      if (k === "intro_bars") return v === "auto" ? "длина вступления: авто" : `длина вступления: ${v}`;
      if (k === "square_repeat") return `повтор квадрата: ${v}×`;
      if (k === "b_lead") {
        return {
          augmented: "соло в середине B: медленнее",
          same_speed: "соло в середине B: в темпе рифа",
          rest: "соло в середине B: без соло",
        }[v] || `соло в середине B: ${v}`;
      }
      if (k === "trailing_rest") return v ? "пауза в конце такта" : "без паузы в конце такта";
      if (k === "octave_piece") return `вероятность октав ${fmtNum(v)}`;
      if (k === "octave_sixteenths") return `октавы шестнадцатыми ${fmtNum(v)}`;
      if (k === "dead_notes") return `глухие перед акцентами ${fmtNum(v)}`;
      if (k === "slide_max") return `макс. слайд октав ${v}`;
    }
  }
  const secLabels = {
    intro: "Вступление",
    A: "Куплет A",
    A2: "Куплет A2",
    B: "Середина B",
    A3: "Кульминация",
    end: "Концовка",
  };
  const layerLabels = {
    comp: "ритм-гитара",
    texture: "фактура",
    lead_rhythm: "риф",
    b: "вариант середины",
    intro: "вступление",
    ending: "концовка",
    tempo: "темп",
  };
  const formatTarget = (layer, section) => {
    if (section === "all") return `«${layerLabels[layer] || layer}»`;
    if ((section === "intro" && layer === "intro") || (section === "end" && layer === "ending")) {
      return `«${secLabels[section] || section}»`;
    }
    return `«${secLabels[section] || section} · ${layerLabels[layer] || layer}»`;
  };
  const pl = new Set((prevRec && prevRec.studio && prevRec.studio.locks) || []);
  const cl = new Set((curRec && curRec.studio && curRec.studio.locks) || []);
  const added = [...cl].filter((k) => !pl.has(k));
  const removed = [...pl].filter((k) => !cl.has(k));
  if (added.length === 1 && removed.length === 0) {
    const [l, s] = added[0].split("@");
    return `закрепление ${formatTarget(l, s)}`;
  }
  if (removed.length === 1 && added.length === 0) {
    const [l, s] = removed[0].split("@");
    return `открепление ${formatTarget(l, s)}`;
  }
  const ps = (prevRec && prevRec.studio && prevRec.studio.salts) || {};
  const cs = (curRec && curRec.studio && curRec.studio.salts) || {};
  const changedSalts = Object.keys({ ...ps, ...cs }).filter((k) => Number(ps[k] || 0) !== Number(cs[k] || 0));
  if (changedSalts.length > 1) return "перегенерация всего незакреплённого";
  if (changedSalts.length === 1) {
    const [l, s] = changedSalts[0].split("@");
    return `перегенерация ${formatTarget(l, s)}`;
  }
  return "последнее изменение";
}

function buildDetailFromState(st, sectionLabels = {}) {
  const rec = st.recipe;
  const salts = (rec.studio && rec.studio.salts) || {};
  const locks = new Set((rec.studio && rec.studio.locks) || []);
  const opts = (rec.studio && rec.studio.options) || {};
  const defaultLabels = {
    intro: "Вступление",
    A: "Куплет A",
    A2: "Куплет A2",
    B: "Середина B",
    A3: "Кульминация",
    end: "Концовка",
  };
  const secsSeen = [];
  for (const bar of rec.form || []) {
    if (!secsSeen.includes(bar.sec)) secsSeen.push(bar.sec);
  }
  let cursor = 0;
  const sections = secsSeen.map((sec) => {
    const layers = [];
    if (sec === "A" || sec === "A2") {
      const k = `comp@${sec}`;
      layers.push({
        layer: "comp",
        section: sec,
        label: "Перегенерировать ритм-гитару",
        salt: Number(salts[k] || 0),
        locked: locks.has(k),
      });
    } else if (sec === "B") {
      const k = "b@B";
      layers.push({
        layer: "b",
        section: "B",
        label: "Вариант середины",
        salt: Number(salts[k] || 0),
        locked: locks.has(k),
      });
    }
    let dur = 2;
    if (sec === "intro" && typeof opts.intro_bars === "number") {
      dur = opts.intro_bars * 2;
    } else if ((sec === "A" || sec === "A2") && opts.square_repeat === 2) {
      dur = 4;
    }
    const start_sec = cursor;
    cursor += dur;
    return {
      section: sec,
      start_sec,
      end_sec: cursor,
      label: sectionLabels[sec] || defaultLabels[sec] || sec,
      layers,
    };
  });
  const undo = Array.isArray(st.undo) ? st.undo : [];
  const redo = Array.isArray(st.redo) ? st.redo : [];
  const length = Math.max(2, cursor);
  const rev = fakeRev(rec);
  const marks = (Array.isArray(st.marks) ? st.marks : []).map((m) => ({
    ...m,
    stale: m.rev !== rev,
    from_sec: Math.min(length, Math.max(0, m.t - 2)),
    to_sec: Math.min(length, m.t + 2),
    can_audition: m.t <= length,
  }));
  return {
    id: st.id,
    rev,
    marks,
    seed: rec.seed,
    meter: rec.intent.meter,
    mode: opts.key ? opts.key.mode : rec.intent.mode,
    key: opts.key ? opts.key.tonic : rec.intent.tonic,
    own_key: { tonic: rec.intent.tonic, mode: rec.intent.mode },
    tempo: rec.intent.tempo,
    length,
    sections,
    whole_layers: [
      {
        layer: "lead_rhythm",
        section: "all",
        label: "Новый риф",
        salt: Number(salts["lead_rhythm@all"] || 0),
        locked: locks.has("lead_rhythm@all"),
      },
      {
        layer: "tempo",
        section: "all",
        label: "Темп",
        salt: Number(salts["tempo@all"] || 0),
        locked: locks.has("tempo@all"),
      },
    ],
    options: {
      b_lead: opts.b_lead || "augmented",
      trailing_rest: opts.trailing_rest !== undefined ? opts.trailing_rest : true,
      mute_share: opts.mute_share ?? 0,
      octave_share: opts.octave_share !== undefined ? opts.octave_share : null,
      intro_bars: opts.intro_bars ?? "auto",
      square_repeat: opts.square_repeat ?? 1,
      octave_piece: opts.octave_piece ?? 0.25,
      octave_sixteenths: opts.octave_sixteenths ?? 0.3,
      dead_notes: opts.dead_notes ?? 0.2,
      slide_max: opts.slide_max ?? 5,
      key: opts.key ?? null,
    },
    can_undo: undo.length > 0,
    can_redo: redo.length > 0,
    undo_label: undo.length > 0 ? describeFakeChange(undo[undo.length - 1], rec) : null,
    redo_label: redo.length > 0 ? describeFakeChange(rec, redo[redo.length - 1]) : null,
  };
}

function defaultFakeEvents(st) {
  return [
    { t: 0, dur: 0.5, pitch: 60, vel: 90, guitar: 1, dead: false },
    { t: 0.5, dur: 0.5, pitch: 64, vel: 90, guitar: 2, dead: false },
  ];
}

class StubClassList {
  constructor(el) {
    this.el = el;
    this._set = new Set();
  }
  _syncFromClassName(cn) {
    this._set = new Set(String(cn || "").split(/\s+/).filter(Boolean));
  }
  _syncToClassName() {
    this.el._className = Array.from(this._set).join(" ");
  }
  add(...tokens) {
    for (const t of tokens) if (t) this._set.add(t);
    this._syncToClassName();
  }
  remove(...tokens) {
    for (const t of tokens) this._set.delete(t);
    this._syncToClassName();
  }
  toggle(token, force) {
    if (force === undefined) {
      if (this._set.has(token)) this._set.delete(token);
      else this._set.add(token);
    } else if (force) {
      this._set.add(token);
    } else {
      this._set.delete(token);
    }
    this._syncToClassName();
    return this._set.has(token);
  }
  contains(token) {
    return this._set.has(token);
  }
}

class StubElement {
  constructor(tagName, env, id = "") {
    this.tagName = String(tagName || "div").toUpperCase();
    this._env = env;
    this.id = id;
    this._className = "";
    this.classList = new StubClassList(this);
    this.dataset = {};
    this.style = {};
    this.children = [];
    this.parentNode = null;
    this.disabled = false;
    this.hidden = false;
    this.value = "";
    this.title = "";
    this.type = "";
    this.min = "";
    this.max = "";
    this.step = "";
    this.href = "";
    this.download = "";
    this.files = [];
    this._textContent = "";
    this._innerHTML = "";
    this._attrs = new Map();
    this._listeners = new Map();
    this.onclick = null;
    this.onchange = null;
    this.oninput = null;
    this.onpointerdown = null;
    this.onpointerup = null;
    this.onpointercancel = null;
    this.onkeydown = null;
    this.onblur = null;
  }

  setAttribute(name, val) {
    const k = String(name);
    const v = String(val ?? "");
    this._attrs.set(k, v);
    if (k === "id") this.id = v;
    if (k === "title") this.title = v;
    if (k === "type") this.type = v;
  }
  getAttribute(name) {
    const k = String(name);
    if (this._attrs.has(k)) return this._attrs.get(k);
    if (k === "id" && this.id) return this.id;
    if (k === "title" && this.title) return this.title;
    if (k === "type" && this.type) return this.type;
    return null;
  }
  hasAttribute(name) {
    return this._attrs.has(String(name));
  }
  removeAttribute(name) {
    this._attrs.delete(String(name));
  }
  addEventListener(type, fn) {
    const t = String(type);
    if (!this._listeners.has(t)) this._listeners.set(t, []);
    this._listeners.get(t).push(fn);
  }
  removeEventListener(type, fn) {
    const t = String(type);
    const arr = this._listeners.get(t);
    if (arr) this._listeners.set(t, arr.filter((f) => f !== fn));
  }
  dispatchEvent(ev) {
    const event = ev || { type: "" };
    if (!event.target) event.target = this;
    const h = this["on" + event.type];
    if (typeof h === "function") h(event);
    for (const fn of [...(this._listeners.get(event.type) || [])]) fn(event);
  }
  get firstChild() {
    return this.children[0] || null;
  }
  insertBefore(child, ref) {
    if (!child) return child;
    child.parentNode = this;
    const idx = ref ? this.children.indexOf(ref) : -1;
    if (idx === -1) this.children.push(child);
    else this.children.splice(idx, 0, child);
    return child;
  }

  get className() {
    return this._className;
  }
  set className(val) {
    this._className = String(val || "");
    this.classList._syncFromClassName(this._className);
  }

  get textContent() {
    if (this.children.length === 0) return this._textContent;
    return this._textContent + this.children.map((c) => c.textContent).join("");
  }
  set textContent(val) {
    this._textContent = String(val ?? "");
    this.children = [];
    this._innerHTML = "";
  }

  get innerHTML() {
    return this._innerHTML;
  }
  set innerHTML(val) {
    const html = String(val ?? "");
    this._innerHTML = html;
    this._textContent = "";
    this.children = [];
    if (html !== "") {
      this._env.innerHTMLWrites.push({ id: this.id, tagName: this.tagName, html });
      const tagRe = /<(div|span|button|input|h2)\b([^>]*)>/gi;
      let m;
      while ((m = tagRe.exec(html)) !== null) {
        const tag = m[1];
        const attrs = m[2] || "";
        const idM = /\bid\s*=\s*["']([^"']+)["']/i.exec(attrs);
        if (idM) {
          const child = new StubElement(tag, this._env, idM[1]);
          const disabledM = /\bdisabled\b/i.test(attrs);
          if (disabledM) child.disabled = true;
          this.appendChild(child);
        }
      }
    }
  }

  appendChild(child) {
    if (!child) return child;
    child.parentNode = this;
    this.children.push(child);
    return child;
  }

  remove() {
    if (this.parentNode) {
      this.parentNode.children = this.parentNode.children.filter((c) => c !== this);
      this.parentNode = null;
    }
  }

  click() {
    if (this.disabled) return;
    if (typeof this.onclick === "function") return this.onclick({ target: this });
  }

  _walk(fn) {
    fn(this);
    for (const c of this.children) c._walk(fn);
  }

  querySelectorAll(selector) {
    const parts = selector.split(",").map((s) => s.trim().toLowerCase());
    const out = [];
    this._walk((el) => {
      if (el === this) return;
      for (const p of parts) {
        if (p.startsWith(".")) {
          if (el.classList.contains(p.slice(1))) {
            out.push(el);
            break;
          }
        } else if (el.tagName.toLowerCase() === p) {
          out.push(el);
          break;
        }
      }
    });
    return out;
  }
}

function createAppHarness(options = {}) {
  const {
    autoReadyWorker = true,
    autoPreloadSamples = true,
    failSampleFetch = false,
    initialPieces = [],
    initialLogs = [],
    sectionLabels = {},
    eventsForState = null,
    sampleDuration = 2.5,
    audioInitialState = "suspended",
    resumeBehavior = "resolve", // "resolve" | "reject" | "stay-suspended"
    decodeDelayMs = 0,
    failDecode = false,
    autoRunWorkerCalls = true,
  } = options;

  const env = {
    innerHTMLWrites: [],
    alerts: [],
    downloads: [],
    workerMessages: [],
    workersCreated: 0,
    activeWorker: null,
    audioContexts: [],
    sourcesCreated: [],
    txLog: [],
    nextTimerId: 1,
    timers: new Map(),
    pendingWorkerCalls: [],
    autoRunWorkerCalls,
    failSampleFetch,
    decodeDelayMs,
    failDecode,
    resumeBehavior,
    audioInitialState,
    idbFailNextWrite: null,
    failNextWorkerCall: null,
    idbUnavailable: Boolean(options.idbUnavailable),
    sectionLabels,
    markSeq: 0,
    docListeners: [],
    press(ev) {
      const event = { preventDefault() {}, target: { tagName: "BODY" }, ...ev };
      for (const fn of [...env.docListeners]) fn(event);
      return event;
    },
  };

  // DOM setup
  const root = new StubElement("body", env, "body");
  const productionHtml = fs.readFileSync(path.join(WEB_DIR, "index.html"), "utf8");
  const nested = { errorBanner: "workspace", errorText: "errorBanner", errorCloseBtn: "errorBanner" };
  const staticEls = new Map();
  for (const m of productionHtml.split("<body")[1].matchAll(/<(\w+)\b([^>]*?\bid="([^"]+)"[^>]*)>/g)) {
    const [, tag, attrs, id] = m;
    const el = new StubElement(tag, env, id);
    if (/(^|\s)hidden(\s|=|$)/.test(attrs)) el.hidden = true;
    const role = /\brole="([^"]*)"/.exec(attrs);
    if (role) el.setAttribute("role", role[1]);
    staticEls.set(id, el);
    (staticEls.get(nested[id]) || root).appendChild(el);
  }

  const documentStub = {
    body: root,
    createElement(tag) {
      return new StubElement(tag, env);
    },
    addEventListener(type, fn) {
      if (type === "keydown") env.docListeners.push(fn);
    },
    createTextNode(text) {
      const el = new StubElement("#text", env);
      el.textContent = String(text ?? "");
      return el;
    },
    getElementById(id) {
      let found = null;
      root._walk((el) => {
        if (!found && el.id === id) found = el;
      });
      return found;
    },
  };

  // IndexedDB setup
  const dbPieces = new Map();
  for (const p of initialPieces) {
    dbPieces.set(p.id, deepClone(p));
  }
  const dbLogs = initialLogs.map((r, i) => ({ ...deepClone(r), seq: i + 1 }));
  let nextLogSeq = dbLogs.length + 1;
  let dbTxChain = Promise.resolve();

  const indexedDBStub = {
    _pieces: dbPieces,
    _logs: dbLogs,
    open() {
      const req = { result: null, onupgradeneeded: null, onsuccess: null, onerror: null };
      queueMicrotask(() => {
        if (env.idbUnavailable) {
          req.error = { name: "SecurityError", message: "IndexedDB unavailable" };
          if (req.onerror) req.onerror({ target: req });
          return;
        }
        const db = {
          objectStoreNames: {
            contains: () => true,
          },
          createObjectStore: () => {},
          transaction(storeNames, mode = "readonly") {
            const stores = Array.isArray(storeNames) ? [...storeNames] : [storeNames];
            const txRecord = { storeNames: stores, mode, ops: [], status: "pending", error: null };
            env.txLog.push(txRecord);

            let resolveTxLock;
            const prevLock = dbTxChain;
            const myLock = new Promise((r) => {
              resolveTxLock = r;
            });
            if (mode === "readwrite") {
              dbTxChain = prevLock.then(() => myLock);
            }
            const readyToRun = mode === "readwrite" ? prevLock : Promise.resolve();

            let stagedPieces = null;
            let stagedLogs = null;
            let pendingOps = 0;
            let aborted = false;
            let finalized = false;

            const tx = {
              oncomplete: null,
              onerror: null,
              onabort: null,
              error: null,
              abort() {
                if (finalized) return;
                aborted = true;
                txRecord.status = "aborted";
              },
              objectStore(name) {
                if (!stores.includes(name)) {
                  throw new Error(`NotFoundError: store ${name} not in transaction [${stores.join(",")}]`);
                }
                const makeReq = (opName, fn) => {
                  const r = { result: undefined, error: null, onsuccess: null, onerror: null };
                  pendingOps += 1;
                  readyToRun.then(() => queueMicrotask(() => {
                    if (!stagedPieces) {
                      stagedPieces = new Map(dbPieces);
                      stagedLogs = [...dbLogs];
                    }
                    try {
                      if (aborted) {
                        r.error = tx.error || { name: "AbortError", message: "Transaction aborted" };
                        if (r.onerror) r.onerror({ target: r });
                        return;
                      }
                      if (mode === "readwrite" && env.idbFailNextWrite) {
                        const errName = env.idbFailNextWrite;
                        env.idbFailNextWrite = null;
                        const err = { name: errName, message: errName };
                        tx.error = err;
                        r.error = err;
                        aborted = true;
                        txRecord.status = "aborted";
                        txRecord.error = errName;
                        if (r.onerror) r.onerror({ target: r });
                        return;
                      }
                      r.result = fn();
                      txRecord.ops.push({ store: name, op: opName });
                      if (r.onsuccess) r.onsuccess({ target: r });
                    } catch (err) {
                      const eObj = { name: err.name || "Error", message: err.message || String(err) };
                      tx.error = eObj;
                      r.error = eObj;
                      aborted = true;
                      txRecord.status = "aborted";
                      txRecord.error = eObj.name;
                      if (r.onerror) r.onerror({ target: r });
                    } finally {
                      pendingOps -= 1;
                      if (pendingOps === 0 && !finalized) {
                        finalized = true;
                        queueMicrotask(() => {
                          if (aborted) {
                            resolveTxLock();
                            if (tx.onabort) tx.onabort({ target: tx });
                            else if (tx.onerror) tx.onerror({ target: tx });
                          } else {
                            if (mode === "readwrite") {
                              dbPieces.clear();
                              for (const [k, v] of stagedPieces.entries()) dbPieces.set(k, v);
                              dbLogs.length = 0;
                              for (const row of stagedLogs) dbLogs.push(row);
                            }
                            txRecord.status = "complete";
                            resolveTxLock();
                            if (tx.oncomplete) tx.oncomplete({ target: tx });
                          }
                        });
                      }
                    }
                  }));
                  return r;
                };

                if (name === "pieces") {
                  return {
                    getAll: () => makeReq("getAll", () => Array.from(stagedPieces.values()).map(deepClone)),
                    get: (id) => makeReq("get", () => deepClone(stagedPieces.get(id))),
                    put: (rec) => makeReq("put", () => {
                      stagedPieces.set(rec.id, deepClone(rec));
                      return rec.id;
                    }),
                    add: (rec) => makeReq("add", () => {
                      if (stagedPieces.has(rec.id)) {
                        const err = new Error(`Key ${rec.id} already exists`);
                        err.name = "ConstraintError";
                        throw err;
                      }
                      stagedPieces.set(rec.id, deepClone(rec));
                      return rec.id;
                    }),
                  };
                }
                if (name === "logs") {
                  return {
                    getAll: () => makeReq("getAll", () => stagedLogs.map(deepClone)),
                    add: (row) => makeReq("add", () => {
                      const seq = nextLogSeq++;
                      stagedLogs.push({ ...deepClone(row), seq });
                      return seq;
                    }),
                  };
                }
                throw new Error(`Unknown store: ${name}`);
              },
            };
            return tx;
          },
        };
        req.result = db;
        if (req.onupgradeneeded) req.onupgradeneeded({ target: req });
        if (req.onsuccess) req.onsuccess({ target: req });
      });
      return req;
    },
  };

  // Fake Python command handler
  function runFakePy(cmd, payload) {
    if (cmd === "new_piece") {
      const seed = payload.seed ?? 801000;
      const st = makeFakeState(seed);
      return {
        state: st,
        detail: buildDetailFromState(st, env.sectionLabels),
        events: eventsForState ? eventsForState(st) : defaultFakeEvents(st),
      };
    }
    if (cmd === "snapshot" || cmd === "import_state") {
      const st = deepClone(payload.state);
      return {
        state: st,
        detail: buildDetailFromState(st, env.sectionLabels),
        events: eventsForState ? eventsForState(st) : defaultFakeEvents(st),
      };
    }
    if (cmd === "mark_add" || cmd === "mark_edit" || cmd === "mark_delete") {
      const st = deepClone(payload.state);
      const marks = Array.isArray(st.marks) ? st.marks : [];
      let mark;
      if (cmd === "mark_add") {
        mark = {
          id: payload.id || `m${++env.markSeq}`,
          t: payload.t,
          bar: 1 + Math.floor(payload.t / 2),
          sixteenth: 1,
          section: "A",
          section_label: "Куплет A",
          guitar: payload.guitar || "both",
          text: payload.text || "",
          notes: [],
          rev: payload.rev || fakeRev(st.recipe),
        };
        if (!marks.some((m) => m.id === mark.id)) marks.push(mark);
      } else {
        const i = marks.findIndex((m) => m.id === payload.id);
        if (i < 0) throw new Error(`unknown mark id: ${payload.id}`);
        if (cmd === "mark_edit") {
          if (payload.guitar !== undefined && payload.guitar !== null) marks[i].guitar = payload.guitar;
          if (payload.text !== undefined && payload.text !== null) marks[i].text = payload.text;
          mark = marks[i];
        } else {
          [mark] = marks.splice(i, 1);
        }
      }
      st.marks = marks;
      return { state: st, detail: buildDetailFromState(st, env.sectionLabels), mark: deepClone(mark) };
    }
    if (cmd === "export_marks") {
      const st = payload.state;
      const detail = buildDetailFromState(st, env.sectionLabels);
      return { version: 1, id: st.id, seed: st.recipe.seed, rev: detail.rev, recipe: st.recipe, marks: detail.marks };
    }
    const respond = (nextSt) => ({
      state: nextSt,
      detail: buildDetailFromState(nextSt, env.sectionLabels),
      events: eventsForState ? eventsForState(nextSt) : defaultFakeEvents(nextSt),
    });
    const pushUndo = (oldSt) => [...(oldSt.undo || []), deepClone(oldSt.recipe)].slice(-50);
    if (cmd === "reroll") {
      const oldSt = deepClone(payload.state);
      const nextRec = deepClone(oldSt.recipe);
      const key = `${payload.layer}@${payload.section}`;
      nextRec.studio.salts[key] = (nextRec.studio.salts[key] || 0) + 1;
      return respond({ id: oldSt.id, ...keepMarks(oldSt), recipe: nextRec, undo: pushUndo(oldSt), redo: [] });
    }
    if (cmd === "lock") {
      const oldSt = deepClone(payload.state);
      const nextRec = deepClone(oldSt.recipe);
      const key = `${payload.layer}@${payload.section}`;
      const locks = new Set(nextRec.studio.locks || []);
      if (payload.locked) locks.add(key);
      else locks.delete(key);
      nextRec.studio.locks = Array.from(locks);
      return respond({ id: oldSt.id, ...keepMarks(oldSt), recipe: nextRec, undo: pushUndo(oldSt), redo: [] });
    }
    if (cmd === "option") {
      const oldSt = deepClone(payload.state);
      const nextRec = deepClone(oldSt.recipe);
      nextRec.studio.options[payload.name] = payload.value;
      if (JSON.stringify(nextRec) === JSON.stringify(oldSt.recipe)) {
        return respond({ id: oldSt.id, ...keepMarks(oldSt), recipe: oldSt.recipe, undo: [...(oldSt.undo || [])], redo: [...(oldSt.redo || [])] });
      }
      return respond({ id: oldSt.id, ...keepMarks(oldSt), recipe: nextRec, undo: pushUndo(oldSt), redo: [] });
    }
    if (cmd === "undo") {
      const oldSt = deepClone(payload.state);
      const undo = [...(oldSt.undo || [])];
      let redo = [...(oldSt.redo || [])];
      let rec = oldSt.recipe;
      if (undo.length) {
        redo = [...redo, deepClone(oldSt.recipe)].slice(-50);
        rec = undo.pop();
      }
      return respond({ id: oldSt.id, ...keepMarks(oldSt), recipe: rec, undo, redo });
    }
    if (cmd === "redo") {
      const oldSt = deepClone(payload.state);
      const redo = [...(oldSt.redo || [])];
      let undo = [...(oldSt.undo || [])];
      let rec = oldSt.recipe;
      if (redo.length) {
        undo = pushUndo(oldSt);
        rec = redo.pop();
      }
      return respond({ id: oldSt.id, ...keepMarks(oldSt), recipe: rec, undo, redo });
    }
    if (cmd === "export_midi") {
      const salt = payload.state?.recipe?.studio?.salts?.["comp@A"] || 0;
      return Buffer.from(`MIDI:${payload.state.id}:salt=${salt}`).toString("base64");
    }
    if (cmd === "export_musicxml") {
      return `<score-partwise>${payload.state.id}</score-partwise>`;
    }
    if (cmd === "export_tab") {
      return `== g1 == ${payload.state.id}`;
    }
    throw new Error(`Unknown cmd: ${cmd}`);
  }

  class StubWorker {
    constructor(url) {
      this.url = url;
      this.onmessage = null;
      this.onerror = null;
      this.onmessageerror = null;
      this.terminated = false;
      env.workersCreated += 1;
      env.activeWorker = this;
    }
    terminate() {
      this.terminated = true;
    }
    postMessage(msg) {
      env.workerMessages.push(deepClone(msg));
      if (msg.type === "init") {
        if (autoReadyWorker) {
          queueMicrotask(() => {
            if (!this.terminated && this.onmessage) {
              this.onmessage({ data: { type: "ready", version: "v0.27.7", loadMs: 10 } });
            }
          });
        }
        return;
      }
      if (msg.type === "call") {
        const entry = {
          msg: deepClone(msg),
          worker: this,
          resolve: (res) => {
            if (!this.terminated && this.onmessage) {
              this.onmessage({ data: { type: "result", id: msg.id, ok: true, result: res, elapsedMs: 5 } });
            }
          },
          reject: (err) => {
            if (!this.terminated && this.onmessage) {
              this.onmessage({
                data: { type: "result", id: msg.id, ok: false, error: String(err), elapsedMs: 5 },
              });
            }
          },
        };
        if (env.autoRunWorkerCalls) {
          queueMicrotask(() => {
            if (env.failNextWorkerCall) {
              const errMsg = env.failNextWorkerCall;
              env.failNextWorkerCall = null;
              entry.reject(errMsg);
              return;
            }
            try {
              entry.resolve(runFakePy(msg.cmd, msg.payload));
            } catch (e) {
              entry.reject(e.message || String(e));
            }
          });
        } else {
          env.pendingWorkerCalls.push(entry);
        }
      }
    }
  }

  // AudioContext stub
  class StubAudioContext {
    constructor() {
      this.currentTime = 0;
      this.state = env.audioInitialState;
      this._listeners = new Map();
      this.onstatechange = null;
      this.destination = {};
      env.audioContexts.push(this);
    }
    addEventListener(type, fn) {
      if (!this._listeners.has(type)) this._listeners.set(type, []);
      this._listeners.get(type).push(fn);
    }
    emitStateChange(nextState) {
      this.state = nextState;
      const ev = { type: "statechange", target: this };
      if (typeof this.onstatechange === "function") this.onstatechange(ev);
      for (const fn of this._listeners.get("statechange") || []) fn(ev);
    }
    resume() {
      if (env.resumeBehavior === "reject") {
        return Promise.reject(new Error("NotAllowedError: play() denied"));
      }
      if (env.resumeBehavior === "stay-suspended") {
        return Promise.resolve();
      }
      this.state = "running";
      return Promise.resolve();
    }
    createDynamicsCompressor() {
      return { threshold: { value: 0 }, ratio: { value: 1 }, connect() {} };
    }
    createGain() {
      const sched = [];
      return {
        gain: {
          value: 1,
          _sched: sched,
          setValueAtTime(v, t) {
            sched.push({ op: "setValueAtTime", v, t });
          },
          linearRampToValueAtTime(v, t) {
            sched.push({ op: "linearRampToValueAtTime", v, t });
          },
        },
        _sched: sched,
        connect() {},
      };
    }
    createDelay() {
      return { delayTime: { value: 0 }, connect() {} };
    }
    createBiquadFilter() {
      return { type: "", frequency: { value: 0 }, connect() {} };
    }
    createStereoPanner() {
      return { pan: { value: 0 }, connect() {} };
    }
    createBufferSource() {
      const src = {
        buffer: null,
        playbackRate: { value: 1 },
        loop: false,
        loopStart: 0,
        loopEnd: 0,
        startArgs: null,
        stopArgs: null,
        stopped: false,
        onended: null,
        _gainNode: null,
        connect(node) {
          this._gainNode = node;
        },
        start(...args) {
          src.startArgs = args;
        },
        stop(...args) {
          src.stopArgs = args;
          src.stopped = true;
          if (typeof src.onended === "function") src.onended();
        },
      };
      env.sourcesCreated.push(src);
      return src;
    }
    decodeAudioData() {
      const run = () => {
        if (env.failDecode) {
          return Promise.reject(new Error("EncodingError: decode failed"));
        }
        const len = Math.floor(44100 * sampleDuration);
        const pcm = new Float32Array(len);
        for (let i = 0; i < len; i++) {
          pcm[i] = Math.sin((2 * Math.PI * 220 * i) / 44100);
        }
        return Promise.resolve({
          duration: sampleDuration,
          sampleRate: 44100,
          length: len,
          getChannelData: () => pcm,
        });
      };
      if (env.decodeDelayMs > 0) {
        return new Promise((resolve, reject) => {
          setTimeout(() => run().then(resolve, reject), env.decodeDelayMs);
        });
      }
      return run();
    }
  }

  // Fetch stub (manifest + samples)
  const manifestData = {
    files: {
      "60": { "72": "p060_v072.mp3", "100": "p060_v100.mp3" },
      "64": { "72": "p064_v072.mp3", "100": "p064_v100.mp3" },
    },
  };

  async function fetchStub(url) {
    if (!autoPreloadSamples) {
      await new Promise(() => {});
    }
    if (env.failSampleFetch) {
      return { ok: false, status: 500 };
    }
    if (String(url).endsWith("manifest.json")) {
      return { ok: true, status: 200, json: async () => deepClone(manifestData) };
    }
    return {
      ok: true,
      status: 200,
      arrayBuffer: async () => new Uint8Array([1, 2, 3, 4]).buffer,
    };
  }

  const windowStub = {
    indexedDB: env.idbUnavailable ? null : indexedDBStub,
    AudioContext: StubAudioContext,
    alert: (msg) => env.alerts.push(String(msg)),
  };

  const context = {
    window: windowStub,
    document: documentStub,
    Worker: StubWorker,
    fetch: fetchStub,
    Blob: class StubBlob {
      constructor(parts, opts) {
        this.parts = parts;
        this.type = opts?.type || "";
      }
    },
    URL: {
      createObjectURL(blob) {
        env.downloads.push(blob);
        return "blob:stub";
      },
      revokeObjectURL() {},
    },
    File: class StubFile {},
    performance: { now: () => Date.now() },
    atob: (b64) => Buffer.from(b64, "base64").toString("binary"),
    btoa: (str) => Buffer.from(str, "binary").toString("base64"),
    requestAnimationFrame: () => env.nextTimerId++,
    cancelAnimationFrame: () => {},
    setInterval: (fn, ms) => {
      const id = env.nextTimerId++;
      env.timers.set(id, { fn, ms });
      return id;
    },
    clearInterval: (id) => {
      env.timers.delete(id);
    },
    setTimeout,
    clearTimeout,
    console,
    Promise,
    Map,
    Set,
    Array,
    Object,
    Number,
    String,
    Boolean,
    Math,
    JSON,
    Date,
    Error,
    Uint8Array,
    Buffer,
    parseInt,
    parseFloat,
  };
  windowStub.window = windowStub;

  const appCode = fs.readFileSync(path.join(WEB_DIR, "app.js"), "utf8");
  vm.runInNewContext(appCode, context, { filename: "app.js" });

  env.window = windowStub;
  env.document = documentStub;
  env.indexedDB = indexedDBStub;
  env.runFakePy = runFakePy;
  env.tickTimers = () => {
    for (const { fn } of env.timers.values()) fn();
  };
  env.flushMicrotasks = async (rounds = 8) => {
    for (let i = 0; i < rounds; i++) {
      await new Promise((resolve) => queueMicrotask(resolve));
    }
  };
  env.flushAll = async (ms = 10) => {
    await env.flushMicrotasks(10);
    await new Promise((resolve) => setTimeout(resolve, ms));
    await env.flushMicrotasks(10);
  };
  env.makeFakeState = makeFakeState;
  env.buildDetailFromState = buildDetailFromState;

  return env;
}

export { createAppHarness, makeFakeState, buildDetailFromState, deepClone };
