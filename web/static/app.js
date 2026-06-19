// ES module: loads the wasm-bindgen glue (ADR-0025) and drives the playground.
// `arrange`/`load_score` return JSON strings, so there is no manual linear-memory
// marshalling here — wasm-bindgen handles the String/&[u8] boundary.
import init, {
  arrange as wasmArrange,
  load_score as wasmLoadScore,
  build_chunk_json as wasmBuildChunk,
  split_chunks_json as wasmSplitChunks,
  detect_boundaries_json as wasmDetectBoundaries,
  tag_palette_json as wasmTagPalette,
} from './griff_web.js';
import { createDebugLog } from './debuglog.js';
import {
  MODE_NAMES, OCTAVE_DOUBLE, SAFE_FALLBACK_MODES, friendlyArrangeError, snapOctaveOffset,
} from './modes.js';

const $ = (id) => document.getElementById(id);
const els = {
  file: $('file'), track: $('track'),
  mode: $('mode'), seed: $('seed'), seedOut: $('seedOut'),
  offset: $('offset'), offsetOut: $('offsetOut'),
  variation: $('variation'), varOut: $('varOut'),
  gen: $('gen'), play: $('play'), stop: $('stop'),
  roll: $('roll'), status: $('status'),
  // capture panel (chunk.json, ADR-0026)
  capture: $('capture'),
  capId: $('capId'), capTitle: $('capTitle'), capTuning: $('capTuning'),
  capCohort: $('capCohort'), capTags: $('capTags'), capQuality: $('capQuality'),
  capReviewer: $('capReviewer'), capRights: $('capRights'), capAcq: $('capAcq'),
  capRedist: $('capRedist'), capNotes: $('capNotes'),
  capDetect: $('capDetect'), capDownload: $('capDownload'),
  capBounds: $('capBounds'), capStatus: $('capStatus'),
  // auto-split phrase pager (#2b)
  capSplit: $('capSplit'), splitView: $('splitView'),
  splitPrev: $('splitPrev'), splitNext: $('splitNext'),
  splitPos: $('splitPos'), splitInfo: $('splitInfo'), splitTags: $('splitTags'),
  splitPlay: $('splitPlay'), splitStop: $('splitStop'),
  splitDownload: $('splitDownload'), splitDownloadEach: $('splitDownloadEach'),
  splitDownloadAll: $('splitDownloadAll'),
  // verbose on-page debug log
  debugLog: $('debugLog'), dbgCopy: $('dbgCopy'), dbgClear: $('dbgClear'),
};

const PPQN_FALLBACK = 480;
const MAX_UPLOAD_BYTES = 16 * 1024 * 1024; // mirror the Rust-side upload guard
let current = null;  // last arrange() result (parsed JSON)
let fileName = '';   // name of the uploaded file (recorded as the chunk's source)
let splitChunks = []; // phrase chunks from the last split (#2b)
let splitIdx = 0;     // currently-viewed phrase
let audio = null;    // AudioContext
let voices = [];     // scheduled oscillators
let playStartT = 0, playSpan = 0, raf = 0;

// ---- verbose debug log (on-page, copy-paste friendly) ----
// The bounded, timestamped ring buffer lives in debuglog.js so its formatting
// logic is unit-tested (web/test/debuglog.test.js); here we just mirror it into
// the <pre> on every write and keep it scrolled to the newest line.
const log = createDebugLog();

function renderDebug() {
  if (!els.debugLog) return;
  els.debugLog.textContent = log.text();
  els.debugLog.scrollTop = els.debugLog.scrollHeight;
}
// `data` (optional) is JSON-stringified, so the exact inputs/outputs of each
// engine call are visible without opening the browser console.
function dbg(label, data) { log.push(label, data); renderDebug(); }
function dbgErr(label, data) { log.err(label, data); renderDebug(); }

// Copies the whole log to the clipboard (https + the click gesture satisfy the
// Clipboard API on mobile); briefly flashes the button as feedback.
function copyDebug() {
  const text = log.text() || '(empty)';
  const flash = (msg) => {
    const b = els.dbgCopy, old = b.textContent;
    b.textContent = msg;
    setTimeout(() => { b.textContent = old; }, 1200);
  };
  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(text).then(() => flash('✓ copied'), () => flash('copy failed'));
  } else {
    flash('no clipboard');
  }
}

function clearDebug() { log.clear(); renderDebug(); }

// `logIt` requests a one-line trace of this arrange (discrete actions: Generate,
// mode/track change, load); slider drags pass nothing to avoid flooding the log.
// Errors are always logged regardless.
function arrange(logIt) {
  const mode = +els.mode.value;
  const seed = +els.seed.value;
  const offset = +els.offset.value;
  const variation = (+els.variation.value) / 100;
  const track = +els.track.value;
  let next;
  try {
    next = JSON.parse(wasmArrange(mode, seed, offset, variation, track));
  } catch (err) {
    els.status.classList.add('error');
    els.status.textContent = 'arrange failed: engine returned bad JSON';
    dbgErr('arrange failed', { mode: MODE_NAMES[mode] || mode, seed, offset, variation, track, err: String(err) });
    return;
  }
  current = next;
  if (current.error) {
    dbgErr('arrange', { mode: MODE_NAMES[mode] || mode, seed, offset, variation, track, error: current.error });
  } else if (logIt) {
    const a = current.tracks.find((t) => t.role === 'a');
    const b = current.tracks.find((t) => t.role === 'b');
    dbg('arrange', {
      mode: MODE_NAMES[mode] || mode, seed, offset, variation: +variation.toFixed(2), track,
      a: a ? a.notes.length : 0, b: b ? b.notes.length : 0,
      spread: +Number(current.realized_spread).toFixed(2),
    });
  }
  draw();
  showStatus();
}

function showStatus() {
  if (!current) return;
  const a = current.tracks.find((t) => t.role === 'a');
  const b = current.tracks.find((t) => t.role === 'b');
  els.status.classList.toggle('error', !!current.error);
  if (current.error) {
    els.status.textContent = friendlyArrangeError(current.error) + ' — Part A still shown.';
    return;
  }
  els.status.textContent =
    `A: ${a ? a.notes.length : 0} notes · B: ${b ? b.notes.length : 0} notes` +
    ` · realized spread ${Number(current.realized_spread).toFixed(2)}`;
}

// octave_double only accepts whole-octave Register offsets, so when it's picked
// step the slider in octaves and snap to a valid value (see modes.js); other
// modes use the default 1-semitone step.
function applyModeConstraints() {
  const octave = +els.mode.value === OCTAVE_DOUBLE;
  els.offset.step = octave ? '12' : '1';
  if (octave) {
    els.offset.value = String(snapOctaveOffset(+els.offset.value));
    els.offsetOut.textContent = els.offset.value;
  }
}

// Arranges a freshly loaded score with the selected mode, but if that mode can't
// apply (e.g. counter_melody on a meter-changing song) falls back to an
// always-valid mode so the first view is a result, not an error. Explicit user
// actions keep their mode and get the friendly error instead.
function arrangePreferred() {
  arrange(true);
  if (!current || !current.error) return;
  const chosen = +els.mode.value;
  for (const m of SAFE_FALLBACK_MODES) {
    if (m === chosen) continue;
    els.mode.value = String(m);
    applyModeConstraints();
    arrange(true);
    if (current && !current.error) {
      dbg('mode auto-switched',
        { from: MODE_NAMES[chosen] || chosen, to: MODE_NAMES[m], why: 'selected mode could not apply to this score' });
      return;
    }
  }
  els.mode.value = String(chosen); // none applied: restore the choice + its message
  applyModeConstraints();
  arrange(true);
}

// ---- file loading (MIDI or Guitar Pro; see ADR-0025) ----
const escapeHtml = (s) => String(s).replace(/[&<>"]/g,
  (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

function loadFile(file) {
  dbg('load file', { name: file.name, bytes: file.size });
  if (file.size > MAX_UPLOAD_BYTES) {
    els.status.classList.add('error');
    els.status.textContent =
      `load failed: file too large (${file.size} bytes; limit ${MAX_UPLOAD_BYTES / (1024 * 1024)} MiB)`;
    dbgErr('load rejected', `too large: ${file.size} bytes`);
    return;
  }
  const reader = new FileReader();
  reader.onerror = () => {
    els.status.classList.add('error');
    els.status.textContent = 'load failed: could not read file';
    dbgErr('load failed', 'could not read file');
  };
  reader.onload = () => {
    try {
      const summary = JSON.parse(wasmLoadScore(new Uint8Array(reader.result)));
      if (summary.error) {
        els.status.classList.add('error');
        els.status.textContent = 'load failed: ' + summary.error;
        dbgErr('load failed', summary.error);
        return;
      }
      els.status.classList.remove('error');
      fileName = file.name;
      // A new source = a fresh capture context: retitle and clear stale
      // boundary/status text so we never carry the previous file's metadata.
      els.capTitle.value = file.name.replace(/\.[^.]+$/, '');
      els.capBounds.textContent = '';
      capMsg('', false);
      resetSplit();
      els.capture.hidden = false;
      dbg('loaded', {
        tracks: summary.tracks.length, bars: summary.bars,
        perTrack: summary.tracks.map((t) => `${t.i}:${t.name}=${t.notes}n`),
      });
      populateTracks(summary);
      arrangePreferred();
    } catch (err) {
      els.status.classList.add('error');
      els.status.textContent = 'load failed: ' + err;
      dbgErr('load failed', String(err));
    }
  };
  reader.readAsArrayBuffer(file);
}

function populateTracks(summary) {
  const opts = ['<option value="-1">built-in sample</option>'];
  let firstWithNotes = -1;
  for (const t of summary.tracks) {
    opts.push(`<option value="${t.i}">${escapeHtml(t.name)} · ${t.notes} notes</option>`);
    if (firstWithNotes < 0 && t.notes > 0) firstWithNotes = t.i;
  }
  els.track.innerHTML = opts.join('');
  els.track.value = String(firstWithNotes >= 0 ? firstWithNotes : -1);
  els.status.textContent =
    `loaded ${summary.tracks.length} track(s) · ${summary.bars} bars — pick a track, then ▶`;
}

// ---- chunk.json capture (ADR-0026) ----
function populateTagPalette() {
  let names = [];
  try { names = JSON.parse(wasmTagPalette()); } catch (_) { /* leave empty */ }
  els.capTags.innerHTML = names
    .map((n, i) =>
      `<label><input type="checkbox" value="${i}" /> ${escapeHtml(n)}</label>`)
    .join('');
}

// Space-separated indices of the ticked checkboxes in a .checkgrid group — the
// format build_chunk_json/parse_indices expects (was <select multiple>, which
// needed Ctrl/Cmd-click on desktop and a plain click wiped the whole picks).
const selectedValues = (group) =>
  Array.from(group.querySelectorAll('input[type="checkbox"]:checked'))
    .map((c) => c.value).join(' ');

function capMsg(text, isError) {
  els.capStatus.classList.toggle('error', !!isError);
  els.capStatus.textContent = text;
}

function detectBoundaries() {
  const track = +els.track.value;
  if (track < 0) { capMsg('load a tab and pick a track first', true); dbgErr('detect', 'no track selected'); return; }
  dbg('detect boundaries', { track });
  let res;
  try { res = JSON.parse(wasmDetectBoundaries(track)); }
  catch (e) { capMsg('detect failed: ' + e, true); dbgErr('detect failed', String(e)); return; }
  if (res.error) { capMsg('detect failed: ' + res.error, true); dbgErr('detect failed', res.error); return; }
  const n = res.boundaries.length;
  capMsg('', false); // drop any stale error from a previous failed detect
  dbg('boundaries', { count: n, ticks: res.boundaries });
  els.capBounds.textContent =
    n ? `${n} phrase boundar${n === 1 ? 'y' : 'ies'} detected` : 'no phrase boundaries detected';
}

function downloadChunk() {
  const track = +els.track.value;
  if (track < 0) { capMsg('load a tab and pick a real track first', true); dbgErr('capture', 'no track selected'); return; }
  const id = els.capId.value.trim();
  if (!id) { capMsg('a chunk ID is required (e.g. dgd_001)', true); dbgErr('capture', 'missing chunk id'); return; }

  const tagsIdx = selectedValues(els.capTags);
  const qualityIdx = selectedValues(els.capQuality);
  dbg('capture chunk', {
    track, id, title: els.capTitle.value, tuning: els.capTuning.value,
    cohort: +els.capCohort.value, tagsIdx: tagsIdx || '(none ticked)', qualityIdx,
    reviewer: +els.capReviewer.value, rights: +els.capRights.value,
    acq: +els.capAcq.value, redist: els.capRedist.checked,
  });

  const now = new Date().toISOString();
  const json = wasmBuildChunk(
    track, id, els.capTitle.value, fileName, els.capTuning.value,
    +els.capCohort.value, tagsIdx, qualityIdx,
    +els.capReviewer.value, +els.capRights.value, +els.capAcq.value,
    els.capRedist.checked, els.capNotes.value, now, now,
  );

  let parsed;
  try { parsed = JSON.parse(json); }
  catch (_) { capMsg('capture failed: engine returned bad JSON', true); dbgErr('capture failed', 'bad JSON from engine'); return; }
  if (parsed.error) { capMsg('capture failed: ' + parsed.error, true); dbgErr('capture failed', parsed.error); return; }

  // Resolved metadata — shows why `tags` may be empty (none ticked + no
  // notation-derived technique tags) and which metrics the engine could compute.
  dbg('captured', {
    id, tags: parsed.tags, techniques: parsed.techniques,
    quality: parsed.quality_flags, boundaries: parsed.boundaries.length,
    metrics: { structure: !!parsed.structure, gesture: !!parsed.gesture, complexity: !!parsed.complexity },
  });
  saveBlob(`${id}.chunk.json`, json);
  capMsg(`saved ${id}.chunk.json · ${parsed.boundaries.length} boundaries · ` +
    `${parsed.tags.length} tag(s) · rights recorded`, false);
}

// Triggers a download of `text` (a JSON string) as the file `name`.
function saveBlob(name, text) {
  const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
  const a = document.createElement('a');
  a.href = url; a.download = name;
  document.body.appendChild(a); a.click(); a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

// ---- auto-split: one chunk.json per detected phrase (#2b) ----
// Reuses the capture form's metadata for every phrase; the engine suffixes ids
// (_p<N>) and titles ((phrase <N>)) and drops phrases silent on the track.
function resetSplit() {
  splitChunks = []; splitIdx = 0;
  if (els.splitView) els.splitView.hidden = true;
}

function splitIntoPhrases() {
  const track = +els.track.value;
  if (track < 0) { capMsg('load a tab and pick a real track first', true); return; }
  const id = els.capId.value.trim();
  if (!id) { capMsg('a chunk ID is required (e.g. dgd_001)', true); return; }

  const tagsIdx = selectedValues(els.capTags);
  const qualityIdx = selectedValues(els.capQuality);
  dbg('split into phrases', { track, id, tagsIdx: tagsIdx || '(none ticked)', qualityIdx });

  const now = new Date().toISOString();
  let res;
  try {
    res = JSON.parse(wasmSplitChunks(
      track, id, els.capTitle.value, fileName, els.capTuning.value,
      +els.capCohort.value, tagsIdx, qualityIdx,
      +els.capReviewer.value, +els.capRights.value, +els.capAcq.value,
      els.capRedist.checked, els.capNotes.value, now, now,
    ));
  } catch (_) { capMsg('split failed: engine returned bad JSON', true); dbgErr('split failed', 'bad JSON from engine'); return; }
  if (res.error) { capMsg('split failed: ' + res.error, true); dbgErr('split failed', res.error); return; }

  splitChunks = res.chunks || [];
  splitIdx = 0;
  if (splitChunks.length === 0) { resetSplit(); capMsg('no sounding phrases to split', true); dbg('split result', { phrases: 0 }); return; }
  // Tags can differ per phrase: chosen tags repeat, but notation-derived
  // technique tags surface only in the phrases that contain them — so report
  // every phrase's tags, not just the first (would hide them otherwise).
  const phraseTags = splitChunks.map((c) => {
    try { return JSON.parse(c.chunk).tags || []; } catch (_) { return []; }
  });
  dbg('split result', {
    phrases: splitChunks.length,
    ids: splitChunks.map((c) => c.id),
    bars: splitChunks.map((c) => `${c.bar_lo}-${c.bar_hi}`),
    tags: phraseTags,
  });
  els.splitView.hidden = false;
  capMsg(`split into ${splitChunks.length} phrase chunk(s) — page through and ▶`, false);
  renderPhrase();
}

// Draws the current phrase into the shared roll and reuses the transport synth
// (current.tracks/ppqn/tempo is exactly what draw()/play() already read).
function renderPhrase() {
  stop();
  const ch = splitChunks[splitIdx];
  if (!ch) return;
  let ppqn = PPQN_FALLBACK, tempo = 120, tags = [];
  try { const m = JSON.parse(ch.chunk); ppqn = m.ticks_per_quarter || ppqn; tempo = m.tempo_bpm || tempo; tags = m.tags || []; }
  catch (_) { /* fall back to defaults */ }
  current = {
    ppqn, tempo, error: null, realized_spread: 0,
    tracks: [{ name: ch.title, role: 'a', notes: ch.notes || [] }],
  };
  draw();
  els.splitPos.textContent = `phrase ${splitIdx + 1} / ${splitChunks.length}`;
  els.splitInfo.textContent =
    `${ch.id} · bars ${ch.bar_lo}–${ch.bar_hi} · ${(ch.notes || []).length} notes`;
  renderPhraseTags(tags);
  els.splitPrev.disabled = splitIdx === 0;
  els.splitNext.disabled = splitIdx === splitChunks.length - 1;
}

// Shows the phrase's resolved tags (chosen + notation-derived techniques) as
// ticked, read-only boxes, so detected techniques like slide/hammer_on are
// visible per phrase. Read-only on purpose: these reflect the phrase and do not
// feed the next split (that's the capture form's Tags above).
function renderPhraseTags(tags) {
  if (!els.splitTags) return;
  els.splitTags.innerHTML = tags.length
    ? tags.map((t) =>
        `<label class="on"><input type="checkbox" checked disabled /> ${escapeHtml(t)}</label>`).join('')
    : '<span class="muted small">no tags detected</span>';
}

function stepPhrase(d) {
  if (splitChunks.length === 0) return;
  splitIdx = Math.min(splitChunks.length - 1, Math.max(0, splitIdx + d));
  renderPhrase();
}

function downloadPhrase() {
  const ch = splitChunks[splitIdx];
  if (!ch) { capMsg('split into phrases first', true); return; }
  saveBlob(`${ch.id}.chunk.json`, ch.chunk);
  capMsg(`saved ${ch.id}.chunk.json · bars ${ch.bar_lo}–${ch.bar_hi}`, false);
}

// Saves one manifest-ready <id>_p<N>.chunk.json per phrase — what `griff manifest`
// ingests (it collects individual .chunk.json files, one ChunkMeta each).
// Staggered so the browser does not coalesce the rapid downloads.
function downloadEachPhrase() {
  if (splitChunks.length === 0) { capMsg('split into phrases first', true); return; }
  splitChunks.forEach((ch, i) =>
    setTimeout(() => saveBlob(`${ch.id}.chunk.json`, ch.chunk), i * 120));
  capMsg(`saving ${splitChunks.length} .chunk.json file(s) — for griff manifest`, false);
}

// Bundles every phrase chunk into a single JSON array — one file for sharing or
// review instead of N. NOTE: `griff manifest` reads individual .chunk.json files,
// not this array, so use "each phrase" for the corpus pipeline (#77).
function downloadBundle() {
  if (splitChunks.length === 0) { capMsg('split into phrases first', true); return; }
  const base = els.capId.value.trim() || (splitChunks[0].id || 'phrases').replace(/_p\d+$/, '');
  const chunks = splitChunks.map((ch) => {
    try { return JSON.parse(ch.chunk); } catch (_) { return { id: ch.id, error: 'unparseable chunk' }; }
  });
  saveBlob(`${base}.chunks.json`, JSON.stringify(chunks, null, 2));
  capMsg(`saved ${base}.chunks.json · ${chunks.length} phrase(s) in one file — for sharing/review`, false);
}

// ---- drawing ----
function allNotes() {
  if (!current) return [];
  return current.tracks.flatMap((t) => t.notes.map((n) => ({ ...n, role: t.role })));
}

function draw(playheadTick) {
  const c = els.roll, dpr = window.devicePixelRatio || 1;
  const w = c.clientWidth, h = c.clientHeight;
  c.width = Math.floor(w * dpr); c.height = Math.floor(h * dpr);
  const ctx = c.getContext('2d');
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);

  const notes = allNotes();
  if (notes.length === 0) return;

  const ppqn = current.ppqn || PPQN_FALLBACK;
  const endTick = Math.max(...notes.map((n) => n.s + n.d), 1);
  const loP = Math.min(...notes.map((n) => n.p)) - 1;
  const hiP = Math.max(...notes.map((n) => n.p)) + 1;
  const pad = 6, plotW = w - pad * 2, plotH = h - pad * 2;
  const x = (t) => pad + (t / endTick) * plotW;
  const y = (p) => pad + (1 - (p - loP) / Math.max(1, hiP - loP)) * plotH;
  const rowH = plotH / Math.max(1, hiP - loP);

  // bar gridlines (4/4: bar = 4*ppqn)
  ctx.strokeStyle = '#1c2230'; ctx.lineWidth = 1;
  for (let t = 0; t <= endTick; t += 4 * ppqn) {
    ctx.beginPath(); ctx.moveTo(x(t), pad); ctx.lineTo(x(t), h - pad); ctx.stroke();
  }

  for (const n of notes) {
    const nx = x(n.s), nw = Math.max(2, x(n.s + n.d) - nx);
    const ny = y(n.p) - rowH / 2, nh = Math.max(3, rowH * 0.8);
    ctx.fillStyle = n.role === 'a' ? '#4aa3ff' : '#ffb24a';
    ctx.globalAlpha = 0.35 + 0.55 * (n.v / 127);
    ctx.fillRect(nx, ny, nw, nh);
  }
  ctx.globalAlpha = 1;

  if (playheadTick != null) {
    ctx.strokeStyle = '#5ad19a'; ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(x(playheadTick), pad); ctx.lineTo(x(playheadTick), h - pad); ctx.stroke();
  }
}

// ---- playback (placeholder synth; a real SoundFont comes later) ----
const tickToSec = (tick, ppqn, tempo) => (tick / ppqn) * (60 / tempo);

function play() {
  if (!current || allNotes().length === 0) return;
  stop();
  audio = audio || new (window.AudioContext || window.webkitAudioContext)();
  if (audio.state === 'suspended') audio.resume(); // unlock on the tap gesture

  const ppqn = current.ppqn || PPQN_FALLBACK;
  const tempo = current.tempo || 120;
  const t0 = audio.currentTime + 0.08;
  const master = audio.createGain();
  master.gain.value = 0.25;
  master.connect(audio.destination);

  let maxEnd = 0;
  for (const n of allNotes()) {
    const start = t0 + tickToSec(n.s, ppqn, tempo);
    const dur = Math.max(0.05, tickToSec(n.d, ppqn, tempo));
    maxEnd = Math.max(maxEnd, tickToSec(n.s + n.d, ppqn, tempo));

    const osc = audio.createOscillator();
    osc.type = 'sawtooth';
    osc.frequency.value = 440 * Math.pow(2, (n.p - 69) / 12);
    const g = audio.createGain();
    const peak = 0.18 + 0.5 * (n.v / 127);
    g.gain.setValueAtTime(0.0001, start);
    g.gain.exponentialRampToValueAtTime(peak, start + 0.01);
    g.gain.exponentialRampToValueAtTime(0.0001, start + dur);
    osc.connect(g);
    if (audio.createStereoPanner) {
      const pan = audio.createStereoPanner();
      pan.pan.value = n.role === 'a' ? -0.4 : 0.4;
      g.connect(pan); pan.connect(master);
    } else {
      g.connect(master);
    }
    osc.start(start); osc.stop(start + dur + 0.02);
    voices.push(osc);
  }

  playStartT = t0; playSpan = maxEnd;
  animatePlayhead();
}

function animatePlayhead() {
  cancelAnimationFrame(raf);
  const step = () => {
    if (!audio) return;
    const el = audio.currentTime - playStartT;
    if (el > playSpan + 0.1) { draw(); return; }
    if (el >= 0) {
      const ppqn = current.ppqn || PPQN_FALLBACK, tempo = current.tempo || 120;
      draw((el / (60 / tempo)) * ppqn);
    }
    raf = requestAnimationFrame(step);
  };
  raf = requestAnimationFrame(step);
}

function stop() {
  cancelAnimationFrame(raf);
  for (const v of voices) { try { v.stop(); } catch (_) { /* already stopped */ } }
  voices = [];
  draw();
}

// ---- wiring ----
function bind() {
  els.seed.addEventListener('input', () => { els.seedOut.textContent = els.seed.value; arrange(); });
  els.offset.addEventListener('input', () => { els.offsetOut.textContent = els.offset.value; arrange(); });
  els.variation.addEventListener('input', () => {
    els.varOut.textContent = (els.variation.value / 100).toFixed(2); arrange();
  });
  els.mode.addEventListener('change', () => { applyModeConstraints(); arrange(true); });
  // Phrases are track-specific: a new track invalidates the current split.
  els.track.addEventListener('change', () => {
    resetSplit();
    dbg('track ->', { i: +els.track.value, name: els.track.options[els.track.selectedIndex]?.text || '' });
    arrange(true);
  });
  els.file.addEventListener('change', (e) => {
    const f = e.target.files && e.target.files[0];
    if (f) loadFile(f);
  });
  els.gen.addEventListener('click', () => arrange(true));
  els.play.addEventListener('click', play);
  els.stop.addEventListener('click', stop);
  els.capDetect.addEventListener('click', detectBoundaries);
  els.capDownload.addEventListener('click', downloadChunk);
  els.capSplit.addEventListener('click', splitIntoPhrases);
  els.splitPrev.addEventListener('click', () => stepPhrase(-1));
  els.splitNext.addEventListener('click', () => stepPhrase(1));
  els.splitPlay.addEventListener('click', () => { renderPhrase(); play(); });
  els.splitStop.addEventListener('click', stop);
  els.splitDownload.addEventListener('click', downloadPhrase);
  els.splitDownloadEach.addEventListener('click', downloadEachPhrase);
  els.splitDownloadAll.addEventListener('click', downloadBundle);
  els.dbgCopy.addEventListener('click', copyDebug);
  els.dbgClear.addEventListener('click', clearDebug);
  window.addEventListener('resize', () => draw());
}

init().then(() => {
  bind();
  populateTagPalette();
  dbg('engine ready');
  applyModeConstraints();
  arrangePreferred();
  els.status.textContent = 'ready — load a tab or drag a slider, then ▶ Play';
}).catch((err) => {
  els.status.classList.add('error');
  els.status.textContent = 'failed to load engine: ' + err;
  dbgErr('engine load failed', String(err));
});
