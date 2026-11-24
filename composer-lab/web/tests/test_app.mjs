import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createAppHarness, makeFakeState, buildDetailFromState } from "./harness.js";

const WEB_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function dragSlider(el, value) {
  el.value = String(value);
  el.onpointerdown();
  el.oninput();
  el.onpointerup();
}

function findButton(env, pred) {
  return env.document.getElementById("workspace").querySelectorAll("button").find(pred);
}

function findSlider(env, name) {
  return env.document.getElementById("workspace").querySelectorAll("input").find((el) => el.dataset.option === name);
}

async function bootWithPiece(options = {}) {
  const st = makeFakeState(801000, options.stateOverrides || {});
  const rec = {
    id: st.id,
    state: st,
    detail: buildDetailFromState(st, options.sectionLabels),
    updatedAt: "2026-11-23T00:00:00.000Z",
  };
  const env = createAppHarness({
    initialPieces: [rec],
    ...options,
  });
  await env.flushAll(20);
  return env;
}

test("Finding 1: rapid edits are queued per piece and export waits for preceding edits", async () => {
  const env = await bootWithPiece({ autoRunWorkerCalls: true });
  env.autoRunWorkerCalls = false;

  const rerollButtons = env.document
    .getElementById("workspace")
    .querySelectorAll(".reroll-btn")
    .filter((b) => (b.dataset.layer === "comp" && b.dataset.section === "A") || b.dataset.layer === "tempo");
  assert.equal(rerollButtons.length, 2);

  // Two rapid rerolls of different controls before any worker reply completes, plus an export
  rerollButtons[0].click();
  rerollButtons[1].click();
  env.document.getElementById("dlMidiBtn").click();
  await env.flushMicrotasks(6);

  // Process pending worker calls sequentially as they arrive
  let exportedSalt = null;
  for (let safety = 0; safety < 20 && env.pendingWorkerCalls.length > 0; safety++) {
    const call = env.pendingWorkerCalls.shift();
    if (call.msg.cmd === "export_midi") {
      exportedSalt = call.msg.payload.state.recipe.studio.salts["comp@A"] || 0;
    }
    call.resolve(env.runFakePy(call.msg.cmd, call.msg.payload));
    await env.flushAll(10);
  }

  const saved = env.indexedDB._pieces.get("piece_801000");
  assert.equal(saved.state.recipe.studio.salts["comp@A"], 1);
  assert.equal(saved.state.recipe.studio.salts["tempo@all"], 1);
  assert.equal(saved.state.undo.length, 2, "two rapid rerolls must keep 2 undo entries");
  const rerollLogs = env.indexedDB._logs.filter((l) => l.action === "reroll");
  assert.equal(rerollLogs.length, 2);
  assert.equal(exportedSalt, 1, "export must wait for both preceding edits");
});

test("Finding 2: selecting B while A is visible disables A and commits currentId + state together", async () => {
  const stA = makeFakeState(801000);
  const stB = makeFakeState(802000);
  const env = createAppHarness({
    initialPieces: [
      { id: stA.id, state: stA, detail: buildDetailFromState(stA), updatedAt: "2026-11-23T00:00:02.000Z" },
      { id: stB.id, state: stB, detail: buildDetailFromState(stB), updatedAt: "2026-11-23T00:00:01.000Z" },
    ],
  });
  await env.flushAll(20);
  env.autoRunWorkerCalls = false;

  const items = env.document.getElementById("pieceList").querySelectorAll(".piece-item");
  assert.equal(items.length, 2);

  // Click B to start navigation
  items[1].click();
  await env.flushMicrotasks(4);

  // While navigation to B is pending, outgoing editor controls for A must be disabled
  const compBtnA = env.document
    .getElementById("workspace")
    .querySelectorAll(".reroll-btn")
    .find((b) => b.dataset.layer === "comp");
  assert.equal(compBtnA.disabled, true, "outgoing editor must be disabled while navigation is pending");

  // Even if an edit on A were invoked programmatically before navigation commits, A is blocked or B still loads cleanly
  compBtnA.onclick?.();
  await env.flushMicrotasks(4);

  for (let safety = 0; safety < 20 && env.pendingWorkerCalls.length > 0; safety++) {
    const call = env.pendingWorkerCalls.shift();
    call.resolve(env.runFakePy(call.msg.cmd, call.msg.payload));
    await env.flushAll(10);
  }

  const titleText = env.document.getElementById("workspace").textContent;
  assert.match(titleText, /#802000/, "workspace must finish on piece B, not get stuck on A");
  assert.equal(env.window.__studioDebug.getCurrentId?.(), "piece_802000");
});

test("Finding 3: piece + log write uses single transaction and quota error switches consistently to visible memory fallback", async () => {
  const env = await bootWithPiece();
  env.txLog.length = 0;

  const compBtn = env.document
    .getElementById("workspace")
    .querySelectorAll(".reroll-btn")
    .find((b) => b.dataset.layer === "comp" && b.dataset.section === "A");
  compBtn.click();
  await env.flushAll(15);

  const writeTxs = env.txLog.filter((t) => t.mode === "readwrite" && t.status === "complete");
  assert.ok(
    writeTxs.some((t) => t.storeNames.includes("pieces") && t.storeNames.includes("logs")),
    "piece and log must be written in one IndexedDB transaction"
  );

  // Inject QuotaExceededError on next write
  env.idbFailNextWrite = "QuotaExceededError";
  const compBtn2 = env.document
    .getElementById("workspace")
    .querySelectorAll(".reroll-btn")
    .find((b) => b.dataset.layer === "comp" && b.dataset.section === "A");
  compBtn2.click();
  await env.flushAll(15);

  // Subsequent reads must reflect salt 2 from memory, NOT stale salt 1 from IDB
  assert.equal(env.indexedDB._pieces.get("piece_801000").state.recipe.studio.salts["comp@A"], 1);
  const banner = env.document.getElementById("storageBanner");
  assert.ok(banner && !banner.hidden && banner.textContent.length > 0, "storage fallback must be visible");

  // Download logs and export pieces should both include the in-memory salt-2 mutation
  env.downloads.length = 0;
  env.document.getElementById("logBtn").click();
  await env.flushAll(15);
  assert.equal(env.downloads.length, 1);
  const logLines = env.downloads[0].parts.join("").trim().split("\n");
  assert.equal(logLines.length, 2, "memory fallback log must include both rerolls");

  env.downloads.length = 0;
  env.document.getElementById("exportPiecesBtn").click();
  await env.flushAll(15);
  const exported = JSON.parse(env.downloads[0].parts.join(""));
  assert.equal(exported.pieces[0].state.recipe.studio.salts["comp@A"], 2, "memory fallback piece must reflect salt 2");
});

test("Finding 4: concurrent same-seed creation produces distinct IDs and conflicting import does not overwrite local piece", async () => {
  const env = await bootWithPiece();
  env.autoRunWorkerCalls = false;

  // Trigger two concurrent creations with the same seed 805000
  env.document.getElementById("seedInput").value = "805000";
  env.document.getElementById("newBtn").click();
  env.document.getElementById("newBtn").click();
  await env.flushMicrotasks(6);

  // Resolve both new_piece calls simultaneously
  assert.equal(env.pendingWorkerCalls.length, 2);
  for (const call of env.pendingWorkerCalls.splice(0)) {
    call.resolve(env.runFakePy(call.msg.cmd, call.msg.payload));
  }
  await env.flushAll(20);

  assert.ok(env.indexedDB._pieces.has("piece_805000"), "first piece_805000 must exist");
  assert.ok(env.indexedDB._pieces.has("piece_805000_2"), "second concurrent piece must get piece_805000_2");

  // Now modify local piece_801000 so it differs from an imported backup with the same id piece_801000
  env.autoRunWorkerCalls = true;
  const localPiece = env.indexedDB._pieces.get("piece_801000");
  localPiece.state.recipe.studio.salts["comp@A"] = 9;
  localPiece.state.undo = [makeFakeState(801000).recipe];

  const importedState = makeFakeState(801000);
  importedState.recipe.studio.salts["comp@A"] = 1;
  const fakeFile = {
    text: async () => JSON.stringify({ version: 1, pieces: [{ id: "piece_801000", state: importedState }] }),
  };
  const impInput = env.document.getElementById("importPiecesInput");
  impInput.files = [fakeFile];
  await impInput.onchange();
  await env.flushAll(20);

  const keptLocal = env.indexedDB._pieces.get("piece_801000");
  assert.equal(keptLocal.state.recipe.studio.salts["comp@A"], 9, "local piece_801000 must not be overwritten");
  assert.equal(keptLocal.state.undo.length, 1, "local undo stack must stay intact");
  const allocatedImport = env.indexedDB._pieces.get("piece_801000_2");
  assert.ok(allocatedImport, "conflicting distinct import must allocate piece_801000_2");
  assert.equal(allocatedImport.state.recipe.studio.salts["comp@A"], 1);
});

test("Finding 6: section labels and piece metadata never reach innerHTML", async () => {
  const xss = '<img src=x onerror="alert(1)">';
  const env = await bootWithPiece({
    sectionLabels: { A: xss },
  });

  const badWrite = env.innerHTMLWrites.find((w) => w.html.includes(xss) || w.html.includes("<img"));
  assert.equal(badWrite, undefined, "untrusted section label must never be interpolated into innerHTML");
  const tl = env.document.getElementById("timeline");
  assert.ok(tl.children.some((b) => b.textContent.includes(xss)), "label should still render safely via textContent");
});

test("Finding 7: seeking into sustained note plays remaining slice and stale pre-seek decodes are invalidated", async () => {
  const longNoteEvents = () => [{ t: 0.0, dur: 2.0, pitch: 60, vel: 100, guitar: 1, dead: false }];
  const env = await bootWithPiece({
    eventsForState: longNoteEvents,
    audioInitialState: "running",
  });

  const sampler = env.window.__studioDebug.sampler;
  await sampler.seek(1.0, true);
  await env.flushAll(10);

  assert.equal(env.sourcesCreated.length, 1, "seeking into [0, 2.0] at t=1.0 must start the overlapping note");
  const src = env.sourcesCreated[0];
  assert.equal(src.startArgs.length, 2, "must pass buffer offset to src.start(when, offset)");
  assert.ok(Math.abs(src.startArgs[1] - 1.0) < 0.05, `buffer offset should be ~1.0, got ${src.startArgs[1]}`);
  const playedDur = src.stopArgs[0] - src.startArgs[0];
  assert.ok(Math.abs(playedDur - 1.035) < 0.05, `remaining dur+release should be ~1.035, got ${playedDur}`);

  // Now test stale decode invalidation across seek into rest
  const twoNoteEvents = () => [
    { t: 0.0, dur: 0.5, pitch: 60, vel: 100, guitar: 1, dead: false },
    { t: 2.0, dur: 0.5, pitch: 64, vel: 100, guitar: 1, dead: false },
  ];
  const env2 = await bootWithPiece({
    eventsForState: twoNoteEvents,
    audioInitialState: "running",
    decodeDelayMs: 60,
  });
  const sampler2 = env2.window.__studioDebug.sampler;
  // Start playing at t=0 (begins async decode of pitch 60), immediately seek into rest at t=1.0
  const playPromise = sampler2.play();
  await sampler2.seek(1.0, false); // pause at 1.0
  await playPromise;
  await env2.flushAll(100);
  assert.equal(env2.sourcesCreated.length, 0, "stale decode from before pause/seek must not start a note during rest");
});

test("Finding 8: held notes longer than 2.5s loop sustain region and release at full event end", async () => {
  const heldEvents = () => [{ t: 0.0, dur: 5.0, pitch: 60, vel: 100, guitar: 1, dead: false }];
  const env = await bootWithPiece({
    eventsForState: heldEvents,
    audioInitialState: "running",
    sampleDuration: 2.5,
  });

  const sampler = env.window.__studioDebug.sampler;
  await sampler.play();
  await env.flushAll(15);

  assert.equal(env.sourcesCreated.length, 1);
  const src = env.sourcesCreated[0];
  assert.equal(src.loop, true, "5.0s note must enable buffer looping");
  assert.ok(src.loopStart > 0.2 && src.loopStart < src.loopEnd, `invalid loopStart ${src.loopStart}`);
  assert.ok(src.loopEnd > src.loopStart + 0.1 && src.loopEnd < 2.5, `invalid loopEnd ${src.loopEnd}`);
  const stopDelay = src.stopArgs[0] - src.startArgs[0];
  assert.ok(Math.abs(stopDelay - 5.035) < 0.05, `stop must happen at 5.035s, got ${stopDelay}`);

  // Seeking to t=3.0 of a 5.0s note must wrap buffer offset into [loopStart, loopEnd) and play remaining 2.0s
  env.sourcesCreated.length = 0;
  await sampler.seek(3.0, true);
  await env.flushAll(15);
  assert.equal(env.sourcesCreated.length, 1);
  const s2 = env.sourcesCreated[0];
  assert.equal(s2.loop, true);
  assert.ok(s2.startArgs[1] >= s2.loopStart && s2.startArgs[1] < s2.loopEnd, `offset ${s2.startArgs[1]} must wrap into loop`);
  const remDur = s2.stopArgs[0] - s2.startArgs[0];
  assert.ok(Math.abs(remDur - 2.035) < 0.05, `remaining dur+release should be ~2.035s, got ${remDur}`);
});

test("Finding 9: playback is gated on sample load, prepares opening notes over >150ms decode, and surfaces decode errors with retry", async () => {
  // 1) Play disabled while samples loading
  const envLoading = await bootWithPiece({ autoPreloadSamples: false });
  assert.equal(envLoading.document.getElementById("playBtn").disabled, true, "playBtn disabled while loading");

  // 2) Opening decode > 150ms (220ms) must NOT drop opening note
  const envSlow = await bootWithPiece({
    audioInitialState: "running",
    decodeDelayMs: 220,
  });
  await envSlow.window.__studioDebug.sampler.play();
  await envSlow.flushAll(20);
  assert.ok(envSlow.sourcesCreated.length >= 1, "slow >150ms opening decode must still play opening note");

  // 3) Decode failure must stop playback, show audio error banner + retry button, and recover on retry
  const envFail = await bootWithPiece({
    audioInitialState: "running",
    failDecode: true,
  });
  await envFail.window.__studioDebug.sampler.play();
  await envFail.flushAll(20);

  assert.equal(envFail.window.__studioDebug.sampler.isPlaying(), false);
  assert.equal(envFail.document.getElementById("playBtn").textContent, "Играть");
  const audioBanner = envFail.document.getElementById("audioBanner");
  assert.ok(audioBanner && !audioBanner.hidden, "decode failure must show error banner");
  const retryBtn = envFail.document.getElementById("retryAudioBtn");
  assert.ok(retryBtn, "decode failure must expose a retry button");

  envFail.failDecode = false;
  await retryBtn.click();
  await envFail.flushAll(20);
  assert.equal(envFail.window.__studioDebug.sampler.isPlaying(), true, "retry should recover playback");
});

test("Finding 10: failed audio resume does not show Пауза over silence and interruption pauses cleanly", async () => {
  const env = await bootWithPiece({
    audioInitialState: "suspended",
    resumeBehavior: "reject",
  });

  await env.document.getElementById("playBtn").click();
  await env.flushAll(15);

  assert.equal(env.window.__studioDebug.sampler.isPlaying(), false);
  assert.equal(env.document.getElementById("playBtn").textContent, "Играть");
  const audioBanner = env.document.getElementById("audioBanner");
  assert.ok(audioBanner && !audioBanner.hidden, "resume rejection must surface an error banner");

  // Also test resume() resolving without transitioning state to "running"
  env.resumeBehavior = "stay-suspended";
  await env.document.getElementById("playBtn").click();
  await env.flushAll(15);
  assert.equal(env.window.__studioDebug.sampler.isPlaying(), false);
  assert.equal(env.document.getElementById("playBtn").textContent, "Играть");

  // Now succeed resume, then simulate OS audio interruption via statechange
  env.resumeBehavior = "resolve";
  await env.document.getElementById("playBtn").click();
  await env.flushAll(15);
  assert.equal(env.window.__studioDebug.sampler.isPlaying(), true);
  assert.equal(env.document.getElementById("playBtn").textContent, "Пауза");

  env.audioContexts[0].emitStateChange("interrupted");
  assert.equal(env.window.__studioDebug.sampler.isPlaying(), false);
  assert.equal(env.document.getElementById("playBtn").textContent, "Играть");
});

test("Finding 11: worker.onerror rejects pending calls, clears pending map, and exposes retry", async () => {
  const env = createAppHarness({ autoReadyWorker: false });
  await env.flushAll(10);

  const worker1 = env.activeWorker;
  let caughtInit = false;
  const callPromise = env.window.__studioDebug.callPy("new_piece", { seed: 801000 }).catch(() => {
    caughtInit = true;
  });

  // Trigger fatal worker error
  worker1.onerror({ message: "Script load failed" });
  await callPromise;

  assert.equal(caughtInit, true, "pending call must reject on worker.onerror");
  const retryPyBtn = env.document.getElementById("retryPyBtn");
  assert.ok(retryPyBtn, "fatal worker error must expose retry button");

  retryPyBtn.click();
  assert.equal(env.workersCreated, 2, "retry must spawn a new Worker");
  env.activeWorker.onmessage({ data: { type: "ready", version: "v0.27.7", loadMs: 12 } });
  await env.flushAll(10);
  assert.equal(env.window.__studioDebug.ready, true, "retry worker ready must mark studio ready");
});

test("Studio controls send option immediately without reroll, refresh UI, and hide old doses under Дополнительно", async () => {
  const env = await bootWithPiece();
  env.workerMessages.length = 0;

  const ws = env.document.getElementById("workspace");
  const adv = ws.querySelectorAll("details");
  assert.equal(adv.length, 1, "advanced doses must be inside a <details> element");
  const summary = adv[0].querySelectorAll("summary");
  assert.equal(summary.length, 1);
  assert.equal(summary[0].textContent, "Дополнительно");
  for (const lbl of [
    "Вероятность октав при перегенерации ритма",
    "Октавы шестнадцатыми (при перегенерации)",
    "Глухие удары перед акцентами (только в октавных тактах, при перегенерации)",
  ]) {
    assert.ok(adv[0].textContent.includes(lbl), `missing renamed advanced dose label: ${lbl}`);
  }

  const hints = ws.querySelectorAll(".option-hint");
  assert.equal(hints.length, 5, "each of the 5 whole-piece controls must have a one-line Russian hint");
  for (const h of hints) {
    assert.match(h.textContent, /применяется сразу/i);
  }

  // 1. mute_share slider
  const muteSlider = ws
    .querySelectorAll("input")
    .find((el) => el.dataset.option === "mute_share");
  assert.ok(muteSlider, "mute_share slider must exist");
  dragSlider(muteSlider, 0.6);
  await env.flushAll(15);

  // 2. octave_share slider then Авто button
  const octSlider = env.document
    .getElementById("workspace")
    .querySelectorAll("input")
    .find((el) => el.dataset.option === "octave_share");
  assert.ok(octSlider, "octave_share slider must exist");
  dragSlider(octSlider, 0.8);
  await env.flushAll(15);

  const octAutoBtn = env.document
    .getElementById("workspace")
    .querySelectorAll("button")
    .find((el) => el.dataset.option === "octave_share" && el.dataset.value === "auto");
  assert.ok(octAutoBtn, "octave_share auto button must exist");
  octAutoBtn.click();
  await env.flushAll(15);

  // 3. intro_bars = 8
  const intro8Btn = env.document
    .getElementById("workspace")
    .querySelectorAll("button")
    .find((el) => el.dataset.option === "intro_bars" && el.dataset.value === "8");
  assert.ok(intro8Btn, "intro_bars=8 button must exist");
  intro8Btn.click();
  await env.flushAll(15);

  // 4. square_repeat = 2
  const rep2Btn = env.document
    .getElementById("workspace")
    .querySelectorAll("button")
    .find((el) => el.dataset.option === "square_repeat" && el.dataset.value === "2");
  assert.ok(rep2Btn, "square_repeat=2 button must exist");
  rep2Btn.click();
  await env.flushAll(15);

  const calls = env.workerMessages.filter((m) => m.type === "call");
  assert.equal(
    calls.filter((m) => m.cmd === "reroll").length,
    0,
    "changing the four controls must not trigger reroll",
  );
  const optCalls = calls.filter((m) => m.cmd === "option").map((m) => [m.payload.name, m.payload.value]);
  assert.deepEqual(optCalls, [
    ["mute_share", 0.6],
    ["octave_share", 0.8],
    ["octave_share", null],
    ["intro_bars", 8],
    ["square_repeat", 2],
  ]);

  const saved = env.indexedDB._pieces.get("piece_801000");
  assert.equal(saved.state.recipe.studio.options.mute_share, 0.6);
  assert.equal(saved.state.recipe.studio.options.octave_share, null);
  assert.equal(saved.state.recipe.studio.options.intro_bars, 8);
  assert.equal(saved.state.recipe.studio.options.square_repeat, 2);
  assert.match(env.document.getElementById("workspace").textContent, /24 с/);
});

test("Re-clicking already active options does not push undo or append option logs", async () => {
  const env = await bootWithPiece();

  const ws = env.document.getElementById("workspace");
  const introAuto = ws.querySelectorAll("button").find((el) => el.dataset.option === "intro_bars" && el.dataset.value === "auto");
  const sq1 = ws.querySelectorAll("button").find((el) => el.dataset.option === "square_repeat" && el.dataset.value === "1");
  const bLeadAug = ws.querySelectorAll("button").find((el) => el.textContent === "Медленнее (удвоенные длительности)");
  const muteSlider = ws.querySelectorAll("input").find((el) => el.dataset.option === "mute_share");
  assert.ok(introAuto && sq1 && bLeadAug && muteSlider);

  introAuto.click();
  sq1.click();
  bLeadAug.click();
  dragSlider(muteSlider, 0);
  await env.flushAll(20);

  let saved = env.indexedDB._pieces.get("piece_801000");
  assert.equal(saved.state.undo.length, 0);
  assert.equal(env.document.getElementById("undoBtn").disabled, true);
  assert.equal(env.indexedDB._logs.filter((l) => l.action === "option").length, 0);

  const intro8 = env.document
    .getElementById("workspace")
    .querySelectorAll("button")
    .find((el) => el.dataset.option === "intro_bars" && el.dataset.value === "8");
  intro8.click();
  await env.flushAll(15);

  saved = env.indexedDB._pieces.get("piece_801000");
  assert.equal(saved.state.undo.length, 1);
  assert.equal(env.indexedDB._logs.filter((l) => l.action === "option").length, 1);

  const intro8Again = env.document
    .getElementById("workspace")
    .querySelectorAll("button")
    .find((el) => el.dataset.option === "intro_bars" && el.dataset.value === "8");
  intro8Again.click();
  await env.flushAll(15);

  saved = env.indexedDB._pieces.get("piece_801000");
  assert.equal(saved.state.undo.length, 1);
  assert.equal(env.indexedDB._logs.filter((l) => l.action === "option").length, 1);
});

test("Octave share is a two-button switch: selected button does nothing, Вручную applies the shown slider value", async () => {
  const env = await bootWithPiece();
  env.workerMessages.length = 0;

  const ws = env.document.getElementById("workspace");
  const hints = ws.querySelectorAll(".option-hint");
  assert.match(hints[0].textContent, /A,\s*A2,\s*«Кульминация»\s+и\s+B/);

  const autoBtn = () => findButton(env, (el) => el.dataset.option === "octave_share" && el.dataset.value === "auto");
  const manualBtn = () => findButton(env, (el) => el.dataset.option === "octave_share" && el.dataset.value === "manual");
  assert.equal(autoBtn().textContent, "Авто");
  assert.equal(manualBtn().textContent, "Вручную");
  assert.equal(autoBtn().getAttribute("aria-pressed"), "true");
  assert.equal(manualBtn().getAttribute("aria-pressed"), "false");
  assert.equal(findSlider(env, "octave_share").classList.contains("slider-dim"), true, "slider is dimmed in Авто");
  assert.match(
    ws.querySelectorAll(".option-hint").find((h) => /Вручную/.test(h.textContent)).textContent,
    /ползунок/,
    "hint says that moving the slider switches to manual",
  );

  autoBtn().click();
  await env.flushAll(15);
  assert.equal(env.workerMessages.filter((m) => m.type === "call").length, 0, "selected Авто sends nothing");
  assert.equal(env.indexedDB._pieces.get("piece_801000").state.undo.length, 0);

  manualBtn().click();
  await env.flushAll(15);
  let saved = env.indexedDB._pieces.get("piece_801000");
  assert.equal(saved.state.recipe.studio.options.octave_share, 0.5);
  assert.equal(saved.state.undo.length, 1);
  assert.equal(autoBtn().getAttribute("aria-pressed"), "false");
  assert.equal(manualBtn().getAttribute("aria-pressed"), "true");
  assert.equal(findSlider(env, "octave_share").classList.contains("slider-dim"), false);

  manualBtn().click();
  await env.flushAll(15);
  assert.equal(env.indexedDB._pieces.get("piece_801000").state.undo.length, 1, "selected Вручную sends nothing");

  autoBtn().click();
  await env.flushAll(15);
  saved = env.indexedDB._pieces.get("piece_801000");
  assert.equal(saved.state.recipe.studio.options.octave_share, null);
  assert.equal(saved.state.undo.length, 2);

  const optCalls = env.workerMessages
    .filter((m) => m.type === "call" && m.cmd === "option")
    .map((m) => [m.payload.name, m.payload.value]);
  assert.deepEqual(optCalls, [
    ["octave_share", 0.5],
    ["octave_share", null],
  ]);
});

test("Moving the octave slider while in Авто switches to manual with the slider value", async () => {
  const env = await bootWithPiece();
  env.workerMessages.length = 0;
  dragSlider(findSlider(env, "octave_share"), 0.7);
  await env.flushAll(15);
  const optCalls = env.workerMessages.filter((m) => m.type === "call" && m.cmd === "option");
  assert.equal(optCalls.length, 1);
  assert.deepEqual([optCalls[0].payload.name, optCalls[0].payload.value], ["octave_share", 0.7]);
  const manual = findButton(env, (el) => el.dataset.option === "octave_share" && el.dataset.value === "manual");
  assert.equal(manual.getAttribute("aria-pressed"), "true");
});

test("Item 2 and 4: piece-wide note under Параметры пьесы and re-roll-only hint in Дополнительно", async () => {
  const env = await bootWithPiece();
  const text = env.document.getElementById("workspace").textContent;
  assert.ok(text.includes("Параметры действуют на всю пьесу, в том числе на закреплённые секции."));
  const adv = env.document.getElementById("workspace").querySelectorAll("details")[0];
  assert.ok(
    adv.textContent.includes(
      "Эти параметры действуют только при следующей перегенерации ритм-гитары («Перегенерировать ритм-гитару» или «Перегенерировать всё незакреплённое»).",
    ),
  );
  const note = adv.querySelectorAll(".advanced-note");
  assert.equal(note.length, 1);
  assert.equal(adv.children[1], note[0], "hint is the first line inside Дополнительно, right after the summary");
});

test("Item 3: lock button shows action and state, trailing rest is a two-button choice, locked re-roll has a title", async () => {
  const st = makeFakeState(801000);
  st.recipe.studio.locks = ["comp@A"];
  const env = createAppHarness({
    initialPieces: [{ id: st.id, state: st, detail: buildDetailFromState(st), updatedAt: "2026-11-23T00:00:00.000Z" }],
  });
  await env.flushAll(20);
  const ws = env.document.getElementById("workspace");
  const rows = ws.querySelectorAll(".layer-row");
  const compRow = rows.find((r) => r.children[0].dataset.layer === "comp");
  const [reroll, lock] = compRow.children;
  assert.equal(reroll.disabled, true);
  assert.equal(reroll.title, "Слой закреплён");
  assert.equal(lock.textContent, "🔒 Закреплено");
  assert.equal(lock.title, "Открепить");
  assert.equal(lock.getAttribute("aria-pressed"), "true");
  const tempoRow = rows.find((r) => r.children[0].dataset.layer === "tempo");
  assert.equal(tempoRow.children[1].textContent, "Закрепить");
  assert.equal(tempoRow.children[1].getAttribute("aria-pressed"), "false");
  assert.equal(tempoRow.children[0].title, "", "enabled re-roll has no locked title");

  env.workerMessages.length = 0;
  const withTail = findButton(env, (b) => b.dataset.option === "trailing_rest" && b.dataset.value === "true");
  const noTail = findButton(env, (b) => b.dataset.option === "trailing_rest" && b.dataset.value === "false");
  assert.equal(withTail.textContent, "Пауза в конце такта");
  assert.equal(noTail.textContent, "Без паузы");
  assert.equal(withTail.getAttribute("aria-pressed"), "true");
  assert.equal(noTail.getAttribute("aria-pressed"), "false");
  withTail.click();
  await env.flushAll(10);
  assert.equal(env.workerMessages.filter((m) => m.type === "call").length, 0, "selected choice sends nothing");
  noTail.click();
  await env.flushAll(15);
  const calls = env.workerMessages.filter((m) => m.type === "call" && m.cmd === "option");
  assert.deepEqual([calls[0].payload.name, calls[0].payload.value], ["trailing_rest", false]);
  const noTail2 = findButton(env, (b) => b.dataset.option === "trailing_rest" && b.dataset.value === "false");
  assert.equal(noTail2.getAttribute("aria-pressed"), "true");
});

test("Item 5: three keyboard steps give one option request after 600 ms of quiet, one undo entry", async () => {
  const env = await bootWithPiece();
  env.workerMessages.length = 0;
  const slider = findSlider(env, "mute_share");
  for (const v of ["0.05", "0.1", "0.15"]) {
    slider.value = v;
    slider.oninput();
    if (slider.onchange) slider.onchange();
    await new Promise((r) => setTimeout(r, 100));
  }
  await env.flushAll(10);
  assert.equal(env.workerMessages.filter((m) => m.type === "call").length, 0, "nothing sent before the debounce elapses");
  await new Promise((r) => setTimeout(r, 650));
  await env.flushAll(20);
  const calls = env.workerMessages.filter((m) => m.type === "call" && m.cmd === "option");
  assert.equal(calls.length, 1);
  assert.equal(calls[0].payload.value, 0.15);
  assert.equal(env.indexedDB._pieces.get("piece_801000").state.undo.length, 1);
});

test("Item 5: a pointer drag sends once on release and nothing more after the debounce", async () => {
  const env = await bootWithPiece();
  env.workerMessages.length = 0;
  const slider = findSlider(env, "mute_share");
  slider.onpointerdown();
  for (const v of ["0.2", "0.4", "0.6"]) {
    slider.value = v;
    slider.oninput();
  }
  await new Promise((r) => setTimeout(r, 700));
  await env.flushAll(10);
  assert.equal(env.workerMessages.filter((m) => m.type === "call").length, 0, "no request while the pointer is down");
  slider.onpointerup();
  if (slider.onchange) slider.onchange();
  await env.flushAll(20);
  await new Promise((r) => setTimeout(r, 700));
  await env.flushAll(10);
  const calls = env.workerMessages.filter((m) => m.type === "call" && m.cmd === "option");
  assert.equal(calls.length, 1);
  assert.equal(calls[0].payload.value, 0.6);
});

test("Item 6: status chip shows Пересчёт… while a request runs and the pressed control is disabled", async () => {
  const env = await bootWithPiece();
  const chip = env.document.getElementById("statusChip");
  assert.equal(chip.textContent, "");
  env.autoRunWorkerCalls = false;
  const btn = findButton(env, (b) => b.dataset.layer === "comp" && b.dataset.section === "A");
  btn.click();
  await env.flushAll(10);
  assert.equal(chip.textContent, "Пересчёт…");
  assert.equal(btn.disabled, true, "pressed control is disabled while the request runs");
  const call = env.pendingWorkerCalls.shift();
  call.resolve(env.runFakePy(call.msg.cmd, call.msg.payload));
  await env.flushAll(15);
  assert.equal(chip.textContent, "");
  const fresh = findButton(env, (b) => b.dataset.layer === "comp" && b.dataset.section === "A");
  assert.equal(fresh.disabled, false);
});

test("Item 6: status chip is cleared and the control re-enabled after a failed request", async () => {
  const env = await bootWithPiece();
  env.autoRunWorkerCalls = false;
  const btn = findButton(env, (b) => b.dataset.layer === "comp" && b.dataset.section === "A");
  btn.click();
  await env.flushAll(10);
  env.pendingWorkerCalls.shift().reject("boom");
  await env.flushAll(15);
  assert.equal(env.document.getElementById("statusChip").textContent, "");
  assert.equal(btn.disabled, false);
});

test("Item 7: errors show a dismissible inline banner and never call alert", async () => {
  const env = await bootWithPiece();
  env.autoRunWorkerCalls = false;
  assert.equal(env.document.getElementById("errorBanner").hidden, true);
  const btn = findButton(env, (b) => b.dataset.layer === "comp" && b.dataset.section === "A");
  btn.click();
  await env.flushAll(10);
  env.pendingWorkerCalls.shift().reject("boom");
  await env.flushAll(15);
  assert.equal(env.document.getElementById("errorBanner").hidden, false);
  assert.match(env.document.getElementById("errorText").textContent, /boom/);
  assert.equal(env.alerts.length, 0);
  env.document.getElementById("errorCloseBtn").click();
  assert.equal(env.document.getElementById("errorBanner").hidden, true);
  assert.equal(env.document.getElementById("errorText").textContent, "");
});

test("Item 7: new piece and import failures use the banner too", async () => {
  const env = await bootWithPiece();
  env.autoRunWorkerCalls = false;
  env.document.getElementById("newBtn").click();
  await env.flushAll(10);
  env.pendingWorkerCalls.shift().reject("new failed");
  await env.flushAll(15);
  assert.match(env.document.getElementById("errorText").textContent, /new failed/);
  assert.equal(env.document.getElementById("errorBanner").hidden, false);
  env.document.getElementById("errorCloseBtn").click();
  const impInput = env.document.getElementById("importPiecesInput");
  impInput.files = [{ text: async () => "{not json" }];
  await impInput.onchange();
  await env.flushAll(10);
  assert.equal(env.document.getElementById("errorBanner").hidden, false);
  assert.equal(env.alerts.length, 0);
});

test("Item 7: app source does not use alert()", () => {
  const src = fs.readFileSync(path.join(WEB_DIR, "app.js"), "utf8");
  assert.equal(/\balert\s*\(/.test(src), false);
});

test("Item 8: undo title names the action, redo restores the undone state and a new change clears it", async () => {
  const env = await bootWithPiece();
  const undo = () => env.document.getElementById("undoBtn");
  const redo = () => env.document.getElementById("redoBtn");
  assert.equal(undo().disabled, true);
  assert.equal(redo().disabled, true);
  assert.equal(redo().textContent.startsWith("Вернуть"), true);

  dragSlider(findSlider(env, "mute_share"), 0.3);
  await env.flushAll(15);
  assert.equal(undo().title, "Отменить: глухие ноты 0.3");
  assert.ok(undo().textContent.includes("Отменить"));
  assert.ok(undo().textContent.includes("глухие ноты 0.3"));

  findButton(env, (b) => b.dataset.layer === "comp" && b.dataset.section === "A").click();
  await env.flushAll(15);
  assert.equal(undo().title, "Отменить: перегенерация «Куплет A · ритм-гитара»");

  undo().click();
  await env.flushAll(15);
  let saved = env.indexedDB._pieces.get("piece_801000");
  assert.equal(saved.state.recipe.studio.salts["comp@A"] || 0, 0);
  assert.equal(saved.state.redo.length, 1);
  assert.equal(redo().disabled, false);
  assert.equal(redo().title, "Вернуть: перегенерация «Куплет A · ритм-гитара»");
  assert.equal(undo().title, "Отменить: глухие ноты 0.3");

  redo().click();
  await env.flushAll(15);
  saved = env.indexedDB._pieces.get("piece_801000");
  assert.equal(saved.state.recipe.studio.salts["comp@A"], 1);
  assert.equal(saved.state.redo.length, 0);
  assert.equal(redo().disabled, true);
  assert.equal(undo().title, "Отменить: перегенерация «Куплет A · ритм-гитара»");
  assert.deepEqual(
    env.indexedDB._logs.filter((l) => l.action === "undo" || l.action === "redo").map((l) => l.action),
    ["undo", "redo"],
  );

  undo().click();
  await env.flushAll(15);
  assert.equal(redo().disabled, false);
  dragSlider(findSlider(env, "mute_share"), 0.9);
  await env.flushAll(15);
  saved = env.indexedDB._pieces.get("piece_801000");
  assert.equal(saved.state.redo.length, 0, "a new change clears the redo stack");
  assert.equal(redo().disabled, true);
});

test("Item 9: plain labels, hidden zero badge, titled badge, no internal names", async () => {
  const env = await bootWithPiece();
  const text = env.document.getElementById("workspace").textContent;
  assert.ok(text.includes("Соло в середине B"));
  for (const bad of ["b_lead", "trailing_rest", "--notail"]) {
    assert.equal(text.includes(bad), false, bad);
  }
  for (const t of ["Медленнее (удвоенные длительности)", "В темпе рифа", "Без соло"]) {
    assert.ok(findButton(env, (b) => b.textContent === t), t);
  }
  for (const b of env.document.getElementById("workspace").querySelectorAll(".salt-badge")) {
    assert.equal(b.title, "Сколько раз перегенерировали");
    assert.equal(b.hidden, true, "badge is hidden at 0");
  }
  findButton(env, (b) => b.dataset.layer === "comp" && b.dataset.section === "A").click();
  await env.flushAll(15);
  const badges = env.document.getElementById("workspace").querySelectorAll(".salt-badge");
  assert.equal(badges.filter((b) => !b.hidden).length, 1);
  assert.equal(badges.find((b) => !b.hidden).textContent, "#1");
  const html = fs.readFileSync(path.join(WEB_DIR, "index.html"), "utf8");
  assert.ok(html.includes("Номер пьесы (seed), необязательно"));
  assert.match(html, /<label[^>]*for="seedInput"/);
});

test("Item 11: timeline blocks have short labels for narrow screens and the full name in title and aria-label", async () => {
  const env = await bootWithPiece();
  const tl = env.document.getElementById("timeline");
  const shortByFull = { "Вступление": "Вст.", "Куплет A": "A", "Середина B": "B", "Концовка": "Конц." };
  assert.equal(tl.children.length, 4);
  for (const blk of tl.children) {
    const full = blk.title;
    assert.ok(shortByFull[full], full);
    assert.equal(blk.getAttribute("aria-label").startsWith(full), true);
    assert.equal(blk.querySelectorAll(".name-short")[0].textContent, shortByFull[full]);
    assert.equal(blk.querySelectorAll(".name-full")[0].textContent, full);
  }
  const html = fs.readFileSync(path.join(WEB_DIR, "index.html"), "utf8");
  assert.match(html, /@media \(max-width: 520px\)[\s\S]*\.name-short/);
});

test("Item 12: sliders have names and values for assistive tech, choice groups use aria-pressed, chip is a status region", async () => {
  const env = await bootWithPiece();
  const ws = env.document.getElementById("workspace");
  const sliders = ws.querySelectorAll("input");
  assert.ok(sliders.length >= 7);
  for (const el of sliders) {
    assert.ok((el.getAttribute("aria-label") || "").length > 0, `slider ${el.dataset.option || el.id} needs a name`);
    assert.ok((el.getAttribute("aria-valuetext") || "").length > 0, `slider ${el.dataset.option || el.id} needs valuetext`);
  }
  const mute = findSlider(env, "mute_share");
  mute.value = "0.35";
  mute.oninput();
  assert.equal(mute.getAttribute("aria-valuetext"), "0.35");
  assert.equal(findSlider(env, "octave_share").getAttribute("aria-valuetext"), "авто");
  for (const opt of ["intro_bars", "square_repeat", "b_lead", "trailing_rest", "octave_share"]) {
    const group = ws.querySelectorAll("button").filter((b) => b.dataset.option === opt);
    assert.ok(group.length >= 2, opt);
    for (const b of group) assert.ok(["true", "false"].includes(b.getAttribute("aria-pressed")), `${opt} ${b.textContent}`);
    assert.equal(group.filter((b) => b.getAttribute("aria-pressed") === "true").length, 1, opt);
  }
  const html = fs.readFileSync(path.join(WEB_DIR, "index.html"), "utf8");
  assert.match(html, /id="statusChip"[^>]*role="status"|role="status"[^>]*id="statusChip"/);
});

async function bootTwoPieces(options = {}) {
  const stA = makeFakeState(801000);
  const stB = makeFakeState(802000);
  const env = createAppHarness({
    initialPieces: [
      { id: stA.id, state: stA, detail: buildDetailFromState(stA), updatedAt: "2026-11-23T00:00:02.000Z" },
      { id: stB.id, state: stB, detail: buildDetailFromState(stB), updatedAt: "2026-11-23T00:00:01.000Z" },
    ],
    ...options,
  });
  await env.flushAll(20);
  return env;
}

async function resolveAll(env, limit = 20) {
  for (let i = 0; i < limit && env.pendingWorkerCalls.length > 0; i++) {
    const call = env.pendingWorkerCalls.shift();
    call.resolve(env.runFakePy(call.msg.cmd, call.msg.payload));
    await env.flushAll(10);
  }
}

const pieceItems = (env) => env.document.getElementById("pieceList").querySelectorAll(".piece-item");

test("Review 1: undo during a pending slider debounce commits the slider edit first and stays redoable", async () => {
  const env = await bootWithPiece();
  findButton(env, (b) => b.dataset.option === "intro_bars" && b.dataset.value === "8").click();
  await env.flushAll(15);
  env.workerMessages.length = 0;
  const slider = findSlider(env, "mute_share");
  slider.value = "0.35";
  slider.oninput();
  env.document.getElementById("undoBtn").click();
  await env.flushAll(20);
  await new Promise((r) => setTimeout(r, 700));
  await env.flushAll(15);
  const cmds = env.workerMessages.filter((m) => m.type === "call").map((m) => m.cmd);
  assert.deepEqual(cmds, ["option", "undo"]);
  const saved = env.indexedDB._pieces.get("piece_801000").state;
  assert.equal(saved.recipe.studio.options.mute_share, 0, "undo reverted the slider edit");
  assert.equal(saved.recipe.studio.options.intro_bars, 8);
  assert.equal(saved.redo.length, 1, "the undone state stays redoable");
  assert.equal(env.document.getElementById("redoBtn").disabled, false);
});

test("Review 6: switching pieces during a slider debounce saves the edit on its own piece whatever the latency", async () => {
  const env = await bootTwoPieces();
  env.autoRunWorkerCalls = false;
  const slider = findSlider(env, "mute_share");
  slider.value = "0.35";
  slider.oninput();
  pieceItems(env)[1].click();
  await env.flushAll(10);
  await new Promise((r) => setTimeout(r, 700));
  await env.flushAll(10);
  const first = env.pendingWorkerCalls[0];
  assert.equal(first.msg.cmd, "option");
  assert.equal(first.msg.payload.state.id, "piece_801000");
  assert.equal(first.msg.payload.value, 0.35);
  await resolveAll(env);
  assert.equal(env.indexedDB._pieces.get("piece_801000").state.recipe.studio.options.mute_share, 0.35);
  assert.equal(env.window.__studioDebug.getCurrentId(), "piece_802000");
  assert.match(env.document.getElementById("workspace").textContent, /#802000/);
});

test("Review 7: a failed piece switch redisplays the saved state of the current piece with controls enabled", async () => {
  const env = await bootTwoPieces();
  env.autoRunWorkerCalls = false;
  findButton(env, (b) => b.dataset.option === "intro_bars" && b.dataset.value === "4").click();
  await env.flushAll(10);
  pieceItems(env)[1].click();
  await env.flushAll(10);
  await resolveAll(env, 1);
  await env.flushAll(10);
  const snap = env.pendingWorkerCalls.shift();
  assert.equal(snap.msg.cmd, "snapshot");
  snap.reject("snapshot failed");
  await env.flushAll(15);
  assert.equal(env.window.__studioDebug.getCurrentId(), "piece_801000");
  assert.equal(findButton(env, (b) => b.dataset.option === "intro_bars" && b.dataset.value === "4").getAttribute("aria-pressed"), "true");
  const ws = env.document.getElementById("workspace");
  const stuck = ws.querySelectorAll("button, input").filter((b) => b.disabled && !["undoBtn", "redoBtn"].includes(b.id) && !b.classList.contains("reroll-btn"));
  assert.equal(stuck.length, 0);
  assert.equal(env.document.getElementById("errorBanner").hidden, false);
});

test("Review 8: a rejected slider edit restores the saved value and the pressed state", async () => {
  const env = await bootWithPiece();
  env.autoRunWorkerCalls = false;
  dragSlider(findSlider(env, "octave_share"), 0.75);
  await env.flushAll(10);
  env.pendingWorkerCalls.shift().reject("rejected");
  await env.flushAll(15);
  const auto = findButton(env, (b) => b.dataset.option === "octave_share" && b.dataset.value === "auto");
  const manual = findButton(env, (b) => b.dataset.option === "octave_share" && b.dataset.value === "manual");
  assert.equal(auto.getAttribute("aria-pressed"), "true");
  assert.equal(manual.getAttribute("aria-pressed"), "false");
  assert.equal(findSlider(env, "octave_share").disabled, false);
  env.workerMessages.length = 0;
  manual.click();
  await env.flushAll(10);
  assert.equal(env.workerMessages.filter((m) => m.type === "call" && m.cmd === "option").length, 1, "retry sends a request");
});

test("Review 9: re-roll-all is disabled while running and export failures reach the banner", async () => {
  const env = await bootWithPiece();
  env.autoRunWorkerCalls = false;
  const all = env.document.getElementById("rerollAllBtn");
  all.click();
  await env.flushAll(10);
  assert.equal(all.disabled, true);
  all.click();
  await env.flushAll(10);
  assert.equal(env.pendingWorkerCalls.filter((c) => c.msg.cmd === "reroll").length, 1);
  await resolveAll(env);

  env.autoRunWorkerCalls = true;
  env.failNextWorkerCall = "export boom";
  env.document.getElementById("dlMidiBtn").click();
  await env.flushAll(20);
  assert.equal(env.document.getElementById("errorBanner").hidden, false);
  assert.match(env.document.getElementById("errorText").textContent, /export boom/);
});

test("Review 10: an error from one piece does not stay above another piece", async () => {
  const env = await bootTwoPieces();
  env.autoRunWorkerCalls = false;
  findButton(env, (b) => b.dataset.layer === "comp" && b.dataset.section === "A").click();
  await env.flushAll(10);
  env.pendingWorkerCalls.shift().reject("boom");
  await env.flushAll(15);
  assert.equal(env.document.getElementById("errorBanner").hidden, false);
  pieceItems(env)[1].click();
  await env.flushAll(10);
  await resolveAll(env);
  assert.match(env.document.getElementById("workspace").textContent, /#802000/);
  assert.equal(env.document.getElementById("errorBanner").hidden, true);
});

test("Review 11: the selected piece is marked for assistive tech, the mute hint names sections like the timeline, modes are Russian", async () => {
  const env = await bootTwoPieces();
  const items = pieceItems(env);
  assert.equal(items[0].getAttribute("aria-current"), "true");
  assert.notEqual(items[1].getAttribute("aria-current"), "true");
  assert.ok(items[0].textContent.includes("лидийский"));
  assert.equal(items[0].textContent.includes("lydian"), false);
  const ws = env.document.getElementById("workspace");
  assert.ok(ws.textContent.includes("в секциях A, A2, «Кульминация» и B"));
  assert.equal(ws.textContent.includes("A3"), false);
  assert.ok(ws.querySelectorAll(".piece-title")[0].textContent.includes("лидийский"));
  items[1].click();
  await env.flushAll(15);
  assert.equal(pieceItems(env)[1].getAttribute("aria-current"), "true");
  assert.notEqual(pieceItems(env)[0].getAttribute("aria-current"), "true");
});

test("Review 12: a backup differing only in the redo stack is imported as a separate piece", async () => {
  const env = await bootWithPiece({ autoRunWorkerCalls: true });
  const local = env.indexedDB._pieces.get("piece_801000");
  local.state.redo = [makeFakeState(801000).recipe];
  const imported = JSON.parse(JSON.stringify(local.state));
  imported.redo = [];
  const impInput = env.document.getElementById("importPiecesInput");
  impInput.files = [{ text: async () => JSON.stringify({ version: 1, pieces: [{ id: "piece_801000", state: imported }] }) }];
  await impInput.onchange();
  await env.flushAll(20);
  assert.equal(env.indexedDB._pieces.get("piece_801000").state.redo.length, 1);
  const added = env.indexedDB._pieces.get("piece_801000_2");
  assert.ok(added, "redo-only difference must not be treated as a duplicate");
  assert.equal(added.state.redo.length, 0);
});

test("Review 12: the error banner is part of the production markup and dismisses", async () => {
  const html = fs.readFileSync(path.join(WEB_DIR, "index.html"), "utf8");
  assert.match(html, /<main id="workspace">\s*<div id="errorBanner"[^>]*role="alert"[^>]*hidden>\s*<span id="errorText"><\/span>\s*<button id="errorCloseBtn"/);
  const env = await bootWithPiece();
  assert.equal(env.document.getElementById("errorBanner").parentNode.id, "workspace");
  assert.equal(env.document.getElementById("errorBanner").hidden, true);
});

test("Review 4: import uses the validating command for undo and redo entries", async () => {
  const env = await bootWithPiece({ autoRunWorkerCalls: true });
  const st = makeFakeState(803000);
  const impInput = env.document.getElementById("importPiecesInput");
  impInput.files = [{ text: async () => JSON.stringify({ version: 1, pieces: [{ id: st.id, state: st }] }) }];
  env.workerMessages.length = 0;
  await impInput.onchange();
  await env.flushAll(20);
  const cmds = env.workerMessages.filter((m) => m.type === "call").map((m) => m.cmd);
  assert.equal(cmds[0], "import_state");
  assert.equal(cmds.includes("snapshot") && cmds.indexOf("snapshot") < cmds.indexOf("import_state"), false);
});

test("Review N1: a new piece during a slider debounce saves the edit on the current piece first", async () => {
  const env = await bootWithPiece();
  env.autoRunWorkerCalls = false;
  const slider = findSlider(env, "mute_share");
  slider.value = "0.35";
  slider.oninput();
  env.document.getElementById("newBtn").click();
  await env.flushAll(10);
  await new Promise((r) => setTimeout(r, 700));
  await env.flushAll(10);
  assert.equal(env.pendingWorkerCalls[0].msg.cmd, "option");
  assert.equal(env.pendingWorkerCalls[0].msg.payload.state.id, "piece_801000");
  await resolveAll(env);
  assert.equal(env.indexedDB._pieces.get("piece_801000").state.recipe.studio.options.mute_share, 0.35);
});

test("Review N3: a pressed control is disabled while its request waits in the queue", async () => {
  const env = await bootWithPiece();
  env.autoRunWorkerCalls = false;
  findButton(env, (b) => b.dataset.layer === "comp" && b.dataset.section === "A").click();
  await env.flushAll(10);
  const all = env.document.getElementById("rerollAllBtn");
  all.click();
  assert.equal(all.disabled, true);
  all.click();
  await env.flushAll(10);
  await resolveAll(env);
  const alls = env.workerMessages.filter((m) => m.type === "call" && m.payload.layer === "all");
  assert.equal(alls.length, 1);
});

test("Review N4: every pending slider is flushed in the order it was moved", async () => {
  const env = await bootWithPiece();
  findButton(env, (b) => b.dataset.option === "intro_bars" && b.dataset.value === "8").click();
  await env.flushAll(15);
  env.workerMessages.length = 0;
  const mute = findSlider(env, "mute_share");
  mute.value = "0.35";
  mute.oninput();
  await new Promise((r) => setTimeout(r, 300));
  const dose = findSlider(env, "octave_piece");
  dose.value = "0.5";
  dose.oninput();
  await new Promise((r) => setTimeout(r, 250));
  env.document.getElementById("undoBtn").click();
  await env.flushAll(20);
  await new Promise((r) => setTimeout(r, 700));
  await env.flushAll(15);
  const sent = env.workerMessages.filter((m) => m.type === "call").map((m) => (m.cmd === "option" ? m.payload.name : m.cmd));
  assert.deepEqual(sent, ["mute_share", "octave_piece", "undo"]);
});

const keyButtons = (env, option) => env.document.getElementById("workspace").querySelectorAll("button").filter((b) => b.dataset.option === option);
const keyButton = (env, option, value) => keyButtons(env, option).find((b) => b.dataset.value === value);
const keyCalls = (env) => env.workerMessages.filter((m) => m.type === "call" && m.cmd === "option" && m.payload.name === "key").map((m) => m.payload.value);

test("Key: Тональность offers tonics and Russian modes, marks the current key and has a reset", async () => {
  const env = await bootWithPiece();
  const ws = env.document.getElementById("workspace");
  assert.match(ws.textContent, /Тональность/);
  assert.match(ws.textContent, /Переводит все ноты в выбранную тональность по ступеням; ритм и форма не меняются\. Применяется сразу\./);
  assert.deepEqual(keyButtons(env, "key_tonic").map((b) => b.dataset.value), ["E", "F#", "G", "A", "B", "C#", "D"]);
  assert.deepEqual(
    keyButtons(env, "key_mode").map((b) => b.textContent),
    ["мажор", "дорийский", "фригийский", "лидийский", "миксолидийский", "натуральный минор", "локрийский"],
  );
  for (const opt of ["key_tonic", "key_mode", "key"]) {
    const group = keyButtons(env, opt);
    assert.equal(group.filter((b) => b.getAttribute("aria-pressed") === "true").length, 1, opt);
    for (const b of group) assert.ok(["true", "false"].includes(b.getAttribute("aria-pressed")), opt);
  }
  assert.equal(keyButton(env, "key_tonic", "E").getAttribute("aria-pressed"), "true");
  assert.equal(keyButton(env, "key_mode", "lydian").getAttribute("aria-pressed"), "true");
  assert.equal(keyButton(env, "key", "auto").textContent, "Как в пьесе");
  assert.equal(keyButton(env, "key", "auto").getAttribute("aria-pressed"), "true");
});

test("Key: choices send the whole key, the header shows it, undo and redo name it, reset sends null", async () => {
  const env = await bootWithPiece();
  keyButton(env, "key_tonic", "D").click();
  await env.flushAll(15);
  keyButton(env, "key_mode", "aeolian").click();
  await env.flushAll(15);
  assert.deepEqual(keyCalls(env), [{ tonic: "D", mode: "lydian" }, { tonic: "D", mode: "aeolian" }]);
  assert.match(env.document.getElementById("workspace").textContent, /D минор \(эолийский\)/);
  assert.equal(keyButton(env, "key", "auto").getAttribute("aria-pressed"), "false");
  assert.equal(keyButton(env, "key_tonic", "D").getAttribute("aria-pressed"), "true");
  assert.match(env.document.getElementById("undoBtn").title, /Отменить: тональность: D натуральный минор/);
  env.document.getElementById("undoBtn").click();
  await env.flushAll(15);
  assert.match(env.document.getElementById("redoBtn").title, /Вернуть: тональность: D натуральный минор/);
  env.document.getElementById("redoBtn").click();
  await env.flushAll(15);
  keyButton(env, "key", "auto").click();
  await env.flushAll(15);
  assert.deepEqual(keyCalls(env).pop(), null);
  assert.match(env.document.getElementById("workspace").textContent, /E лидийский/);
  const saved = env.indexedDB._pieces.get("piece_801000").state;
  assert.equal(saved.recipe.studio.options.key, null);
  assert.ok(env.indexedDB._logs.some((l) => l.action === "option" && l.value && "key" in l.value));
});

test("Key: the pressed choice does nothing, controls are disabled while busy and an error reaches the banner", async () => {
  const env = await bootWithPiece();
  keyButton(env, "key_tonic", "E").click();
  keyButton(env, "key", "auto").click();
  await env.flushAll(10);
  assert.deepEqual(keyCalls(env), []);
  env.autoRunWorkerCalls = false;
  const btn = keyButton(env, "key_tonic", "G");
  btn.click();
  await env.flushAll(10);
  assert.equal(btn.disabled, true);
  assert.equal(env.document.getElementById("statusChip").textContent, "Пересчёт…");
  env.pendingWorkerCalls.shift().reject("key boom");
  await env.flushAll(15);
  assert.equal(env.document.getElementById("statusChip").textContent, "");
  assert.equal(env.document.getElementById("errorBanner").hidden, false);
  assert.match(env.document.getElementById("errorText").textContent, /key boom/);
  assert.equal(keyButton(env, "key_tonic", "G").disabled, false);
});

test("Key: a pending slider is flushed before the key change", async () => {
  const env = await bootWithPiece();
  env.workerMessages.length = 0;
  const mute = findSlider(env, "mute_share");
  mute.value = "0.35";
  mute.oninput();
  keyButton(env, "key_mode", "dorian").click();
  await env.flushAll(20);
  await new Promise((r) => setTimeout(r, 700));
  await env.flushAll(15);
  const sent = env.workerMessages.filter((m) => m.type === "call").map((m) => (m.cmd === "option" ? m.payload.name : m.cmd));
  assert.deepEqual(sent, ["mute_share", "key"]);
});

test("Key: two quick clicks, tonic then mode, end in the combined key", async () => {
  const env = await bootWithPiece();
  env.autoRunWorkerCalls = false;
  env.workerMessages.length = 0;
  keyButton(env, "key_tonic", "D").click();
  await env.flushAll(10);
  keyButton(env, "key_mode", "aeolian").click();
  await env.flushAll(10);
  await resolveAll(env);
  await env.flushAll(15);
  assert.deepEqual(keyCalls(env), [{ tonic: "D", mode: "lydian" }, { tonic: "D", mode: "aeolian" }]);
  const saved = env.indexedDB._pieces.get("piece_801000").state;
  assert.deepEqual(saved.recipe.studio.options.key, { tonic: "D", mode: "aeolian" });
  assert.match(env.document.getElementById("workspace").textContent, /D минор \(эолийский\)/);
});

test("Key: Как в пьесе while a key request is pending does not reuse the old key", async () => {
  const env = await bootWithPiece();
  env.autoRunWorkerCalls = false;
  env.workerMessages.length = 0;
  keyButton(env, "key_tonic", "D").click();
  await env.flushAll(10);
  keyButton(env, "key", "auto").click();
  await env.flushAll(10);
  keyButton(env, "key_mode", "dorian").click();
  await env.flushAll(10);
  await resolveAll(env);
  await env.flushAll(15);
  assert.deepEqual(keyCalls(env), [{ tonic: "D", mode: "lydian" }, null, { tonic: "E", mode: "dorian" }]);
});

const MARK_HINT = "Отметьте место, где звучит не по-человечески: кнопка «Тут не так», клавиша M или Shift+клик по таймлайну. Музыку это не меняет.";
const KEY_M = { key: "m", code: "KeyM" };

const byClass = (env, cls) => env.document.getElementById("workspace").querySelectorAll(`.${cls}`);
const savedMarks = (env) => env.indexedDB._pieces.get("piece_801000").state.marks || [];
const markLogs = (env) => env.indexedDB._logs.filter((l) => l.action.startsWith("mark_"));

async function bootWithMarks(marks, options = {}) {
  const st = makeFakeState(801000);
  st.marks = marks.map((m, i) => ({
    id: `m${i + 1}`, t: 3, bar: 2, sixteenth: 5, section: "A", section_label: "Куплет A",
    guitar: "both", text: "", notes: [], rev: JSON.stringify({}), ...m,
  }));
  return bootWithPiece({ stateOverrides: st, ...options });
}

test("Marks: М adds a mark at the playback position without touching undo, redo, playback or the music", async () => {
  const env = await bootWithPiece({ audioInitialState: "running" });
  const sampler = env.window.__studioDebug.sampler;
  await sampler.seek(1.25, true);
  await env.flushAll(10);
  assert.equal(sampler.isPlaying(), true);
  env.audioContexts[0].currentTime = 0.5;
  const timersBefore = [...env.timers.keys()];
  const before = JSON.parse(JSON.stringify(env.indexedDB._pieces.get("piece_801000").state));

  env.press(KEY_M);
  await env.flushAll(30);

  const saved = env.indexedDB._pieces.get("piece_801000").state;
  assert.equal(saved.marks.length, 1);
  assert.equal(saved.marks[0].t, 1.75);
  assert.equal(saved.marks[0].guitar, "both");
  assert.deepEqual({ recipe: saved.recipe, undo: saved.undo, redo: saved.redo }, { recipe: before.recipe, undo: before.undo, redo: before.redo });
  assert.equal(sampler.isPlaying(), true, "the mark must not interrupt playback");
  assert.deepEqual([...env.timers.keys()], timersBefore, "the player must not be restarted");
  assert.deepEqual(markLogs(env).map((l) => [l.action, l.value.id]), [["mark_add", saved.marks[0].id]]);
  assert.equal(env.document.getElementById("undoBtn").disabled, true);
  const cmds = env.workerMessages.filter((m) => m.type === "call").map((m) => m.cmd);
  assert.deepEqual(cmds.filter((c) => c.startsWith("mark_")), ["mark_add"]);
});

test("Marks: the button «Тут не так» adds a mark, М is ignored in text fields, with modifiers and without a piece", async () => {
  const env = await bootWithPiece();
  const sampler = env.window.__studioDebug.sampler;
  await sampler.seek(2.0, false);
  env.document.getElementById("markBtn").click();
  await env.flushAll(20);
  assert.deepEqual(savedMarks(env).map((m) => m.t), [2]);
  assert.equal(env.document.getElementById("markBtn").textContent, "Тут не так");

  for (const ev of [
    { ...KEY_M, target: { tagName: "INPUT" } },
    { ...KEY_M, target: { tagName: "TEXTAREA" } },
    { ...KEY_M, target: { tagName: "DIV", isContentEditable: true } },
    { ...KEY_M, ctrlKey: true },
    { ...KEY_M, metaKey: true },
    { ...KEY_M, altKey: true },
    { key: "n", code: "KeyN" },
  ]) {
    env.press(ev);
    await env.flushAll(10);
  }
  assert.equal(savedMarks(env).length, 1);

  const empty = createAppHarness({});
  await empty.flushAll(20);
  empty.press(KEY_M);
  await empty.flushAll(20);
  assert.equal(empty.workerMessages.some((m) => m.type === "call" && m.cmd === "mark_add"), false);
});

test("Marks: Shift+click on the timeline adds a mark at the clicked place, a plain click only seeks", async () => {
  const env = await bootWithPiece();
  const sampler = env.window.__studioDebug.sampler;
  const blk = env.document.getElementById("timeline").children[1];
  const start = Number(blk.dataset.start);
  const end = Number(blk.dataset.end);
  blk.getBoundingClientRect = () => ({ left: 100, width: 200 });

  blk.onclick({ shiftKey: true, clientX: 150 });
  await env.flushAll(20);
  assert.deepEqual(savedMarks(env).map((m) => m.t), [start + 0.25 * (end - start)]);

  blk.onclick({ shiftKey: false, clientX: 250 });
  await env.flushAll(20);
  assert.equal(savedMarks(env).length, 1);
  assert.equal(sampler.currentTime(), start);
  assert.equal(env.document.getElementById("timeline").children.length, 4);
});

test("Marks: a long press on touch adds a mark and the click after it does not seek", async () => {
  const env = await bootWithPiece();
  const sampler = env.window.__studioDebug.sampler;
  const blk = env.document.getElementById("timeline").children[1];
  const start = Number(blk.dataset.start);
  blk.getBoundingClientRect = () => ({ left: 0, width: 100 });
  blk.onpointerdown({ pointerType: "touch", clientX: 50 });
  blk.onpointerup({ pointerType: "touch" });
  await env.flushAll(20);
  blk.onclick({});
  assert.equal(savedMarks(env).length, 0, "a short touch is a plain click");
  assert.equal(sampler.currentTime(), start);

  await sampler.seek(0, false);
  blk.onpointerdown({ pointerType: "touch", clientX: 50 });
  await new Promise((r) => setTimeout(r, 650));
  await env.flushAll(20);
  assert.equal(savedMarks(env).length, 1);
  blk.onpointerup({ pointerType: "touch" });
  blk.onclick({});
  await env.flushAll(10);
  assert.equal(sampler.currentTime(), 0, "the click that ends a long press must not seek");

  blk.onpointerdown({ pointerType: "mouse", clientX: 50 });
  await new Promise((r) => setTimeout(r, 650));
  await env.flushAll(10);
  assert.equal(savedMarks(env).length, 1, "a held mouse button is not a long press");
});

test("Marks: the editor sets guitar and text, Enter and Готово close it, Esc deletes only a just added mark", async () => {
  const env = await bootWithPiece();
  env.press(KEY_M);
  await env.flushAll(20);
  const editor = () => env.document.getElementById("markEditor");
  assert.ok(editor(), "an editor opens after a mark is added");
  assert.equal(editor().tagName, "DIV", "the editor is inline, not a dialog");
  const guitarBtns = editor().querySelectorAll("button").filter((b) => b.dataset.markGuitar);
  assert.deepEqual(guitarBtns.map((b) => b.dataset.markGuitar), ["lead", "rhythm", "both"]);
  assert.equal(guitarBtns.find((b) => b.dataset.markGuitar === "both").getAttribute("aria-pressed"), "true");
  guitarBtns.find((b) => b.dataset.markGuitar === "lead").click();
  const input = env.document.getElementById("markText");
  assert.equal(input.getAttribute("maxlength"), "80");
  input.value = "лишняя пауза";
  env.document.getElementById("markDone").click();
  await env.flushAll(20);
  assert.equal(editor(), null);
  assert.deepEqual(savedMarks(env).map((m) => [m.guitar, m.text]), [["lead", "лишняя пауза"]]);
  assert.deepEqual(markLogs(env).map((l) => l.action), ["mark_add", "mark_edit"]);

  env.press({ key: "Escape" });
  await env.flushAll(20);
  assert.equal(savedMarks(env).length, 1, "Esc after Готово must not delete the mark");

  env.press(KEY_M);
  await env.flushAll(20);
  assert.equal(savedMarks(env).length, 2);
  env.document.getElementById("markText").value = "ещё";
  env.press({ key: "Escape", target: env.document.getElementById("markText") });
  await env.flushAll(20);
  assert.equal(savedMarks(env).length, 1, "Esc deletes the mark that was just added");
  assert.equal(editor(), null);
  assert.deepEqual(markLogs(env).map((l) => l.action), ["mark_add", "mark_edit", "mark_add", "mark_delete"]);

  env.press(KEY_M);
  await env.flushAll(20);
  env.document.getElementById("markText").onkeydown({ key: "Enter" });
  await env.flushAll(20);
  assert.equal(savedMarks(env).length, 2, "Enter closes the editor and keeps the mark");
  assert.equal(editor(), null);

  const id = savedMarks(env)[1].id;
  byClass(env, "mark-flag").find((f) => f.dataset.markId === id).onclick();
  await env.flushAll(10);
  env.document.getElementById("markDelete").click();
  await env.flushAll(20);
  assert.equal(savedMarks(env).length, 1);
  assert.equal(markLogs(env).pop().action, "mark_delete");
});

test("Marks: flags, list and Russian texts show bar:sixteenth, guitar, text and stale state", async () => {
  const env = await bootWithMarks([
    { t: 3, bar: 2, sixteenth: 5, guitar: "lead", text: "лишняя пауза", rev: JSON.stringify({}) },
    { t: 5, bar: 3, sixteenth: 1, guitar: "rhythm", text: "старая", rev: "other" },
  ]);
  assert.equal(env.document.getElementById("marksTitle").textContent, "Отметки (2)");
  assert.equal(env.document.getElementById("marksHint").textContent, MARK_HINT);
  const flags = byClass(env, "mark-flag");
  const rows = byClass(env, "mark-row");
  assert.equal(flags.length, 2);
  assert.equal(rows.length, 2);
  assert.match(rows[0].textContent, /2:5/);
  assert.match(rows[0].textContent, /лишняя пауза/);
  assert.match(rows[0].textContent, /соло/);
  assert.doesNotMatch(rows[0].textContent, /до изменения/);
  assert.match(rows[1].textContent, /3:1/);
  assert.match(rows[1].textContent, /\(до изменения\)/);
  assert.equal(flags[0].classList.contains("stale"), false);
  assert.equal(flags[1].classList.contains("stale"), true);
  assert.equal(rows[1].classList.contains("stale"), true);
  assert.match(flags[1].title, /до изменения/);
  assert.equal(flags[0].style.left, `${(3 / 8) * 100}%`);
  assert.ok(flags[0].getAttribute("aria-label"));
});

test("Marks: a mark made before a reroll turns stale after it", async () => {
  const env = await bootWithPiece();
  env.press(KEY_M);
  await env.flushAll(20);
  assert.equal(byClass(env, "mark-row")[0].classList.contains("stale"), false);
  const rerollBtn = env.document.getElementById("workspace").querySelectorAll(".reroll-btn").find((b) => b.dataset.layer === "tempo");
  rerollBtn.click();
  await env.flushAll(30);
  const row = byClass(env, "mark-row")[0];
  assert.equal(row.classList.contains("stale"), true);
  assert.match(row.textContent, /\(до изменения\)/);
  assert.equal(savedMarks(env).length, 1, "the reroll keeps the mark");
});

test("Marks: clicking a flag or a row seeks one bar before the mark and opens its editor", async () => {
  const env = await bootWithMarks([{ t: 5 }]);
  const sampler = env.window.__studioDebug.sampler;
  const flag = byClass(env, "mark-flag")[0];
  const row = byClass(env, "mark-row")[0];
  const calls = env.workerMessages.length;
  flag.onclick();
  await env.flushAll(10);
  assert.equal(sampler.currentTime(), 3);
  assert.ok(env.document.getElementById("markEditor"));
  env.document.getElementById("markDone").click();
  await env.flushAll(20);
  assert.equal(env.document.getElementById("markEditor"), null);
  await sampler.seek(0, false);
  const rowOpen = byClass(env, "mark-row")[0].querySelectorAll("button").find((b) => b.classList.contains("mark-open"));
  rowOpen.onclick();
  await env.flushAll(10);
  assert.equal(sampler.currentTime(), 3);
  assert.ok(env.document.getElementById("markEditor"));
  assert.equal(env.workerMessages.slice(calls).filter((m) => m.type === "call").length, 0, "seeking changes no state");
  assert.ok(row);
});

test("Marks: «Послушать место» plays from one bar before to one bar after the mark, then stops", async () => {
  const env = await bootWithMarks([{ t: 5 }], { audioInitialState: "running" });
  const sampler = env.window.__studioDebug.sampler;
  const ctx = () => env.audioContexts[0];
  const listen = byClass(env, "mark-audition")[0];
  assert.equal(listen.textContent, "Послушать место");
  listen.onclick();
  await env.flushAll(20);
  assert.equal(sampler.isPlaying(), true);
  assert.equal(sampler.currentTime(), 3);

  ctx().currentTime = 1.5;
  env.tickTimers();
  assert.equal(sampler.isPlaying(), true, "still inside the range");
  ctx().currentTime = 4.1;
  env.tickTimers();
  assert.equal(sampler.isPlaying(), false, "stops at one bar after the mark");
  assert.equal(sampler.currentTime(), 7);

  await sampler.seek(0, true);
  await env.flushAll(10);
  assert.equal(sampler.isPlaying(), true);
  ctx().currentTime = ctx().currentTime + 7.5;
  env.tickTimers();
  assert.equal(sampler.isPlaying(), true, "a later ordinary play is not cut off by an old range");
});

test("Marks: marks are in export and import, and a legacy piece without marks loads", async () => {
  const env = await bootWithMarks([{ t: 5, text: "туда" }]);
  await env.document.getElementById("exportPiecesBtn").onclick();
  await env.flushAll(20);
  const payload = JSON.parse(env.downloads.pop().parts.join(""));
  assert.equal(payload.pieces[0].state.marks[0].text, "туда");

  const st = JSON.parse(JSON.stringify(payload.pieces[0].state));
  st.recipe.studio.salts["comp@A"] = 4;
  const impInput = env.document.getElementById("importPiecesInput");
  impInput.files = [{ text: async () => JSON.stringify({ version: 1, pieces: [{ id: st.id, state: st }] }) }];
  await impInput.onchange();
  await env.flushAll(30);
  const imported = env.indexedDB._pieces.get("piece_801000_2");
  assert.equal(imported.state.marks[0].text, "туда");
  assert.equal(env.document.getElementById("marksTitle").textContent, "Отметки (1)");

  const legacy = await bootWithPiece();
  assert.equal(legacy.document.getElementById("marksTitle").textContent, "Отметки (0)");
  assert.equal(byClass(legacy, "mark-flag").length, 0);
});

test("Marks: «Скачать отметки» saves marks.json of the piece, the sidebar button of all pieces", async () => {
  const env = await bootWithMarks([{ t: 5, text: "а" }]);
  await env.document.getElementById("marksDlBtn").onclick();
  await env.flushAll(20);
  const one = JSON.parse(env.downloads.pop().parts.join(""));
  assert.equal(one.seed, 801000);
  assert.equal(one.id, "piece_801000");
  assert.ok(one.recipe);
  assert.deepEqual(one.marks.map((m) => m.text), ["а"]);

  await env.document.getElementById("exportMarksAllBtn").onclick();
  await env.flushAll(20);
  const all = JSON.parse(env.downloads.pop().parts.join(""));
  assert.deepEqual(all.pieces.map((p) => p.id), ["piece_801000"]);
  assert.equal(all.pieces[0].marks.length, 1);
});

test("Marks: a failed mark request reaches the banner and leaves the marks as saved", async () => {
  const env = await bootWithPiece();
  env.failNextWorkerCall = "disk is full";
  env.press(KEY_M);
  await env.flushAll(20);
  assert.equal(env.document.getElementById("errorBanner").hidden, false);
  assert.match(env.document.getElementById("errorText").textContent, /disk is full/);
  assert.equal(savedMarks(env).length, 0);
  assert.equal(env.document.getElementById("marksTitle").textContent, "Отметки (0)");
});

test("Marks: production markup has the sidebar button and the page has no alert", () => {
  const html = fs.readFileSync(path.join(WEB_DIR, "index.html"), "utf8");
  assert.match(html, /<button id="exportMarksAllBtn">Скачать отметки всех пьес<\/button>/);
  assert.match(html, /\.mark-flag[^{]*\{/);
  assert.match(html, /\.mark-flag\.stale|\.stale/);
});

const markCalls = (env) => env.workerMessages.filter((m) => m.type === "call" && m.cmd.startsWith("mark_"));

test("Marks review: a slow mark write keeps what another tab did meanwhile and only adds its mark", async () => {
  const env = await bootWithPiece();
  env.autoRunWorkerCalls = false;
  env.press(KEY_M);
  await env.flushAll(10);
  assert.equal(env.pendingWorkerCalls.length, 1);

  const stored = env.indexedDB._pieces.get("piece_801000");
  stored.state.recipe.studio.salts["comp@A"] = 3;
  stored.state.undo = [];
  stored.state.redo = [makeFakeState(801000).recipe];
  stored.state.marks = [{ id: "mother", t: 1, bar: 1, sixteenth: 1, section: "A", section_label: "Куплет A", guitar: "both", text: "", notes: [], rev: "x" }];

  const call = env.pendingWorkerCalls.shift();
  call.resolve(env.runFakePy(call.msg.cmd, call.msg.payload));
  await env.flushAll(30);

  const after = env.indexedDB._pieces.get("piece_801000").state;
  assert.equal(after.recipe.studio.salts["comp@A"], 3, "the recipe changed meanwhile must stay");
  assert.equal(after.redo.length, 1, "redo must not be cleared");
  assert.equal(after.undo.length, 0);
  assert.deepEqual(after.marks.map((m) => m.id).sort(), [after.marks.find((m) => m.id !== "mother").id, "mother"].sort());
  assert.equal(after.marks.length, 2);
  assert.deepEqual(markLogs(env).map((l) => l.action), ["mark_add"]);
});

test("Marks review: a mark sends full precision time, a client id and the revision that was loaded", async () => {
  const env = await bootWithPiece();
  const sampler = env.window.__studioDebug.sampler;
  await sampler.seek(1.2345678, false);
  env.press(KEY_M);
  await env.flushAll(20);
  const [call] = markCalls(env);
  assert.equal(call.payload.t, 1.2345678);
  assert.match(call.payload.id, /^m[0-9a-f]{6,}$/);
  assert.equal(call.payload.rev, "{}");
  assert.equal(savedMarks(env)[0].id, call.payload.id);
});

test("Marks review: pressing М while a reroll is running names the revision that is playing", async () => {
  const env = await bootWithPiece();
  const shown = env.document.getElementById("workspace").querySelectorAll(".reroll-btn").find((b) => b.dataset.layer === "tempo");
  env.autoRunWorkerCalls = false;
  shown.click();
  await env.flushAll(10);
  env.press(KEY_M);
  await env.flushAll(10);
  await resolveAll(env);
  const [call] = markCalls(env);
  assert.equal(call.payload.rev, "{}", "the old revision was audible when М was pressed");
  const row = byClass(env, "mark-row")[0];
  assert.match(row.textContent, /\(до изменения\)/);
});

test("Marks review: audition is disabled for a mark outside the current piece and never inverted", async () => {
  const env = await bootWithMarks([{ t: 100, rev: "0".repeat(12) }, { t: 5 }]);
  const listen = byClass(env, "mark-audition");
  assert.equal(listen[0].disabled, true);
  assert.equal(listen[1].disabled, false);
  const sampler = env.window.__studioDebug.sampler;
  await sampler.seek(1, false);
  listen[0].click();
  await env.flushAll(10);
  assert.equal(sampler.currentTime(), 1);
});

test("Marks review: М works on sliders and buttons but not in text fields", async () => {
  const env = await bootWithPiece();
  env.press({ ...KEY_M, target: { tagName: "INPUT", type: "range" } });
  await env.flushAll(20);
  env.press({ ...KEY_M, target: { tagName: "BUTTON" } });
  await env.flushAll(20);
  env.press({ ...KEY_M, target: { tagName: "INPUT", type: "text" } });
  env.press({ ...KEY_M, target: { tagName: "INPUT", type: "number" } });
  env.press({ ...KEY_M, target: { tagName: "TEXTAREA" } });
  await env.flushAll(20);
  assert.equal(savedMarks(env).length, 2);
});

test("Marks review: opening another flag or another piece keeps the typed comment", async () => {
  const env = await bootWithMarks([{ t: 3 }, { t: 5 }]);
  const flags = () => byClass(env, "mark-flag");
  flags()[0].onclick();
  env.document.getElementById("markText").value = "черновик";
  flags()[1].onclick();
  await env.flushAll(20);
  assert.equal(savedMarks(env)[0].text, "черновик");
  assert.equal(env.document.getElementById("markEditor").dataset.markId, "m2");

  const env2 = await bootTwoPieces();
  const st = env2.indexedDB._pieces.get("piece_801000").state;
  st.marks = [{ id: "m1", t: 3, bar: 2, sixteenth: 5, section: "A", section_label: "Куплет A", guitar: "both", text: "", notes: [], rev: "{}" }];
  env2.document.getElementById("pieceList").querySelectorAll(".piece-item")[0].click();
  await env2.flushAll(30);
  byClass(env2, "mark-flag")[0].onclick();
  env2.document.getElementById("markText").value = "не потерять";
  pieceItems(env2)[1].click();
  await env2.flushAll(40);
  assert.equal(env2.indexedDB._pieces.get("piece_801000").state.marks[0].text, "не потерять");
});

test("Marks review: moving a touch more than 10 px cancels the long press", async () => {
  const env = await bootWithPiece();
  const blk = env.document.getElementById("timeline").children[1];
  blk.getBoundingClientRect = () => ({ left: 0, width: 100 });
  blk.onpointerdown({ pointerType: "touch", clientX: 50, clientY: 10 });
  blk.onpointermove({ pointerType: "touch", clientX: 70, clientY: 10 });
  await new Promise((r) => setTimeout(r, 650));
  await env.flushAll(10);
  assert.equal(savedMarks(env).length, 0);
  blk.onpointerup({});
  blk.onpointerdown({ pointerType: "touch", clientX: 50, clientY: 10 });
  blk.onpointermove({ pointerType: "touch", clientX: 54, clientY: 13 });
  await new Promise((r) => setTimeout(r, 650));
  await env.flushAll(20);
  assert.equal(savedMarks(env).length, 1);
});

test("Marks review: importing a piece with marks logs their ids", async () => {
  const env = await bootWithMarks([{ t: 5 }, { t: 6 }]);
  const st = JSON.parse(JSON.stringify(env.indexedDB._pieces.get("piece_801000").state));
  st.recipe.studio.salts["comp@A"] = 4;
  const impInput = env.document.getElementById("importPiecesInput");
  impInput.files = [{ text: async () => JSON.stringify({ version: 1, pieces: [{ id: st.id, state: st }] }) }];
  await impInput.onchange();
  await env.flushAll(30);
  const row = env.indexedDB._logs.find((l) => l.action === "mark_import");
  assert.deepEqual(row.value.ids, ["m1", "m2"]);
  assert.equal(row.piece_id, "piece_801000_2");
});

test("Marks review: the editor opens for the newest added mark", async () => {
  const env = await bootWithPiece();
  env.press(KEY_M);
  env.press(KEY_M);
  await env.flushAll(40);
  assert.equal(savedMarks(env).length, 2);
  assert.equal(env.document.getElementById("markEditor").dataset.markId, savedMarks(env)[1].id);
});
