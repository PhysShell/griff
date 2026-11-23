import test from "node:test";
import assert from "node:assert/strict";
import { createAppHarness, makeFakeState, buildDetailFromState } from "./harness.js";

async function bootWithPiece(options = {}) {
  const st = makeFakeState(801000);
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

  const compButtons = env.document
    .getElementById("workspace")
    .querySelectorAll(".reroll-btn")
    .filter((b) => b.dataset.layer === "comp" && b.dataset.section === "A");
  assert.equal(compButtons.length, 1);

  // Trigger two rapid comp@A rerolls before any worker reply completes, plus an export
  compButtons[0].click();
  compButtons[0].click();
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
  assert.equal(saved.state.recipe.studio.salts["comp@A"], 2, "two rapid rerolls must reach salt 2");
  assert.equal(saved.state.undo.length, 2, "two rapid rerolls must keep 2 undo entries");
  const rerollLogs = env.indexedDB._logs.filter((l) => l.action === "reroll");
  assert.equal(rerollLogs.length, 2);
  assert.equal(exportedSalt, 2, "export must wait for both preceding edits");
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
