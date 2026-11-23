"""Fretboard realization: string/fret assignment with technique derivation.

Single lines are realised by a Viterbi pass over (string, fret, hand position,
tapped) states; chords by a voicing search inside one hand window.
"""

OPEN = [40, 45, 50, 55, 59, 64]  # string index 0 = low E ... 5 = high E
MAX_FRET = 22
SPAN = 4


def _candidates(pitch):
    out = []
    for s, o in enumerate(OPEN):
        f = pitch - o
        if 0 <= f <= MAX_FRET:
            out.append((s, f))
    return out


FAST = 0.12  # seconds


def realize_line(notes, allow_tap, sec16=None):
    """notes: time-ordered dicts with start, dur, pitch. Adds string, fret,
    tech. Returns total cost. With sec16 (seconds per sixteenth), a hand jump
    over 4 frets or a skip over 2 strings within FAST seconds is priced as
    unplayable, so it is taken only when the pitches leave no other path."""
    if not notes:
        return 0.0
    layers = []
    prev = None
    for i, n in enumerate(notes):
        layer = {}
        cands = _candidates(n["pitch"])
        if not cands:
            raise ValueError(f"pitch {n['pitch']} not on fretboard")
        gap = n["start"] - notes[i - 1]["start"] if i else 99
        for s, f in cands:
            if prev is None:
                hs = [1] if f == 0 else list(range(max(1, f - SPAN + 1), f + 1))
                for h in hs:
                    layer[(s, f, h, False)] = (f * 0.05, None)
                continue
            for key, (pc, _) in prev.items():
                ps, pf, ph, ptap = key
                opts = []
                if f == 0:
                    opts.append((ph, False, 0.0))
                elif ph <= f <= ph + SPAN - 1:
                    opts.append((ph, False, 0.0))
                elif f == ph + SPAN:
                    opts.append((ph, False, 0.6))
                if f > 0 and not (ph <= f <= ph + SPAN - 1):
                    for h in range(max(1, f - SPAN + 1), f + 1):
                        shift = abs(h - ph)
                        speed = 1.0 if gap >= 2 else 2.2
                        opts.append((h, False, 0.8 + shift * 0.35 * speed))
                if allow_tap and n.get("tappable") and f >= ph + SPAN + 2 and s == ps and not ptap:
                    opts.append((ph, True, 0.5))
                if allow_tap and ptap and f > 0 and s == ps and ph <= f <= ph + SPAN - 1:
                    opts.append((ph, False, -0.3))
                fast = sec16 is not None and 0 < gap * sec16 < FAST
                for h, tap, c in opts:
                    if fast and (abs(h - ph) > 4 or abs(s - ps) > 2 or (f and pf and abs(f - pf) >= 7)):
                        c += 25.0
                    c += 0.12 * abs(s - ps)
                    if gap < 2 and s == ps and pf == f and f > 0:
                        c += 0.4
                    if ptap and s != ps:
                        c += 1.0
                    c += 0.02 * f + 0.06 * max(0, f - 12) * (1 + 0.4 * (5 - s))
                    k = (s, f, h, tap)
                    tot = pc + c
                    if k not in layer or tot < layer[k][0]:
                        layer[k] = (tot, key)
        if not layer:
            raise ValueError("no fretboard path")
        layers.append(layer)
        prev = layer
    key = min(prev, key=lambda k: prev[k][0])
    total = prev[key][0]
    path = []
    for layer in reversed(layers):
        path.append(key)
        key = layer[key][1]
    path.reverse()
    for i, (n, (s, f, h, tap)) in enumerate(zip(notes, path)):
        n["string"], n["fret"] = s, f
        tech = []
        if tap:
            tech.append("tap")
        elif i:
            p = notes[i - 1]
            ps, pf = p["string"], p["fret"]
            sounding = p["start"] + p["dur"] >= n["start"] - 0.01
            close = n["start"] - p["start"] <= 2
            if s == ps and pf != f and sounding and close:
                if n.get("art") == "slide" or (abs(f - pf) <= 2 and n["start"] - p["start"] >= 2):
                    tech.append("slide")
                elif f > pf:
                    tech.append("hammer")
                else:
                    tech.append("pull")
            elif s == ps and pf != f and n.get("art") == "slide" and abs(f - pf) <= 5:
                tech.append("slide")
        if n.get("art") == "pm":
            tech.append("pm")
        n["tech"] = tech
    return total


def apply_ring(notes, max_ring):
    """Let notes ring until the same string is re-struck (physical cut)."""
    by_string = {}
    for n in sorted(notes, key=lambda x: x["start"]):
        by_string.setdefault(n["string"], []).append(n)
    for seq in by_string.values():
        for a, b in zip(seq, seq[1:]):
            limit = b["start"] - a["start"]
            a["dur"] = min(max(a["dur"], min(max_ring, limit)) if a.get("ring") else a["dur"], limit)
        if seq and seq[-1].get("ring"):
            seq[-1]["dur"] = max(seq[-1]["dur"], max_ring)


def low_cluster(pitches, below=66, span=4):
    """Three or more notes within `span` semitones under `below`: muddy."""
    low = sorted(p for p in pitches if p < below)
    return any(c - a <= span for a, c in zip(low, low[2:]))


def voice_chord(pcs, bass_pc, lo, hi, prev_frets=None, n_strings=4, avoid_cluster=False):
    """Return list of (string, fret, pitch) for a chord voicing whose pitch
    classes are all in pcs, bass note pitch class == bass_pc, in [lo, hi]."""
    best = None
    for h in range(0, 15):
        window = [0] + list(range(max(1, h), h + SPAN))
        for low_s in range(0, 6 - n_strings + 1):
            strings = range(low_s, low_s + n_strings)
            choice = []
            ok = True
            for s in strings:
                opts = [f for f in window if (OPEN[s] + f) % 12 in pcs and lo <= OPEN[s] + f <= hi]
                if not opts:
                    ok = False
                    break
                choice.append((s, opts))
            if not ok:
                continue
            voicing = []
            for s, opts in choice:
                if s == low_s:
                    opts = [f for f in opts if (OPEN[s] + f) % 12 == bass_pc] or None
                    if opts is None:
                        ok = False
                        break
                f = max(opts, key=lambda f: (f == 0, -f)) if h == 0 else min(opts, key=lambda f: abs(f - h - 1))
                voicing.append((s, f, OPEN[s] + f))
            if not ok or len({p for _, _, p in voicing}) < len(voicing):
                continue
            if avoid_cluster and low_cluster([p for _, _, p in voicing]):
                continue
            pcset = {p % 12 for _, _, p in voicing}
            cost = (len(pcs) - len(pcset & set(pcs))) * 1.5
            fretted = [f for _, f, _ in voicing if f > 0]
            if fretted:
                cost += (max(fretted) - min(fretted)) * 0.2
            cost -= 0.25 * sum(1 for _, f, _ in voicing if f == 0)
            if prev_frets is not None:
                cost += 0.15 * abs((sum(fretted) / len(fretted) if fretted else 0) - prev_frets)
            if best is None or cost < best[0]:
                best = (cost, voicing)
    return best[1] if best else None
