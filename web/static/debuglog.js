// Pure, DOM-free debug-log ring buffer for the playground's on-page panel.
// Kept apart from app.js so its formatting/bounding logic is unit-testable under
// `node --test` (web/test/debuglog.test.js) without a browser or the wasm glue.
// The DOM rendering stays in app.js; this owns only the lines.
//
// `max`  — keep at most this many lines (oldest dropped) so the buffer is bounded.
// `now`  — clock injection point; tests pass a fixed Date for deterministic stamps.
export function createDebugLog({ max = 400, now = () => new Date() } = {}) {
  let lines = [];

  // One line: "HH:MM:SS  label {json}". `data` is optional and JSON-stringified;
  // a value that cannot be serialized is flagged rather than thrown, so logging
  // never breaks the action it is tracing.
  const format = (label, data) => {
    const t = now().toTimeString().slice(0, 8);
    let body = '';
    if (data !== undefined) {
      try { body = ' ' + (typeof data === 'string' ? data : JSON.stringify(data)); }
      catch (_) { body = ' [unserializable]'; }
    }
    return `${t}  ${label}${body}`;
  };

  const push = (label, data) => {
    lines.push(format(label, data));
    if (lines.length > max) lines = lines.slice(-max);
    return lines[lines.length - 1];
  };

  return {
    push,
    err: (label, data) => push('✗ ' + label, data),
    clear: () => { lines = []; },
    lines: () => lines.slice(),
    text: () => lines.join('\n'),
    get length() { return lines.length; },
  };
}
