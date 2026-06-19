// Pure, DOM-free helpers keeping a chunk id consistent with the loaded source
// file (#77). The chunk id persists across file loads, so a stale id (e.g. a
// `speeddemon.mid` id left in place when `dgd.gpx` is loaded) would stamp the new
// song's chunks with the old name and silently collide in the corpus. These
// derive a sensible default id and detect a clear mismatch. Unit-tested under
// `node --test` (web/test/idguard.test.js); no DOM or wasm.

// Lowercase slug of a filename's stem: drop the extension, map runs of
// non-alphanumerics to '_', trim. A name with no ASCII-alphanumerics slugs to
// '' (then the guard simply has no opinion).
// Lowercase slug of an arbitrary string: runs of non-alphanumerics → '_', trimmed.
function slugify(s) {
  return String(s || '').toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/^_+|_+$/g, '');
}

export function slugFromFilename(name) {
  return slugify(String(name || '').replace(/\.[^.]+$/, ''));
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

// Whether `candidate` lines up with `slug` directly or via initials either way
// (so "dgd" ↔ "dance_gavin_dance" both pass).
function relatesTo(candidate, slug) {
  return candidate === slug
    || chunkIdInitials(candidate) === slug
    || candidate === chunkIdInitials(slug);
}

// Whether `id` plausibly belongs to `filename`. Compares the id as-is *and*
// without a trailing version number, so a versioned id (dgd_001 ↔ dgd.gpx) and a
// numerically-named file (song_2 ↔ "Song 2.mid") both pass; only a genuinely
// foreign id is flagged. An empty id or an unrecognised file means "no opinion".
export function idMatchesFile(id, filename) {
  const slug = slugFromFilename(filename);
  const full = slugify(id);
  if (!full || !slug) return true;
  const stem = full.replace(/_p?\d+$/, '');
  return relatesTo(full, slug) || relatesTo(stem, slug);
}
