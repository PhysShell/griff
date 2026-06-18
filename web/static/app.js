// ES module: loads the wasm-bindgen glue (ADR-0025) and drives the playground.
// `arrange`/`load_score` return JSON strings, so there is no manual linear-memory
// marshalling here — wasm-bindgen handles the String/&[u8] boundary.
import init, {
  arrange as wasmArrange,
  load_score as wasmLoadScore,
  build_chunk_json as wasmBuildChunk,
  detect_boundaries_json as wasmDetectBoundaries,
  tag_palette_json as wasmTagPalette,
} from './griff_web.js';

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
};

const PPQN_FALLBACK = 480;
const MAX_UPLOAD_BYTES = 16 * 1024 * 1024; // mirror the Rust-side upload guard
let current = null;  // last arrange() result (parsed JSON)
let fileName = '';   // name of the uploaded file (recorded as the chunk's source)
let audio = null;    // AudioContext
let voices = [];     // scheduled oscillators
let playStartT = 0, playSpan = 0, raf = 0;

function arrange() {
  const mode = +els.mode.value;
  const seed = +els.seed.value;
  const offset = +els.offset.value;
  const variation = (+els.variation.value) / 100;
  const track = +els.track.value;
  current = JSON.parse(wasmArrange(mode, seed, offset, variation, track));
  draw();
  showStatus();
}

function showStatus() {
  if (!current) return;
  const a = current.tracks.find((t) => t.role === 'a');
  const b = current.tracks.find((t) => t.role === 'b');
  els.status.classList.toggle('error', !!current.error);
  if (current.error) {
    els.status.textContent =
      `error: ${current.error} — try another offset/mode (A still shown)`;
    return;
  }
  els.status.textContent =
    `A: ${a ? a.notes.length : 0} notes · B: ${b ? b.notes.length : 0} notes` +
    ` · realized spread ${Number(current.realized_spread).toFixed(2)}`;
}

// ---- file loading (MIDI or Guitar Pro; see ADR-0025) ----
const escapeHtml = (s) => String(s).replace(/[&<>"]/g,
  (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

function loadFile(file) {
  if (file.size > MAX_UPLOAD_BYTES) {
    els.status.classList.add('error');
    els.status.textContent =
      `load failed: file too large (${file.size} bytes; limit ${MAX_UPLOAD_BYTES / (1024 * 1024)} MiB)`;
    return;
  }
  const reader = new FileReader();
  reader.onerror = () => {
    els.status.classList.add('error');
    els.status.textContent = 'load failed: could not read file';
  };
  reader.onload = () => {
    try {
      const summary = JSON.parse(wasmLoadScore(new Uint8Array(reader.result)));
      if (summary.error) {
        els.status.classList.add('error');
        els.status.textContent = 'load failed: ' + summary.error;
        return;
      }
      els.status.classList.remove('error');
      fileName = file.name;
      // A new source = a fresh capture context: retitle and clear stale
      // boundary/status text so we never carry the previous file's metadata.
      els.capTitle.value = file.name.replace(/\.[^.]+$/, '');
      els.capBounds.textContent = '';
      capMsg('', false);
      els.capture.hidden = false;
      populateTracks(summary);
      arrange();
    } catch (err) {
      els.status.classList.add('error');
      els.status.textContent = 'load failed: ' + err;
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
  if (track < 0) { capMsg('load a tab and pick a track first', true); return; }
  let res;
  try { res = JSON.parse(wasmDetectBoundaries(track)); }
  catch (e) { capMsg('detect failed: ' + e, true); return; }
  if (res.error) { capMsg('detect failed: ' + res.error, true); return; }
  const n = res.boundaries.length;
  capMsg('', false); // drop any stale error from a previous failed detect
  els.capBounds.textContent =
    n ? `${n} phrase boundar${n === 1 ? 'y' : 'ies'} detected` : 'no phrase boundaries detected';
}

function downloadChunk() {
  const track = +els.track.value;
  if (track < 0) { capMsg('load a tab and pick a real track first', true); return; }
  const id = els.capId.value.trim();
  if (!id) { capMsg('a chunk ID is required (e.g. dgd_001)', true); return; }

  const now = new Date().toISOString();
  const json = wasmBuildChunk(
    track, id, els.capTitle.value, fileName, els.capTuning.value,
    +els.capCohort.value, selectedValues(els.capTags), selectedValues(els.capQuality),
    +els.capReviewer.value, +els.capRights.value, +els.capAcq.value,
    els.capRedist.checked, els.capNotes.value, now, now,
  );

  let parsed;
  try { parsed = JSON.parse(json); }
  catch (_) { capMsg('capture failed: engine returned bad JSON', true); return; }
  if (parsed.error) { capMsg('capture failed: ' + parsed.error, true); return; }

  const url = URL.createObjectURL(new Blob([json], { type: 'application/json' }));
  const a = document.createElement('a');
  a.href = url; a.download = `${id}.chunk.json`;
  document.body.appendChild(a); a.click(); a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 0);
  capMsg(`saved ${id}.chunk.json · ${parsed.boundaries.length} boundaries · ` +
    `${parsed.tags.length} tag(s) · rights recorded`, false);
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
  els.mode.addEventListener('change', arrange);
  els.track.addEventListener('change', arrange);
  els.file.addEventListener('change', (e) => {
    const f = e.target.files && e.target.files[0];
    if (f) loadFile(f);
  });
  els.gen.addEventListener('click', arrange);
  els.play.addEventListener('click', play);
  els.stop.addEventListener('click', stop);
  els.capDetect.addEventListener('click', detectBoundaries);
  els.capDownload.addEventListener('click', downloadChunk);
  window.addEventListener('resize', () => draw());
}

init().then(() => {
  bind();
  populateTagPalette();
  arrange();
  els.status.textContent = 'ready — load a tab or drag a slider, then ▶ Play';
}).catch((err) => {
  els.status.classList.add('error');
  els.status.textContent = 'failed to load engine: ' + err;
});
