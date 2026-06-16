(() => {
  'use strict';
  const $ = (id) => document.getElementById(id);
  const els = {
    mode: $('mode'), seed: $('seed'), seedOut: $('seedOut'),
    offset: $('offset'), offsetOut: $('offsetOut'),
    variation: $('variation'), varOut: $('varOut'),
    gen: $('gen'), play: $('play'), stop: $('stop'),
    roll: $('roll'), status: $('status'),
  };

  const PPQN_FALLBACK = 480;
  let wasm = null;     // wasm exports
  let current = null;  // last arrange() result (parsed JSON)
  let audio = null;    // AudioContext
  let voices = [];     // scheduled oscillators
  let playStartT = 0, playSpan = 0, raf = 0;

  // ---- engine ----
  async function initWasm() {
    const resp = await fetch('./griff_web.wasm');
    const bytes = await resp.arrayBuffer();
    // Import-free module (ADR-0024): no import object needed.
    const { instance } = await WebAssembly.instantiate(bytes, {});
    wasm = instance.exports;
  }

  function arrange() {
    if (!wasm) return;
    const mode = +els.mode.value;
    const seed = +els.seed.value;
    const offset = +els.offset.value;
    const variation = (+els.variation.value) / 100;
    const ptr = wasm.arrange(mode, seed, offset, variation);
    const len = wasm.arrange_len();
    // Read AFTER the call: arrange() may have grown linear memory.
    const view = new Uint8Array(wasm.memory.buffer, ptr, len);
    current = JSON.parse(new TextDecoder('utf-8').decode(view));
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
    els.gen.addEventListener('click', arrange);
    els.play.addEventListener('click', play);
    els.stop.addEventListener('click', stop);
    window.addEventListener('resize', () => draw());
  }

  initWasm().then(() => {
    bind();
    arrange();
    els.status.textContent = 'ready — drag a slider, then ▶ Play';
  }).catch((err) => {
    els.status.classList.add('error');
    els.status.textContent = 'failed to load engine: ' + err;
  });
})();
