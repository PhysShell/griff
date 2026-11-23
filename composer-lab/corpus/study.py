"""Corpus dosing study (see CORPUS-STUDY.md).

python corpus/study.py <tabs-dir> <out-dir>

Writes per-song statistics, the sample log and an audit sheet to <out-dir>
(keep it local: it names copyrighted songs and prints bars from them) and
prints only aggregates.
"""
import hashlib
import json
import random
import re
import statistics
import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

BANDS = {"Dance Gavin Dance": 7, "Hail The Sun": 4, "A Lot Like Birds": 4}
SKIP_NAME = re.compile(r"bass|vocal|vox|voice|drum|synth|key|piano|lyric|organ|omnichord|string|choir|pad", re.I)
HARMONY = {3, 4, 8, 9, 15, 16}
Q = 960


# ---------------------------------------------------------------- corpus reading

def play_order(headers):
    """Measure indices in playing order with repeats and alternate endings expanded."""
    order, i, start, done = [], 0, 0, {}
    while i < len(headers) and len(order) < 5000:
        h = headers[i]
        if h.isRepeatOpen:
            start = i
        p = done.get(start, 0) + 1
        if h.repeatAlternative and not (h.repeatAlternative >> (p - 1)) & 1:
            i += 1
            continue
        order.append(i)
        if h.repeatClose > 0:
            if done.get(start, 0) < h.repeatClose:
                done[start] = done.get(start, 0) + 1
                i = start
                continue
            done[start] = 0
        i += 1
    return order


def guitar_tracks(song):
    tracks = [t for t in song.tracks if not t.isPercussionTrack and len(t.strings) >= 6
              and not SKIP_NAME.search(t.name)]
    count = lambda t: sum(len(b.notes) for m in t.measures for v in m.voices for b in v.beats)
    tracks = sorted(tracks, key=count, reverse=True)
    return [t for t in tracks if count(t) > 0][:2], [t.name for t in tracks]


def bar_notes(measure):
    """(onset16, pitch) of struck notes, or None when voice 1 has notes but does not
    fill the bar. Guitar Pro stores an empty bar as a single quarter rest."""
    v0 = measure.voices[0].beats
    if sum(b.duration.time for b in v0) != measure.length and any(b.notes for b in v0):
        return None
    out = []
    for v in measure.voices:
        for b in v.beats:
            for n in b.notes:
                if n.type.name in ("tie", "dead"):
                    continue
                out.append((round((b.start - measure.start) / (Q / 4)), n.realValue))
    return out


def load_song(path):
    import guitarpro
    song = guitarpro.parse(str(path))
    tracks, names = guitar_tracks(song)
    if len(tracks) < 2:
        return None, names
    bars, dropped = [], 0
    for i in play_order(song.measureHeaders):
        h = song.measureHeaders[i]
        L = h.timeSignature.numerator * 16 // h.timeSignature.denominator.value
        parts = [bar_notes(t.measures[i]) for t in tracks]
        if any(p is None for p in parts):
            dropped += 1
            continue
        bars.append({"L": L, "parts": parts, "src": i + 1})
    return {"bars": bars, "dropped": dropped, "tracks": [t.name for t in tracks]}, names


def sample(tabs):
    files = sorted(p for p in Path(tabs).iterdir() if p.suffix.lower() in (".gp3", ".gp4", ".gp5"))
    rng = random.Random(20261124)
    chosen, log = [], []
    for band, n in BANDS.items():
        pool = [p for p in files if p.name.startswith(band + " - ") and "(ver" not in p.name.lower()]
        rng.shuffle(pool)
        got = 0
        for p in pool:
            if got == n:
                break
            try:
                song, names = load_song(p)
            except Exception as e:
                log.append({"file": p.name, "replaced": f"load error: {e}"})
                continue
            if song is None:
                log.append({"file": p.name, "replaced": f"fewer than two guitar tracks: {names}"})
                continue
            got += 1
            chosen.append((p, song))
            log.append({"file": p.name, "sha256": hashlib.sha256(p.read_bytes()).hexdigest(),
                        "tracks": song["tracks"], "bars": len(song["bars"]), "dropped_bars": song["dropped"]})
    return chosen, log


# ---------------------------------------------------------------- statistics

def onsets(notes):
    return {o for o, _ in notes}


def relation(bar):
    a, b = bar["parts"]
    oa, ob = onsets(a), onsets(b)
    half = bar["L"] / 2
    if not oa or not ob:
        return "one_silent" if (oa or ob) else "both_silent"
    shared = oa & ob
    if len(shared) >= 0.8 * max(len(oa), len(ob)):
        iv = Counter()
        for o in shared:
            pa = max(p for t, p in a if t == o)
            pb = max(p for t, p in b if t == o)
            iv[abs(pa - pb)] += 1
        top = iv.most_common(1)[0][0]
        if top == 0:
            return "unison"
        if top in (12, 24):
            return "octave"
        if top in HARMONY:
            return "harmony"
        return "same_rhythm"
    busy = (len(oa) >= half, len(ob) >= half)
    if all(busy):
        return "both_busy"
    if any(busy):
        return "one_busy"
    return "neither_busy"


def pattern(bar):
    return tuple(sorted(onsets(bar["parts"][0]) | onsets(bar["parts"][1])))


def squares(bars):
    out, i = [], 0
    while i + 3 < len(bars):
        w = bars[i:i + 4]
        if len({b["L"] for b in w}) == 1 and pattern(w[0]) and pattern(w[0]) == pattern(w[1]) == pattern(w[2]):
            p3, p4 = len(pattern(w[0])), pattern(w[3])
            if p4 == pattern(w[0]):
                kind = "repeat"
            elif len(p4) <= 2:
                kind = "stop"
            elif len(p4) >= 1.3 * p3:
                kind = "denser"
            elif len(p4) <= 0.7 * p3:
                kind = "thinner"
            else:
                kind = "shifted"
            out.append((i, kind))
            i += 4
        else:
            i += 1
    return out


def runs(bars):
    key = lambda b: (b["L"], tuple(sorted(b["parts"][0])), tuple(sorted(b["parts"][1])))
    out, cur = [], 1
    for x, y in zip(bars, bars[1:]):
        if key(x) == key(y) and pattern(x):
            cur += 1
        else:
            out.append(cur)
            cur = 1
    out.append(cur)
    return [r for r in out if r > 1] or [1]


def last_bar(bars):
    live = [b for b in bars if pattern(b)]
    if not live:
        return "none"
    b = live[-1]
    n = len(pattern(b))
    if n >= b["L"] / 2:
        return "busy"
    return "held" if n <= 2 else "stop"


def song_stats(bars):
    two = [b for b in bars if pattern(b)]
    rel = Counter(relation(b) for b in two)
    sq = squares(bars)
    kinds = Counter(k for _, k in sq)
    return {
        "bars": len(bars),
        "relation": {k: rel[k] / max(1, len(two)) for k in
                     ("one_busy", "both_busy", "unison", "octave", "harmony", "same_rhythm", "neither_busy",
                      "one_silent")},
        "squares": len(sq),
        "square_varied": (1 - kinds["repeat"] / len(sq)) if sq else None,
        "square_kind": {k: kinds[k] / len(sq) for k in ("repeat", "stop", "denser", "thinner", "shifted")} if sq else None,
        "median_run": statistics.median(runs(bars)),
        "last_bar": last_bar(bars),
    }


# ---------------------------------------------------------------- generator side

def piece_bars(piece):
    ch = piece["chords"]
    out = []
    for i, c in enumerate(ch):
        s = c["start"]
        e = ch[i + 1]["start"] if i + 1 < len(ch) else piece["total16"]
        parts = [[(int(n["start"] - s), n["pitch"]) for n in piece["parts"][g] if s <= n["start"] < e]
                 for g in ("g1", "g2")]
        out.append({"L": e - s, "parts": parts, "src": i + 1})
    return out


def generator_stats(seeds, **flags):
    import composer
    res = []
    for seed in seeds:
        args = composer.argparse.Namespace(meter=None, mode=None, key=None, tempo=None, **flags)
        res.append(song_stats(piece_bars(composer.compose(composer.make_recipe(seed * 1000, args)))))
    return res


def summary(stats):
    def q(vals):
        vals = sorted(v for v in vals if v is not None)
        if not vals:
            return None
        qs = statistics.quantiles(vals, n=4) if len(vals) > 1 else [vals[0]] * 3
        return {"median": round(statistics.median(vals), 3), "q1": round(qs[0], 3), "q3": round(qs[2], 3), "n": len(vals)}
    out = {"square_varied": q([s["square_varied"] for s in stats]),
           "median_run": q([s["median_run"] for s in stats]),
           "squares_per_song": q([s["squares"] for s in stats])}
    for k in stats[0]["relation"]:
        out["rel_" + k] = q([s["relation"][k] for s in stats])
    for k in ("repeat", "stop", "denser", "thinner", "shifted"):
        out["sq_" + k] = q([s["square_kind"][k] for s in stats if s["square_kind"]])
    lb = Counter(s["last_bar"] for s in stats)
    out["last_bar"] = {k: round(v / len(stats), 3) for k, v in lb.items()}
    return out


def tab_lines(bar):
    rows = []
    for name, notes in zip(("g1", "g2"), bar["parts"]):
        cells = ["."] * bar["L"]
        for o, p in notes:
            if 0 <= o < bar["L"]:
                cells[o] = str(p)
        rows.append(f"{name}: " + " ".join(c.rjust(2) for c in cells))
    return rows


def main():
    tabs, out = Path(sys.argv[1]), Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)
    chosen, log = sample(tabs)
    (out / "sample_log.json").write_text(json.dumps(log, indent=1, ensure_ascii=False), encoding="utf-8")
    per = [dict(song_stats(s["bars"]), file=p.name) for p, s in chosen]
    (out / "per_song.json").write_text(json.dumps(per, indent=1, ensure_ascii=False), encoding="utf-8")
    rng = random.Random(20261124)
    order = list(range(len(per)))
    rng.shuffle(order)
    halves = [[per[i] for i in order[: len(order) // 2]], [per[i] for i in order[len(order) // 2:]]]
    audit, pool = [], [(p.name, b) for p, s in chosen for b in s["bars"] if pattern(b)]
    for name, b in rng.sample(pool, 20):
        audit.append(f"{name} bar {b['src']} (L={b['L']}): {relation(b)}\n" + "\n".join(tab_lines(b)))
    (out / "audit.txt").write_text("\n\n".join(audit), encoding="utf-8")
    result = {
        "songs": len(per),
        "replacements": sum("replaced" in x for x in log),
        "dropped_bars": sum(x.get("dropped_bars", 0) for x in log),
        "corpus": summary(per),
        "half_a": summary(halves[0]),
        "half_b": summary(halves[1]),
        "v1": summary(generator_stats(range(401, 441))),
        "v4": summary(generator_stats(range(401, 441), arrange=True, repeat=True)),
    }
    (out / "summary.json").write_text(json.dumps(result, indent=1), encoding="utf-8")
    print(json.dumps(result, indent=1))


if __name__ == "__main__":
    main()
