// Pure, DOM-free helpers keeping a chunk id consistent with the loaded source
// file (#77). The chunk id persists across file loads, so a stale id (e.g. a
// `speeddemon.mid` id left in place when `dgd.gpx` is loaded) would stamp the new
// song's chunks with the old name and silently collide in the corpus. These
// derive a sensible default id and detect a clear mismatch. Unit-tested under
// `node --test` (web/test/idguard.test.js); no DOM or wasm.

// Lowercase slug of a filename's stem: drop the extension, map runs of
// non-alphanumerics to '_', trim. A name with no ASCII-alphanumerics slugs to
// '' (then the guard simply has no opinion).
export function slugFromFilename(name) {
  const stem = String(name || '').replace(/\.[^.]+$/, '');
  return stem.toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/^_+|_+$/g, '');
}

// A sensible default chunk id for a freshly loaded file: its slug.
export function defaultChunkId(name) {
  return slugFromFilename(name);
}

// First character of each '_'-separated segment, e.g. dance_gavin_dance → dgd.
export function chunkIdInitials(slug) {
  return String(slug || '')
    .split('_')
    .filter(Boolean)
    .map((s) => s[0])
    .join('');
}

// The id's name part, dropping a trailing version number (_001 / _p3).
function idStem(id) {
  return String(id || '').trim().toLowerCase().replace(/_p?\d+$/, '');
}

// Whether `id` plausibly belongs to `filename`: an exact stem match, or one
// being the other's initials (so "dgd" ↔ "dance_gavin_dance" both pass). An
// empty id or an unrecognised file means "no opinion" (true) — the guard only
// flags a clear mismatch, never a blank or an abbreviation.
export function idMatchesFile(id, filename) {
  const slug = slugFromFilename(filename);
  const stem = idStem(id);
  if (!stem || !slug) return true;
  return stem === slug || chunkIdInitials(stem) === slug || stem === chunkIdInitials(slug);
}
