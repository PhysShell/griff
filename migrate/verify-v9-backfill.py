#!/usr/bin/env python3
"""Read-only verifier for the v9 sha256 backfill (PR C evidence).

    verify-v9-backfill.py <before_corpus> <after_corpus> <tabs> [census_before.json census_after.json]

Reproduces and checks every load-bearing claim in
`docs/audit/2026-07-v9-corpus-backfill.md`, and binds them to the exact corpus
snapshot documented there (see the fingerprint constants below). Deterministic:
no RNG, no seed, no wall-clock; the same inputs always yield the same numbers and
the same exit code. It reads only; it writes nothing.

If the two optional census JSON paths are given, the verifier also asserts they
are byte-identical to the committed census artifacts — so the "byte-identical to
fresh output" claim is machine-checked, not eyeballed.

`<before_corpus>` is the untouched v8 copy, `<after_corpus>` is the migrate-v9
output, `<tabs>` is the tabs root passed to migrate-v9 (identities are taken
relative to it). Extra, non-record files under either corpus root are ignored;
only `*.chunk.json` and `manifest.json` are records.

Algorithms are fixed here so the verdict is reproducible:

  * Tab resolution (mirrors migrate-v9): match a chunk's `source.filename` by
    exact basename, else by extension-insensitive stem. Candidate tabs that are
    byte-identical (equal digest) are NOT ambiguous — their digest is pinned.

  * Record-tree digest: collect every record file, take its POSIX path relative
    to the corpus root, sort those paths as UTF-8 strings, and for each emit the
    line  f"{sha256_hex(file_bytes)}  {relpath}\n"  (two spaces). The tree digest
    is sha256_hex of those lines concatenated in sorted order.

  * File-level holdout (HoldoutTargetSourceFile): the key is `source.sha256`. A
    source is held out iff int(sha256_hex(key)[:8], 16) % 5 == 0 — a fixed 1-of-5
    content bucket (~20%), independent of ordering. The holdout law requires that
    no source key land on both sides.
"""
import hashlib
import json
import os
import sys
from pathlib import Path

TOTAL_CHUNKS = 9907
CURATED = ("manifest.json", 220)
INGEST = ("ingest/manifest.json", 9687)

# ── snapshot fingerprint ─────────────────────────────────────────────────────
# These bind the verifier to the exact corpus snapshot documented in
# docs/audit/2026-07-v9-corpus-backfill.md — not merely to "some correctly
# migrated 9907-record corpus". A different corpus, even one this same tool
# migrated correctly, is EXPECTED to fail these checks: this is an evidence
# verifier, and the evidence is a specific snapshot. If the corpus legitimately
# changes, regenerate the report and update these constants together.
EXPECTED_BEFORE_TREE = "875b89f8891c3eb15b19e5e5151732bc36cb7c82860654193b8ecd23a1b61927"
EXPECTED_AFTER_TREE = "352511fcc38da87e49d7ed520c41b4eb897bcee571efc97c256461f2a0bbfe62"
EXPECTED_SOURCES = 399
EXPECTED_HOLDOUT_SOURCES = 77
EXPECTED_HOLDOUT_CHUNKS = 1792
EXPECTED_TRAIN_SOURCES = 322
EXPECTED_TRAIN_CHUNKS = 8115

# Committed census artifacts, relative to this script (repo-root/migrate/…).
_REPO = Path(__file__).resolve().parent.parent
COMMITTED_CENSUS_BEFORE = _REPO / "census" / "corpus-health.json"
COMMITTED_CENSUS_AFTER = _REPO / "docs" / "audit" / "2026-07-v9-corpus-health.json"


def sha_hex(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()


def sha_file(p: Path) -> str:
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for blk in iter(lambda: f.read(1 << 16), b""):
            h.update(blk)
    return h.hexdigest()


def build_tab_index(tabs: Path):
    by_basename, by_stem = {}, {}
    for root, _, files in os.walk(tabs):
        for name in files:
            digest = sha_file(Path(root) / name)
            by_basename.setdefault(name, set()).add(digest)
            stem = name.rsplit(".", 1)[0] if "." in name else name
            by_stem.setdefault(stem, set()).add(digest)
    return by_basename, by_stem


def resolve(filename: str, by_basename, by_stem):
    stem = filename.rsplit(".", 1)[0] if "." in filename else filename
    for table, key in ((by_basename, filename), (by_stem, stem)):
        digs = table.get(key)
        if digs:
            return next(iter(digs)) if len(digs) == 1 else "AMBIGUOUS"
    return None


def record_files(corpus: Path):
    recs = list(corpus.rglob("*.chunk.json"))
    recs += [p for p in corpus.rglob("manifest.json")]
    return recs


def tree_digest(corpus: Path) -> str:
    lines = []
    for p in record_files(corpus):
        rel = p.relative_to(corpus).as_posix()
        lines.append((rel, sha_file(p)))
    lines.sort(key=lambda t: t[0])
    blob = "".join(f"{d}  {rel}\n" for rel, d in lines)
    return sha_hex(blob.encode("utf-8"))


def held_out(key: str) -> bool:
    return int(sha_hex(key.encode("utf-8"))[:8], 16) % 5 == 0


def load(p: Path):
    with open(p, encoding="utf-8") as f:
        return json.load(f)


def main() -> int:
    before, after, tabs = (Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]))
    by_basename, by_stem = build_tab_index(tabs)
    fails = []

    def require(cond, msg):
        if not cond:
            fails.append(msg)

    # ── standalone chunk records ─────────────────────────────────────────────
    before_chunks = sorted(before.rglob("*.chunk.json"))
    after_chunks = sorted(after.rglob("*.chunk.json"))
    require(len(before_chunks) == TOTAL_CHUNKS, f"before chunks {len(before_chunks)} != {TOTAL_CHUNKS}")
    require(len(after_chunks) == TOTAL_CHUNKS, f"after chunks {len(after_chunks)} != {TOTAL_CHUNKS}")

    before_sha = 0
    sha_present = track_present = sha_correct = struct_ok = 0
    keyed = []
    for bp in before_chunks:
        rel = bp.relative_to(before)
        if load(bp)["source"].get("sha256"):
            before_sha += 1
    for ap in after_chunks:
        rel = ap.relative_to(after)
        a = load(ap)
        src = a["source"]
        if src.get("sha256"):
            sha_present += 1
            keyed.append(src["sha256"])
        if src.get("track_index") is not None:
            track_present += 1
        if resolve(src["filename"], by_basename, by_stem) == src.get("sha256"):
            sha_correct += 1
        else:
            if len(fails) < 40:
                fails.append(f"sha mismatch {rel}")
        bp = before / rel
        if bp.exists():
            b = load(bp)
            a2 = json.loads(json.dumps(a))
            a2["source"].pop("sha256", None)
            if a2 == b:
                struct_ok += 1
            elif len(fails) < 40:
                fails.append(f"structural drift beyond sha256: {rel}")
        else:
            fails.append(f"missing before record: {rel}")

    print("== standalone chunks ==")
    print(f"  before / after count   : {len(before_chunks)} / {len(after_chunks)}")
    print(f"  before sha256 present  : {before_sha}  (v8 baseline; must be 0)")
    print(f"  after  sha256 coverage : {sha_present}/{len(after_chunks)}")
    print(f"  after  track_index set : {track_present}  (must be 0)")
    print(f"  sha256 correct vs tab  : {sha_correct}/{len(after_chunks)}")
    print(f"  structural-identical   : {struct_ok}/{len(after_chunks)}  (only sha256 added)")

    require(before_sha == 0, "before corpus already carried sha256")
    require(sha_present == TOTAL_CHUNKS, "sha256 coverage < 100%")
    require(track_present == 0, "track_index was set (must never be guessed)")
    require(sha_correct == TOTAL_CHUNKS, "some sha256 do not match the resolved tab")
    require(struct_ok == TOTAL_CHUNKS, "structural drift beyond the added sha256")

    # ── manifests ────────────────────────────────────────────────────────────
    print("== manifests ==")
    for rel, n in (CURATED, INGEST):
        b, a = load(before / rel), load(after / rel)
        require(b["schema_version"] == 8, f"{rel}: before schema != 8")
        require(a["schema_version"] == 9, f"{rel}: after schema != 9")
        require(len(a["chunks"]) == n, f"{rel}: chunk count != {n}")
        cov = sum(1 for c in a["chunks"] if c["source"].get("sha256"))
        trk = sum(1 for c in a["chunks"] if c["source"].get("track_index") is not None)
        cor = sum(1 for c in a["chunks"] if resolve(c["source"]["filename"], by_basename, by_stem) == c["source"].get("sha256"))
        a2 = json.loads(json.dumps(a))
        a2["schema_version"] = 8
        for c in a2["chunks"]:
            c["source"].pop("sha256", None)
        require(a2 == b, f"{rel}: manifest drift beyond sha256 + schema bump")
        require(cov == n and cor == n and trk == 0, f"{rel}: coverage/correctness/track check failed")
        print(f"  {rel:22} chunks={n} schema 8->{a['schema_version']} sha={cov}/{n} track={trk} correct={cor}/{n}")

    # ── record-tree digests ──────────────────────────────────────────────────
    print("== record-tree digests (defined in this file) ==")
    tdb, tda = tree_digest(before), tree_digest(after)
    print(f"  before : {tdb}")
    print(f"  after  : {tda}")
    require(tdb != tda, "record-tree digests identical (migration changed nothing?)")
    require(tdb == EXPECTED_BEFORE_TREE, f"before tree digest != snapshot fingerprint ({tdb})")
    require(tda == EXPECTED_AFTER_TREE, f"after tree digest != snapshot fingerprint ({tda})")

    # ── file-level holdout ───────────────────────────────────────────────────
    print("== file-level holdout (HoldoutTargetSourceFile) ==")
    require(len(keyed) == TOTAL_CHUNKS, "some chunk lacked a holdout key")
    sources = sorted(set(keyed))
    hold_src = {s for s in sources if held_out(s)}
    train_src = {s for s in sources if not held_out(s)}
    hold_ch = sum(1 for k in keyed if k in hold_src)
    train_ch = sum(1 for k in keyed if k in train_src)
    leakage = len(hold_src & train_src)
    print(f"  distinct source files  : {len(sources)}")
    print(f"  holdout : {len(hold_src)} sources -> {hold_ch} chunks")
    print(f"  train   : {len(train_src)} sources -> {train_ch} chunks")
    print(f"  cross-side leakage     : {leakage}  (must be 0)")
    print(f"  unassigned chunks      : {TOTAL_CHUNKS - hold_ch - train_ch}")
    require(leakage == 0, "source file present on both holdout sides")
    require(hold_ch + train_ch == TOTAL_CHUNKS, "chunks unassigned by holdout")
    require(len(hold_src) > 0 and len(train_src) > 0, "degenerate holdout split")
    # bind to the documented snapshot counts, not just the invariants
    require(len(sources) == EXPECTED_SOURCES, f"distinct sources {len(sources)} != {EXPECTED_SOURCES}")
    require(len(hold_src) == EXPECTED_HOLDOUT_SOURCES, f"holdout sources {len(hold_src)} != {EXPECTED_HOLDOUT_SOURCES}")
    require(hold_ch == EXPECTED_HOLDOUT_CHUNKS, f"holdout chunks {hold_ch} != {EXPECTED_HOLDOUT_CHUNKS}")
    require(len(train_src) == EXPECTED_TRAIN_SOURCES, f"train sources {len(train_src)} != {EXPECTED_TRAIN_SOURCES}")
    require(train_ch == EXPECTED_TRAIN_CHUNKS, f"train chunks {train_ch} != {EXPECTED_TRAIN_CHUNKS}")

    # ── census artifacts (optional args): bind fresh output to committed ──────
    if len(sys.argv) >= 6:
        census_before, census_after = Path(sys.argv[4]), Path(sys.argv[5])
        print("== census artifacts vs committed ==")
        for fresh, committed in ((census_before, COMMITTED_CENSUS_BEFORE),
                                 (census_after, COMMITTED_CENSUS_AFTER)):
            same = fresh.read_bytes() == committed.read_bytes()
            print(f"  {fresh.name:16} == {committed.relative_to(_REPO)} : {'IDENTICAL' if same else 'DIFFERS'}")
            require(same, f"{fresh} != committed {committed}")
    else:
        print("== census artifacts: not checked (pass before.json after.json to bind) ==")

    print()
    if fails:
        print(f"FAIL ({len(fails)} problem(s)):")
        for m in fails[:40]:
            print("  -", m)
        return 1
    print("ALL CHECKS PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
