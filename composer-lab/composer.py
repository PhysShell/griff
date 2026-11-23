"""Griff composer-lab: hierarchical motif-development composer for two guitars.

Layers: intent -> form -> harmony -> motif family -> phrase programs
(development operators) -> guitar roles -> fretboard realization.
Every sampled decision is stored in a recipe so a piece can be rebuilt.
"""
import argparse
import copy
import functools
import hashlib
import itertools
import json
import math
import random
import sys
from pathlib import Path

import fretboard
import musicxml
from midiio import write_midi

SCALES = {
    "ionian": [0, 2, 4, 5, 7, 9, 11],
    "lydian": [0, 2, 4, 6, 7, 9, 11],
    "mixolydian": [0, 2, 4, 5, 7, 9, 10],
    "dorian": [0, 2, 3, 5, 7, 9, 10],
    "aeolian": [0, 2, 3, 5, 7, 8, 10],
}
TONICS = {"E": 4, "F#": 6, "A": 9, "B": 11, "D": 2, "G": 7, "C#": 1}
METERS = {  # name: (sixteenths per bar, numerator, denominator, groupings)
    "4/4": (16, 4, 4, [[3, 3, 3, 3, 4], [3, 3, 2, 3, 3, 2], [4, 4, 4, 4], [3, 3, 4, 3, 3]]),
    "7/8": (14, 7, 8, [[4, 4, 3, 3], [3, 4, 3, 4], [4, 4, 6], [2, 2, 3, 2, 2, 3]]),
    "6/8": (12, 6, 8, [[3, 3, 3, 3], [2, 2, 2, 3, 3], [6, 6]]),
    "5/4": (20, 5, 4, [[4, 4, 4, 4, 4], [3, 3, 4, 3, 3, 4], [6, 6, 4, 4]]),
}
G1_RANGE = (55, 86)
G2_RANGE = (40, 72)


class Theory:
    def __init__(self, tonic_pc, mode):
        self.tonic_pc = tonic_pc
        self.scale = SCALES[mode]
        self.base = 48 + tonic_pc

    def midi(self, deg):
        o, i = divmod(deg, 7)
        return self.base + 12 * o + self.scale[i]

    def deg_near(self, midi):
        best = None
        for d in range(-21, 35):
            dist = abs(self.midi(d) - midi)
            if best is None or dist < best[0]:
                best = (dist, d)
        return best[1]

    def triad_ok(self, root):
        r, t, f = (self.midi(root + k) for k in (0, 2, 4))
        return f - r == 7

    def color_ok(self, root, k):
        """A 9th (k=1) colours the chord only when it is a major 9th."""
        return k != 1 or (self.midi(root + 1) - self.midi(root)) % 12 == 2

    def chord_pcs(self, root, ext=(0, 2, 4)):
        return {self.midi(root + k) % 12 for k in ext if self.color_ok(root, k)}


def stable(deg, root):
    return (deg - root) % 7 in (0, 2, 4)


# ---------------------------------------------------------------- planning

def plan_intent(rng, args):
    meter = args.meter or rng.choice(["4/4", "4/4", "7/8", "7/8", "6/8", "5/4"])
    L, num, den, groupings = METERS[meter]
    grouping = rng.choice(groupings)
    mode = args.mode or rng.choice(["ionian", "lydian", "lydian", "mixolydian", "dorian", "aeolian"])
    tonic = args.key or rng.choice(list(TONICS))
    base_tempo = {"4/4": (128, 160), "7/8": (140, 176), "6/8": (120, 150), "5/4": (130, 160)}[meter]
    tempo = args.tempo or rng.randrange(*base_tempo)
    b_meter = meter
    if rng.random() < 0.3:
        b_meter = rng.choice([m for m in METERS if m != meter])
    return {
        "meter": meter, "grouping": grouping, "mode": mode, "tonic": tonic,
        "tempo": tempo, "b_meter": b_meter,
        "b_grouping": rng.choice(METERS[b_meter][3]),
        "lead": rng.choice(["g1", "g1", "g2"]),
        "intro_bars": rng.choice([1, 2, 2]),
    }


def sample_prog(rng, th, n, start_choices, end_choices):
    moves = [(3, 3.0), (5, 2.5), (1, 2.0), (6, 1.2), (4, 1.5), (2, 1.0)]
    for _ in range(200):
        prog = [rng.choice(start_choices)]
        while len(prog) < n - 1:
            r = prog[-1]
            opts = [((r + m) % 7, w) for m, w in moves if th.triad_ok((r + m) % 7)]
            tot = sum(w for _, w in opts)
            x = rng.random() * tot
            for c, w in opts:
                x -= w
                if x <= 0:
                    prog.append(c)
                    break
        prog.append(rng.choice(end_choices))
        if all(a != b for a, b in zip(prog, prog[1:])) and len(set(prog)) >= min(3, n):
            return prog
    return prog


def plan_harmony(rng, th, intent):
    ok = [r for r in range(7) if th.triad_ok(r)]
    starts = [r for r in (0, 3, 5) if r in ok] or [0]
    a = sample_prog(rng, th, 4, starts, [r for r in (4, 3, 1) if r in ok] or [0])
    if 0 not in a:
        a[0 if a[1] != 0 else 2] = 0
    a2 = list(a)
    alt = [r for r in ok if r not in (a[2], a[1])]
    a2[2] = rng.choice(alt)
    a2[3] = rng.choice([r for r in (5, 4, 3) if r in ok and r != a2[2]] or [a[3]])
    b = sample_prog(rng, th, 2, [r for r in (5, 3, 1) if r in ok and r != a[0]] or [3],
                    [r for r in (4, 1, 6) if r in ok] or [4])
    final = list(a[:2]) + [rng.choice([r for r in (3, 1) if r in ok and r != a[1]] or [a[2]]),
                          rng.choice([r for r in (4, 6) if r in ok] or [4])]
    return {"A": a, "A2": a2, "B": b, "A3": final, "end": 0}


def gen_rhythm(rng, L, grouping, density, tail_rest=True):
    starts = []
    p = 0
    for g in grouping:
        starts.append(p)
        p += g
    on = {0}
    for s in starts[1:]:
        if rng.random() < 0.7:
            on.add(s)
    for s in starts[1:]:
        if rng.random() < 0.35 * density:
            on.add(s - 1)
            if rng.random() < 0.5:
                on.add(s - 2)
    for _ in range(int(L * 0.12 * density)):
        on.add(rng.randrange(1, L))
    if rng.random() < 0.6:
        tail = rng.choice([3, 4])
        if tail_rest:
            on = {o for o in on if o < L - tail} or {0}
    on = sorted(on)
    while len(on) < 4:
        cand = rng.randrange(1, L - 2)
        if cand not in on:
            on.append(cand)
            on.sort()
    return on


def gen_contour(rng, n, arch):
    offs = [0]
    for i in range(1, n):
        phase = i / max(1, n - 1)
        up_bias = (0.65 if phase < 0.55 else 0.35) if arch == "up" else (0.35 if phase < 0.55 else 0.65)
        last_leap = len(offs) > 1 and abs(offs[-1] - offs[-2]) >= 3
        if last_leap:
            step = -1 if offs[-1] > offs[-2] else 1
        else:
            x = rng.random()
            mag = 1 if x < 0.55 else (0 if x < 0.62 else (2 if x < 0.84 else rng.choice([3, 4])))
            step = mag if rng.random() < up_bias else -mag
        nxt = max(-6, min(8, offs[-1] + step))
        offs.append(nxt)
    return offs


def gen_cell_hook(rng, L, grouping, tail_rest=True):
    """Hook = a 2-4 note interval cell sequenced across the meter's groups."""
    cell_len = rng.choice([2, 3, 3, 4])
    cell = [0]
    for _ in range(cell_len - 1):
        cell.append(cell[-1] + rng.choice([1, 1, 2, -1, -2, 3, -3, 4]))
    seq_step = rng.choice([-1, -1, 1, 2, -2, 3])
    rhythm, contour = [], []
    pos, k = 0, 0
    rests = rng.random() < 0.5 and tail_rest
    for gi, g in enumerate(grouping):
        base = k * seq_step
        n = min(g, cell_len)
        spacing = 1 if g <= cell_len + 1 or rng.random() < 0.6 else 2
        for i in range(n):
            on = pos + i * spacing
            if on < pos + g:
                rhythm.append(on)
                contour.append(base + cell[i])
        if rests and gi == len(grouping) - 1 and len(rhythm) > 3:
            keep = [j for j, o in enumerate(rhythm) if o < pos + 1]
            rhythm, contour = [rhythm[j] for j in keep], [contour[j] for j in keep]
        pos += g
        k += 1
    shift = contour[0]
    contour = [c - shift for c in contour]
    lo = min(contour)
    if lo < -6:
        contour = [c - lo - 6 for c in contour]
    return rhythm, contour, {"cell": cell, "seq_step": seq_step, "rest_tail": rests}


def plan_motifs(rng, intent, tail_rest=True, _inner=False):
    if not tail_rest and not _inner:
        # The hook without its trailing rest is drawn from a copy of the RNG, so
        # every draw after it (and the rest of the recipe) stays as with the rest.
        alt = random.Random()
        alt.setstate(rng.getstate())
        out = plan_motifs(rng, intent)
        full = plan_motifs(alt, intent, False, _inner=True)["hook"]
        out["hook"].update({k: full[k] for k in ("rhythm", "contour", "slides", "cellinfo")})
        return out
    L, _, _, _ = METERS[intent["meter"]]
    arch = rng.choice(["up", "down"])
    if rng.random() < 0.7:
        rhythm, contour, cellinfo = gen_cell_hook(rng, L, intent["grouping"], tail_rest)
    else:
        rhythm = gen_rhythm(rng, L, intent["grouping"], rng.uniform(1.0, 1.6), tail_rest)
        contour = gen_contour(rng, len(rhythm), arch)
        cellinfo = {"cell": None}
    slides = [i for i in range(1, len(rhythm)) if rng.random() < 0.15]
    cycle = rng.choice([3, 4, 5, 5, 6, 7])
    for _ in range(50):
        ladder_pick = [0] + [rng.randrange(0, 7) for _ in range(cycle - 1)]
        colorful = any(i in (3, 4) for i in ladder_pick)
        no_rep = all(a != b for a, b in zip(ladder_pick, ladder_pick[1:] + ladder_pick[:1]))
        if colorful and no_rep and len(set(ladder_pick)) >= 3:
            break
    texture = rng.choice(["arp", "arp", "pedal", "tap", "tap"])
    bL = METERS[intent["b_meter"]][0]
    b_acc = accents_of(intent["b_grouping"])
    chug = sorted({a + 1 for a in b_acc if rng.random() < 0.5 and a + 1 < bL} |
                  {x for x in range(bL) if x % 2 == 0 and rng.random() < 0.25})
    return {
        "hook": {"rhythm": rhythm, "contour": contour, "slides": slides,
                 "anchor": rng.choice([0, 2, 2, 4, 1]), "arch": arch, "cellinfo": cellinfo,
                 "push": rng.random() < 0.4, "run_first": rng.random() < 0.5},
        "texture": texture,
        "chug": chug,
        "arp": {"cycle": ladder_pick, "ladder": rng.choice(
            [[0, 4, 7, 8, 9, 11, 14], [0, 2, 4, 7, 8, 9, 11], [0, 4, 8, 9, 11, 13, 14],
             [0, 4, 7, 9, 11, 12, 14]])},
    }


def plan_form(rng, intent, harmony):
    L = METERS[intent["meter"]][0]
    form = []
    for i in range(intent["intro_bars"]):
        form.append({"sec": "intro", "chord": harmony["A"][i], "prog": "intro"})
    prog_a = rng.choice([["stmt", "seq", "frag", "cad_open"], ["stmt", "seq_orn", "frag", "cad_open"]])
    prog_a2 = rng.choice([["stmt_orn", "invert", "call", "cad_open"], ["stmt", "retro", "call", "cad_open"],
                          ["displace", "seq", "call", "cad_open"]])
    prog_a3 = ["oct_harm", "seq_harm", "frag_harm", "cad_final"]
    for chords, progs, sec in ((harmony["A"], prog_a, "A"), (harmony["A2"], prog_a2, "A2")):
        for c, p in zip(chords, progs):
            form.append({"sec": sec, "chord": c, "prog": p})
    for c in harmony["B"]:
        form.append({"sec": "B", "chord": c, "prog": "b1"})
        form.append({"sec": "B", "chord": c, "prog": "b2"})
    for c, p in zip(harmony["A3"], prog_a3):
        form.append({"sec": "A3", "chord": c, "prog": p})
    form.append({"sec": "end", "chord": harmony["end"], "prog": "end"})
    return form


# ---------------------------------------------------------------- operators

def motif_events(hook, L):
    r = hook["rhythm"]
    out = []
    for i, on in enumerate(r):
        nxt = r[i + 1] if i + 1 < len(r) else L
        out.append({"on": on, "dur": min(nxt - on, 6), "off": hook["contour"][i],
                    "art": "slide" if i in hook["slides"] else None})
    return out


def op(name, ev, L, rng_local):
    ev = [dict(e) for e in ev]
    if name == "invert":
        for e in ev:
            e["off"] = -e["off"]
    elif name == "retro":
        offs = [e["off"] for e in ev][::-1]
        base = offs[0]
        for e, o in zip(ev, offs):
            e["off"] = o - base
    elif name == "frag":
        head = [e for e in ev if e["on"] < L // 2]
        half = L // 2
        second = [dict(e, on=e["on"] + half, off=e["off"] + rng_local.choice([1, 2, -1])) for e in head]
        ev = head + [e for e in second if e["on"] < L]
        for i, e in enumerate(ev):
            nxt = ev[i + 1]["on"] if i + 1 < len(ev) else L
            e["dur"] = min(e["dur"], nxt - e["on"])
    elif name == "call":
        ev = [e for e in ev if e["on"] < L // 2]
    elif name == "orn":
        extra = []
        for e in ev:
            if e["dur"] >= 3 and rng_local.random() < 0.6:
                extra.append({"on": e["on"] + e["dur"] - 1, "dur": 1, "off": e["off"] + 1, "art": None})
                e["dur"] -= 1
        ev = sorted(ev + extra, key=lambda e: e["on"])
    elif name == "displace":
        k = rng_local.choice([1, 2, 3])
        ev = [dict(e, on=(e["on"] + k)) for e in ev if e["on"] + k < L]
    elif name == "seq":
        cut = L // 2
        shift = rng_local.choice([2, -2, 3, -1])
        ev = [dict(e, off=e["off"] + shift) if e["on"] >= cut else e for e in ev]
    elif name == "cad_run":
        head = [e for e in ev if e["on"] < L // 3]
        start = (head[-1]["on"] + 1) if head else 0
        tgt_on = max(start + 3, L - max(3, L // 4))
        run_len = tgt_on - start
        base = head[-1]["off"] if head else 0
        direction = rng_local.choice([1, -1])
        if head:
            head[-1]["dur"] = 1
        run = [{"on": start + i, "dur": 1, "off": base + direction * (i + 1), "art": None} for i in range(run_len)]
        ev = head + run + [{"on": tgt_on, "dur": L - tgt_on, "off": None, "art": None, "cad": True}]
    elif name == "cad":
        head = [e for e in ev if e["on"] < L // 2][:3]
        last_on = (head[-1]["on"] + head[-1]["dur"]) if head else 0
        if head:
            head[-1]["dur"] = max(1, min(head[-1]["dur"], L // 2 - head[-1]["on"]))
        tgt = {"on": max(last_on, L // 2), "dur": L - max(last_on, L // 2), "off": None, "art": "slide", "cad": True}
        ev = head + [tgt]
    return ev


# ---------------------------------------------------------------- realization

def place_line(th, ev, root, anchor_fn, bar_start, accents, lo, hi, vel, harm=0, strict=True, colors=False):
    """colors: accented notes keep 9ths, 11ths, 13ths and 7ths and move only
    when they sit a semitone above a chord tone (a minor ninth); otherwise they
    are pulled onto the triad."""
    anchor = anchor_fn(root)
    prev = None
    degs = []
    triad = {th.midi(root + k) % 12 for k in (0, 2, 4)}
    harsh = lambda d: any((th.midi(d) - c) % 12 == 1 for c in triad)
    for e in ev:
        if e.get("off") is None:
            cands = [d for d in range(anchor - 7, anchor + 8) if (d - root) % 7 in (0, 2)]
            deg = min(cands, key=lambda d: abs(d - (prev if prev is not None else anchor)))
        elif colors:
            deg = anchor + e["off"]
            if (e["on"] in accents or e is ev[-1]) and harsh(deg):
                up = prev is None or deg >= prev
                step = 1 if up else -1
                deg = deg + step if not harsh(deg + step) else deg - step
        else:
            deg = anchor + e["off"]
            if strict and (e["on"] in accents or e is ev[0] or e is ev[-1]) and not stable(deg, root):
                up = prev is None or deg >= prev
                deg = deg + (1 if up else -1) if stable(deg + (1 if up else -1), root) else deg + (-1 if up else 1)
        deg += harm
        if harm and not stable(deg, root) and e["on"] in accents:
            deg -= 1
        degs.append(deg)
        prev = deg - harm
    pitches = [th.midi(d) for d in degs]
    if pitches:
        shift = 0
        while max(pitches) + shift > hi and min(pitches) + shift - 12 >= lo - 5:
            shift -= 12
        while min(pitches) + shift < lo and max(pitches) + shift + 12 <= hi + 5:
            shift += 12
        pitches = [p + shift for p in pitches]
        pitches = [p - 12 if p > 86 else (p + 12 if p < 40 else p) for p in pitches]
    notes = []
    for e, p in zip(ev, pitches):
        notes.append({"start": bar_start + e["on"], "dur": e["dur"], "pitch": p,
                      "vel": vel + (14 if e["on"] in accents else 0) + (8 if e["on"] == 0 else 0),
                      "art": e.get("art")})
    return notes


def arp_bar(th, motif, root, bar_start, L, lo, hi, phase, step, vel, accents, mask=None, shape=None, onsets=None):
    """Arpeggio/pedal texture. Ladder slots >= 3 form an upper shape held as
    common tones across chord changes (moved only on a semitone clash).
    `mask` = lead onsets in this bar; the texture yields to them except on
    accents so the two guitars interlock."""
    ladder = motif["ladder"]
    cyc = motif["cycle"]
    base = root
    while th.midi(base) < lo:
        base += 7
    while th.midi(base + ladder[-1]) > hi and th.midi(base) - 12 >= lo:
        base -= 7
    chord_pcs = th.chord_pcs(root)
    upper = {}
    for idx in range(3, len(ladder)):
        d = base + ladder[idx]
        if shape is not None and idx in shape:
            d = shape[idx]
            pc = th.midi(d) % 12
            if any((pc - c) % 12 in (1, 11) for c in chord_pcs):
                d = min((x for x in range(d - 2, d + 3) if stable(x, root)), key=lambda x: abs(x - d))
        upper[idx] = d
    if shape is not None and not shape:
        shape.update(upper)
    notes = []
    t = 0
    pedal = motif.get("texture") == "pedal"
    grid = sorted(onsets) if onsets is not None else range(0, L, step)
    for t in grid:
        idx = cyc[phase % len(cyc)]
        if pedal:
            idx = (len(ladder) - 1) if (t // step) % 2 else min(idx, len(ladder) - 3)
        d = upper.get(idx, base + ladder[idx])
        if (d - root) % 7 == 1 and not th.color_ok(root, 1):
            d += 1
        p = th.midi(d)
        while p > hi:
            p -= 12
        phase += 1
        if mask is not None and t in mask and t not in accents:
            continue
        notes.append({"start": bar_start + t, "dur": 8, "pitch": p, "ring": True,
                      "tappable": p >= 64 and idx >= 3,
                      "vel": vel + (12 if t in accents else 0) + (8 if t == 0 else 0)})
    return notes, phase


def tap_bar(th, root, bar_start, L, phase, vel, accents, mask, state, reset_on_hits=False):
    """Single-string tapping triad: tap (right hand) -> pull-off -> hammer-on,
    all three chord tones, the string chosen to keep the fretting hand still."""
    pcs = th.chord_pcs(root, (0, 2, 4, 1))
    best = None
    for s in (3, 4, 5):
        for a in range(2, 13):
            if (fretboard.OPEN[s] + a) % 12 not in pcs:
                continue
            for b in range(a + 2, a + 5):
                if (fretboard.OPEN[s] + b) % 12 not in pcs:
                    continue
                for c in range(a + 6, min(a + 13, 22)):
                    if (fretboard.OPEN[s] + c) % 12 not in pcs:
                        continue
                    cost = (abs(a - state.get("a", a)) + 0.5 * abs(s - state.get("s", s)) + 0.1 * (c - a)
                            + 0.15 * abs(a - 6))
                    if best is None or cost < best[0]:
                        best = (cost, s, a, b, c)
    if best is None:
        return [], phase
    _, s, a, b, c = best
    state.update(s=s, a=a)
    cyc = [(c, "tap"), (a, "pull"), (b, "hammer")]
    notes = []
    last_reset = -99
    for x in range(L):
        if reset_on_hits and x in accents and x - last_reset >= 3:
            phase = 0
            last_reset = x
        f, tech = cyc[phase % 3]
        phase += 1
        if mask is not None and x in mask and x not in accents:
            continue
        if tech != "tap" and (not notes or notes[-1]["start"] != bar_start + x - 1 or notes[-1]["fixed"][0] != s):
            tech = "pick"
        elif tech != "tap":
            tech = "hammer" if f > notes[-1]["fixed"][1] else "pull"
        notes.append({"start": bar_start + x, "dur": 3, "pitch": fretboard.OPEN[s] + f, "fixed": (s, f),
                      "preset_tech": [] if tech == "pick" else [tech], "ring": False,
                      "vel": vel + (12 if x in accents else 0)})
    return notes, phase


V8 = {  # doses of the v8 rhythm-guitar idioms; meant to become user controls
    "octave_piece": 0.5,  # share of pieces with one A or A2 section of octave shapes in the rhythm guitar
    "octave_sixteenths": 0.3,  # of those, share in sixteenths instead of eighths
    "dead_notes": 0.35,  # of those, share with dead notes before the accents
    "slide_max": 5,  # largest position change (frets) bridged by a slide
}


def octave_bar(th, root, bar_start, L, accents, grid, dead, state, slide_max=None):
    """Octave shape on the A and G strings (G two frets higher, D string
    muted), struck on `grid`; slides into a new position when it is close;
    optional dead notes on the sixteenth before each accent."""
    x = (th.midi(root) - fretboard.OPEN[1]) % 12
    if x < 2:
        x += 12
    prev = state.get("x")
    smax = V8["slide_max"] if slide_max is None else slide_max
    slide = prev is not None and prev != x and abs(prev - x) <= smax
    state["x"] = x
    ons = sorted(eighths(accents, L)) if grid == 8 else list(range(L))
    notes = []
    for k, o in enumerate(ons):
        nxt = ons[k + 1] if k + 1 < len(ons) else L
        tech = ["slide"] if slide and k == 0 else []
        for s, f in ((1, x), (3, x + 2)):
            notes.append({"start": bar_start + o, "dur": max(1, nxt - o), "pitch": fretboard.OPEN[s] + f,
                          "fixed": (s, f), "preset_tech": tech, "dyad": True, "ring": False,
                          "vel": 86 if o in accents else 72})
    if dead:
        for a in sorted(accents):
            if a > 0 and a - 1 not in ons:
                for s, f in ((1, x), (3, x + 2)):
                    notes.append({"start": bar_start + a - 1, "dur": 1, "pitch": fretboard.OPEN[s] + f,
                                  "fixed": (s, f), "preset_tech": ["dead"], "dyad": True, "ring": False, "vel": 44})
    return notes


def plan_v8(rng, form):
    plan = {}
    secs = [s for s in ("A", "A2") if any(b["sec"] == s and b.get("role") for b in form)]
    if secs and rng.random() < V8["octave_piece"]:
        plan[rng.choice(secs)] = {"grid": 16 if rng.random() < V8["octave_sixteenths"] else 8,
                                  "dead": rng.random() < V8["dead_notes"]}
    out = []
    for b in form:
        b = dict(b)
        if b["sec"] in plan and b.get("role") in ("chords", "chatter"):
            b["role"] = "octave"
            b["oct"] = plan[b["sec"]]
        out.append(b)
    return {"octave": plan}, out


V9 = {  # doses of the v9 comping vocabulary; meant to become user controls
    "push": 0.5,  # pattern has one accent anticipated by a sixteenth
    "offbeat": 0.5,  # pattern has one extra hit on a free eighth
    "drop": 0.3,  # pattern leaves one accent out (a breath)
    "dead_strum": 0.25,  # pattern has a muted strum before an accent
    "vary_bar4": 0.45,  # a square varies its pattern in bar 4
    "vary_next_bar1": 0.1,  # ...or, instead, in bar 1 of the next square
    "half_square": 0.15,  # bar 2 varies too (phrasing in half-squares)
    "variants": {"shift": 0.5, "fill": 0.3, "stop": 0.2},
    "intro": {"pause": 0.35, "build": 0.3, "short": 0.35},
}


def comp_pattern(rng, grouping, L):
    """A comping rhythm built from the meter's grouping and then bent, so that
    pieces in the same meter do not share one strumming pattern."""
    acc = sorted(accents_of(grouping))
    hits = {a: "hit" for a in acc}
    if rng.random() < V9["push"] and len(acc) > 1:
        a = rng.choice(acc[1:])
        if a - 1 not in hits:
            del hits[a]
            hits[a - 1] = "hit"
    if rng.random() < V9["offbeat"]:
        free = [x for x in range(2, L, 2) if not {x - 1, x, x + 1} & set(hits)]
        if free:
            hits[rng.choice(free)] = "hit"
    if rng.random() < V9["drop"] and len(hits) > 3:
        del hits[rng.choice([x for x in hits if x != 0])]
    if rng.random() < V9["dead_strum"]:
        free = [x - 1 for x in hits if x > 0 and x - 1 not in hits]
        if free:
            hits[rng.choice(free)] = "dead"
    return sorted(hits.items())


def vary_pattern(pat, kind, L):
    pat = [tuple(x) for x in pat]
    if kind == "stop":
        return pat[:1]
    out = dict(pat)
    if kind == "shift":
        for p in [p for p, _ in pat][-2:]:
            k = out.pop(p)
            out[p - 1 if p > 0 and p - 1 not in out else p] = k
    elif kind == "fill":
        for x in range(pat[-1][0], L, 2):
            out.setdefault(x, "hit")
    return sorted(out.items())


def plan_v9(rng, intent, form):
    """Comping patterns per section, where they vary (bar 4, bar 2, or bar 1
    of the next square), and how the intro hands over to A."""
    L = METERS[intent["meter"]][0]
    secs = [s for s in ("intro", "A", "A2") if any(b["sec"] == s for b in form)]
    pats = {s: comp_pattern(rng, intent["grouping"], L) for s in secs}
    events = {}
    for s in ("A", "A2"):
        if s not in secs:
            continue
        r = rng.random()
        kind = pick(rng, V9["variants"])
        if r < V9["vary_next_bar1"] and s == "A" and "A2" in secs:
            events[("A2", 0)] = kind
        elif r < V9["vary_next_bar1"] + V9["vary_bar4"]:
            events[(s, 3)] = kind
        if rng.random() < V9["half_square"]:
            events[(s, 1)] = "shift"
    intro = pick(rng, V9["intro"])
    out, pos = [], {}
    n_intro = sum(b["sec"] == "intro" for b in form)
    for b in form:
        b = dict(b)
        i = pos.get(b["sec"], 0)
        pos[b["sec"]] = i + 1
        if b["sec"] == "intro" and intro == "short" and i < n_intro - 1:
            continue
        if b["sec"] in pats:
            b["comp"] = pats[b["sec"]]
            if (b["sec"], i) in events:
                b["comp_var"] = events[(b["sec"], i)]
        if b["sec"] == "intro" and i == n_intro - 1 and intro in ("pause", "build"):
            b["handover"] = intro
        out.append(b)
    return {"patterns": {s: pats[s] for s in secs}, "events": {f"{s}:{i}": k for (s, i), k in events.items()},
            "intro": intro}, out


def comp_strike(th, root, bar_start, L, g, pattern, vel, avoid_cluster):
    lo, hi = (40, 64) if g == "g2" else (52, 76)
    v = fretboard.voice_chord(th.chord_pcs(root, (0, 2, 4, 1)), th.midi(root) % 12, lo, hi, None, 4,
                              avoid_cluster=avoid_cluster)
    notes = []
    for k, (p, kind) in enumerate(pattern):
        nxt = pattern[k + 1][0] if k + 1 < len(pattern) else L
        for s, f, pitch in v or []:
            if kind == "dead":
                notes.append({"start": bar_start + p, "dur": 1, "pitch": pitch, "vel": 40, "fixed": (s, f),
                              "preset_tech": ["dead"], "ring": False})
            else:
                notes.append({"start": bar_start + p, "dur": max(1, nxt - p), "pitch": pitch,
                              "vel": vel + (8 if p == 0 else 0), "fixed": (s, f), "ring": True})
    return notes


def leap_cycle(cycle):
    """Reorder an arpeggio cycle so consecutive notes skip at least one ladder
    step where possible (string skips instead of a stepwise ladder)."""
    def steps(cyc):
        return sum(abs(a - b) <= 1 for a, b in zip(cyc, cyc[1:] + cyc[:1]))

    best = list(cycle)
    for first in range(len(cycle)):
        rest = list(cycle)
        cur = rest.pop(first)
        out = [cur]
        while rest:
            far = [c for c in rest if abs(c - cur) >= 2]
            cur = max(far or rest, key=lambda c: (abs(c - cur) >= 2, -abs(abs(c - cur) - 3)))
            rest.remove(cur)
            out.append(cur)
        if steps(out) < steps(best):
            best = out
    return best


def chord_hits(th, root, bar_start, L, g, accents, vel, low=False, avoid_cluster=False):
    """Sparse texture: one ringing voicing struck on each accent."""
    lo, hi = ((40, 64) if low or g == "g2" else (52, 76))
    v = fretboard.voice_chord(th.chord_pcs(root, (0, 2, 4, 1)), th.midi(root) % 12, lo, hi, None, 4,
                              avoid_cluster=avoid_cluster)
    hits = sorted(accents)
    notes = []
    for i, a in enumerate(hits):
        end = hits[i + 1] if i + 1 < len(hits) else L
        for s, f, p in v or []:
            notes.append({"start": bar_start + a, "dur": end - a, "pitch": p, "vel": vel + (8 if a == 0 else 0),
                          "fixed": (s, f), "ring": True})
    return notes


def doubled(th, notes, root, kind, bar_start, accents):
    """The second guitar plays the lead's rhythm: unison, an octave below, a
    diatonic third below, or a low root-fifth dyad."""
    out = []
    for n in notes:
        base = {"start": n["start"], "dur": n["dur"], "vel": n["vel"] - 8}
        if kind == "unison":
            out.append(dict(base, pitch=n["pitch"]))
        elif kind == "octave":
            out.append(dict(base, pitch=n["pitch"] - 12 if n["pitch"] - 12 >= 40 else n["pitch"]))
        elif kind == "harm3":
            d = th.deg_near(n["pitch"]) - 2
            if not stable(d, root) and int(n["start"] - bar_start) in accents:
                d -= 1
            p = th.midi(d)
            while p - n["pitch"] > -3:
                p -= 12
            out.append(dict(base, pitch=p if p >= 40 else p + 12))
        else:
            low = th.midi(root)
            while low > 52:
                low -= 12
            while low < 40:
                low += 12
            out.append(dict(base, pitch=low))
            out.append(dict(base, pitch=low + 7, chordtone=True))
    return out


def minor_ninth(a, b):
    return abs(a - b) % 12 == 1 and a != b


def resolve_minor_ninths(parts, lead, other):
    """The corpus has many major sevenths between the guitars but almost no
    minor ninths. An accompaniment note sounding a minor ninth or semitone
    against a lead note is cut short before it, or dropped when struck together
    with it or under it. The lead is never changed, so a riff stays verbatim."""
    resolved = []
    keep = []
    for m in sorted(parts[other], key=lambda n: n["start"]):
        end = m["start"] + m["dur"]
        hits = [n for n in parts[lead] if m["start"] <= n["start"] < end and minor_ninth(n["pitch"], m["pitch"])]
        under = [n for n in parts[lead] if n["start"] < m["start"] < n["start"] + n["dur"]
                 and minor_ninth(n["pitch"], m["pitch"])]
        if under or any(n["start"] == m["start"] for n in hits):
            resolved.append(m["start"])
            continue
        if hits:
            m["dur"] = min(n["start"] for n in hits) - m["start"]
            resolved.append(m["start"])
        keep.append(m)
    gone = {n["start"] for n in parts[other] if n.get("dyad") and n not in keep}
    keep = [n for n in keep if not (n.get("dyad") and n["start"] in gone)]
    for start in {n["start"] for n in keep if n.get("dyad")}:
        pair = [n for n in keep if n.get("dyad") and n["start"] == start]
        for n in pair:
            n["dur"] = min(x["dur"] for x in pair)
    prev, by_string = None, {}
    for n in keep:
        if n.get("chordtone") or (n.get("fixed") and "preset_tech" not in n):
            continue
        p = by_string.get(n["string"]) if n.get("dyad") else prev
        legato = {"hammer", "pull", "slide"} & set(n["tech"])
        if legato and (p is None or p["string"] != n["string"] or p["fret"] == n["fret"]
                       or ("hammer" in legato and n["fret"] < p["fret"])
                       or ("pull" in legato and n["fret"] > p["fret"])):
            n["tech"] = [x for x in n["tech"] if x not in legato]
        prev = n
        by_string[n["string"]] = n
    parts[other] = keep
    return resolved


def apply_rests_and_pushes(parts, lead, other, form, spans, plan):
    """Rests at square boundaries and rhythm-guitar anticipations (the last
    hits of a bar an eighth early), as listed in plan."""
    def bars_of(sec):
        return [i for i, b in enumerate(form) if b["sec"] == sec]
    mute = []
    for kind in plan["rests"]:
        if kind == "rhythm_out_A2" and bars_of("A2"):
            mute += [(i, other, 0) for i in bars_of("A2")[:2]]
        elif kind == "lead_late_B" and bars_of("B"):
            mute.append((bars_of("B")[0], lead, 0))
        elif kind == "drop_before_A3" and bars_of("A3"):
            i = bars_of("A3")[0] - 1
            if i >= 0 and form[i]["sec"] != "intro":
                mute += [(i, lead, 1), (i, other, 1)]
    for i, g, after in mute:
        s, e = spans[i]
        parts[g] = [n for n in parts[g] if not (s + after <= n["start"] < e)]
    for i in plan["pushes"]:
        s, e = spans[i]
        ons = sorted({n["start"] for n in parts[other] if s <= n["start"] < e})
        tail = [o for k, o in enumerate(ons) if k >= len(ons) - 3 and o - 2 > (ons[k - 1] if k else s - 1)]
        for n in parts[other]:
            if n["start"] in tail:
                n["start"] -= 2
                n["dur"] = max(1, min(n["dur"], 2))


def plan_v6(rng, form):
    kinds = ["rhythm_out_A2", "lead_late_B", "drop_before_A3"]
    rests = [k for k in kinds if rng.random() < 0.4] or [rng.choice(kinds)]
    pushes = []
    for sec in ("A", "A2"):
        bars = [i for i, b in enumerate(form) if b["sec"] == sec]
        if len(bars) >= 4 and rng.random() < 0.45:
            i = bars[rng.choice([1, 3])]
            if form[i]["prog"] != "shift" and form[i].get("role") == "chords":
                pushes.append(i)
    return {"rests": rests, "pushes": pushes, "end": pick(rng, ENDINGS_V6)}


def eighths(accents, L):
    """Eighth-note grid re-anchored on every accent, so odd groupings keep their downbeats."""
    out, last = set(), -2
    for x in range(L):
        if x in accents or x - last >= 2:
            out.add(x)
            last = x
    return out


def accents_of(grouping):
    out, p = set(), 0
    for g in grouping:
        out.add(p)
        p += g
    return out


def _studio_rng(seed, layer, section, salt):
    return random.Random(f"{seed}:{layer}:{section}:{salt}")


def _det_unit(key):
    h = hashlib.sha256(key.encode("utf-8")).digest()
    return (int.from_bytes(h[:7], "big") >> 3) / (1 << 53)


def _studio_octave_spec(seed, sec, comp_doses_map):
    doses = comp_doses_map.get(sec) or V8
    rng = random.Random(f"{seed}:octave_spec:{sec}")
    return {
        "grid": 16 if rng.random() < float(doses.get("octave_sixteenths", V8["octave_sixteenths"])) else 8,
        "dead": rng.random() < float(doses.get("dead_notes", V8["dead_notes"])),
        "slide_max": int(doses.get("slide_max", V8["slide_max"])),
    }


def _strip_leading_legato(notes, s0, s1):
    in_sec = sorted((n for n in notes if s0 <= n["start"] < s1), key=lambda n: (n["start"], n["pitch"]))
    for n in in_sec:
        if not n.get("chordtone") and not n.get("dyad") and not (n.get("fixed") and "preset_tech" not in n):
            n["tech"] = [x for x in n.get("tech", []) if x not in ("hammer", "pull", "slide")]
            break
    seen_dyad_str = set()
    for n in in_sec:
        if n.get("dyad") and n["string"] not in seen_dyad_str:
            seen_dyad_str.add(n["string"])
            n["tech"] = [x for x in n.get("tech", []) if x != "slide"]


def _apply_square_repeat(parts, chords, meters, total16, repeat, form=None):
    if repeat <= 1:
        return total16
    for target_sec in ("A", "A2"):
        idxs = [i for i, c in enumerate(chords) if c["sec"] == target_sec]
        if not idxs:
            continue
        first_i, last_i = idxs[0], idxs[-1]
        s0 = chords[first_i]["start"]
        s1 = chords[last_i + 1]["start"] if last_i + 1 < len(chords) else total16
        sec_len = s1 - s0
        extra_len = sec_len * (repeat - 1)
        for g in ("g1", "g2"):
            _strip_leading_legato(parts[g], s0, s1)
            sec_notes = []
            for n in parts[g]:
                if s0 <= n["start"] < s1:
                    n["dur"] = min(n["dur"], max(1, s1 - n["start"]))
                    sec_notes.append(n)
                elif n["start"] >= s1:
                    n["start"] += extra_len
            for r in range(1, repeat):
                shift = sec_len * r
                parts[g].extend(dict(copy.deepcopy(n), start=n["start"] + shift) for n in sec_notes)
            parts[g].sort(key=lambda n: (n["start"], n["pitch"]))
        for c in chords[last_i + 1:]:
            c["start"] += extra_len
        block = [dict(c) for c in chords[first_i:last_i + 1]]
        inserted_chords = []
        for r in range(1, repeat):
            shift = sec_len * r
            inserted_chords.extend(dict(c, start=c["start"] + shift) for c in block)
        chords[last_i + 1:last_i + 1] = inserted_chords
        for i, c in enumerate(chords):
            c["bar"] = i
        if form is not None:
            fblock = [copy.deepcopy(b) for b in form[first_i:last_i + 1]]
            inserted_form = []
            for r in range(1, repeat):
                inserted_form.extend(copy.deepcopy(b) for b in fblock)
            form[last_i + 1:last_i + 1] = inserted_form
        for k, (tick, num, den) in enumerate(meters):
            if tick >= s1 * 120:
                meters[k] = (tick + extra_len * 120, num, den)
        total16 += extra_len
    return total16


def _apply_mute_share(parts, other, chords, total16, seed, mute_share):
    if mute_share <= 0.0:
        return
    spans = [
        (c["start"], chords[i + 1]["start"] if i + 1 < len(chords) else total16)
        for i, c in enumerate(chords)
        if c["sec"] in ("A", "A2", "A3", "B")
    ]
    if not spans:
        return
    by_onset = {}
    for n in parts[other]:
        st = n["start"]
        if any(s0 <= st < s1 for s0, s1 in spans):
            by_onset.setdefault(st, []).append(n)
    for onset, group in sorted(by_onset.items()):
        if all("dead" in n.get("tech", []) for n in group):
            continue
        if _det_unit(f"{seed}:mute:{onset}") < mute_share:
            for n in group:
                if "dead" in n.get("tech", []):
                    continue
                n["tech"] = ["dead"]
                n["dur"] = 1
                n["vel"] = 40
                n["ring"] = False


def _studio_cycle(rng, v8):
    cycle = rng.choice([3, 4, 5, 5, 6, 7])
    ladder_pick = list(range(cycle))
    for _ in range(50):
        cand = [0] + [rng.randrange(0, 7) for _ in range(cycle - 1)]
        colorful = any(i in (3, 4) for i in cand)
        no_rep = all(a != b for a, b in zip(cand, cand[1:] + cand[:1]))
        if colorful and no_rep and len(set(cand)) >= 3:
            ladder_pick = cand
            break
    return leap_cycle(ladder_pick) if v8 else ladder_pick


def _studio_tempo(seed, base_tempo, salt):
    tempo = base_tempo
    for k in range(1, salt + 1):
        rng = _studio_rng(seed, "tempo", "all", k)
        lo = max(60, int(round(tempo * 0.8)))
        hi = min(220, int(round(tempo * 1.1)))
        val = max(lo, min(hi, int(round(tempo * rng.uniform(0.8, 1.1)))))
        if val == tempo and hi > lo:
            step = rng.choice([-1, 1])
            val = val + step if lo <= val + step <= hi else val - step
        tempo = val
    return tempo


def _studio_hook_variants(recipe):
    """Replay the recipe's motif stream: ({True: hook with trailing rest, False: hook without},
    whether the recipe's own hook keeps the rest), or None when the stream cannot be matched."""
    it = recipe["intent"]
    seed = recipe["seed"]
    want_intent = json.dumps(it, sort_keys=True)
    want_hook = json.dumps(recipe["motifs"]["hook"], sort_keys=True)
    for flags in itertools.product((False, True), repeat=4):
        given = {k: (it[f] if on else None) for (k, f), on in zip(
            (("meter", "meter"), ("mode", "mode"), ("key", "tonic"), ("tempo", "tempo")), flags)}
        args = argparse.Namespace(**given)
        rng = random.Random(seed)
        intent = plan_intent(rng, args)
        if "lead" in recipe and not args.mode:
            intent["mode"] = pick(random.Random(seed * 86028121 + 13), MODES_V7)
        if json.dumps(intent, sort_keys=True) != want_intent:
            continue
        plan_harmony(rng, Theory(TONICS[intent["tonic"]], intent["mode"]), intent)
        state = rng.getstate()
        hooks = {}
        for tail in (True, False):
            r = random.Random()
            r.setstate(state)
            hooks[tail] = plan_motifs(r, intent, tail_rest=tail)["hook"]
        for tail in (True, False):
            if json.dumps(hooks[tail], sort_keys=True) == want_hook:
                return hooks, tail
    return None


def studio_base_trailing_rest(recipe):
    variants = _studio_hook_variants(recipe)
    if variants is not None:
        return variants[1]
    return not str(recipe.get("generator", "")).endswith("-notail")


def _studio_lead_rhythm(seed, intent, salt, tail_rest):
    rng = _studio_rng(seed, "lead_rhythm", "all", salt)
    m = plan_motifs(rng, intent, tail_rest=tail_rest)
    lp = plan_lead(rng, m["hook"], intent)
    return m["hook"], lp


def _studio_intro(seed, base_bars, base_dens, salt):
    rng = _studio_rng(seed, "intro", "intro", salt)
    for _ in range(8):
        n_bars = rng.choice([1, 2, 4])
        dens = rng.choice([1, 2, 3])
        tex = rng.choice(["arp", "tap"]) if dens == 3 else "arp"
        if salt != 1 or n_bars != base_bars or dens != base_dens:
            return n_bars, dens, tex
    return n_bars, dens, tex


def _studio_comp(seed, sec, salt, grouping, L, doses, base_sec_bars):
    rng = _studio_rng(seed, "comp", sec, salt)
    pool = [
        ["chords", "chords", "chatter"],
        ["chatter", "chords", "chatter"],
        ["chords", "chatter", "chords"],
        ["chords", "chords", "chords"],
        ["chatter", "chatter", "chords"],
    ]
    base_roles = [b.get("role") for b in base_sec_bars if b.get("role")]
    base_oct = base_sec_bars[0].get("oct") if base_roles and base_roles[0] == "octave" else None
    base_acc_pat = [(a, "hit") for a in sorted(accents_of(grouping))]

    def draw():
        if rng.random() < doses["octave_piece"]:
            return {
                "kind": "octave",
                "oct": {
                    "grid": 16 if rng.random() < doses["octave_sixteenths"] else 8,
                    "dead": rng.random() < doses["dead_notes"],
                    "slide_max": int(doses["slide_max"]),
                },
            }
        return {"kind": "roles", "roles": rng.choice(pool), "comp": comp_pattern(rng, grouping, L)}

    plan = draw()
    for _ in range(8):
        if salt != 1:
            break
        if plan["kind"] == "octave":
            same = base_oct is not None and (
                plan["oct"]["grid"] == base_oct.get("grid") and plan["oct"]["dead"] == base_oct.get("dead")
            )
        else:
            same = plan["comp"] == base_acc_pat
        if not same:
            break
        plan = draw()
    return plan


def _studio_texture(seed, sec, salt, base_arp, v8):
    rng = _studio_rng(seed, "texture", sec, salt)
    base_tex = base_arp.get("texture")
    base_key = (base_tex, list(base_arp.get("cycle", [])))
    for _ in range(8):
        tex = rng.choice(["arp", "arp", "pedal", "tap"])
        cyc = _studio_cycle(rng, v8)
        same_tap = (tex == "tap" and base_tex == "tap")
        if salt != 1 or (not same_tap and (tex, cyc) != base_key):
            return dict(base_arp, texture=tex, cycle=cyc)
    return dict(base_arp, texture=tex, cycle=cyc)


def _studio_b(seed, salt, b_meter, b_grouping, base_chug, base_arp, v8, base_vars):
    rng = _studio_rng(seed, "b", "B", salt)
    bL = METERS[b_meter][0]
    b_acc = sorted(accents_of(b_grouping))
    mid = max(a for a in b_acc if a <= bL // 2)
    last_a = b_acc[-1]
    half_opts = sorted({
        tuple(sorted(s))
        for s in ({0, mid}, {0}, {0, max(1, mid - 1)}, {0, last_a}, {0, mid, last_a})
    })
    b_vars = ["chug", "half", "clean"]

    def draw():
        v1 = rng.choice(b_vars)
        v2 = rng.choice([v for v in b_vars if v != v1])
        chug = sorted(
            {a + 1 for a in b_acc if rng.random() < 0.5 and a + 1 < bL}
            | {x for x in range(bL) if x % 2 == 0 and rng.random() < 0.25}
        ) or [0]
        half_hits = set(rng.choice(half_opts))
        clean_arp = dict(base_arp, texture="arp", cycle=_studio_cycle(rng, v8))
        return {"vars": [v1, v2], "chug": chug, "half_hits": half_hits, "arp": clean_arp}

    plan = draw()
    for _ in range(8):
        if salt != 1:
            break
        v1 = plan["vars"][0]
        same_v = v1 == (base_vars[0] if base_vars else "chug")
        if not same_v:
            break
        if v1 == "chug" and plan["chug"] != (base_chug or [0]):
            break
        if v1 == "half" and plan["half_hits"] != {0, mid}:
            break
        if v1 == "clean" and plan["arp"]["cycle"] != base_arp["cycle"]:
            break
        plan = draw()
    return plan


def _studio_ending(seed, salt, base_end):
    base = base_end or "held"
    _studio_rng(seed, "ending", "end", salt)
    others = [e for e in ("held", "stop", "busy") if e != base]
    if random.Random(f"{seed}:ending:end:order").random() < 0.5:
        others.reverse()
    order = others + [base]
    return order[(salt - 1) % 3]


_STUDIO_BASELINES = {}


def _studio_baseline(recipe):
    plain = {k: v for k, v in recipe.items() if k != "studio"}
    key = json.dumps(plain, sort_keys=True, default=list)
    trace = _STUDIO_BASELINES.get(key)
    if trace is None:
        trace = {}
        compose(copy.deepcopy(plain), _trace=trace)
        if len(_STUDIO_BASELINES) >= 64:
            _STUDIO_BASELINES.clear()
        _STUDIO_BASELINES[key] = trace
    return trace


def _studio_canon(notes, s0, s1):
    return sorted(
        (n["start"] - s0, json.dumps({k: v for k, v in n.items() if k != "start"}, sort_keys=True, default=list))
        for n in notes
        if s0 <= n["start"] < s1
    )


def _prepare_studio(recipe, arp, v8):
    studio = recipe["studio"]
    salts = studio.get("salts") or {}
    opts = studio.get("options") or {}
    comp_doses_map = studio.get("comp_doses") or {}
    it = recipe["intent"]
    seed = recipe["seed"]

    base = _studio_baseline(recipe)
    variants = _studio_hook_variants(recipe)
    base_tail = variants[1] if variants else not str(recipe.get("generator", "")).endswith("-notail")
    tail_rest = bool(opts.get("trailing_rest", base_tail))
    lead_salt = int(salts.get("lead_rhythm@all", 0))
    hook, lead_plan = recipe["motifs"]["hook"], recipe.get("lead")
    if lead_salt > 0:
        hook, lead_plan = _studio_lead_rhythm(seed, it, lead_salt, tail_rest)
    elif tail_rest != base_tail and variants:
        hook = variants[0][tail_rest]
        if "lead" in recipe:
            lead_plan = plan_lead(random.Random(seed * 49979693 + 21), hook, it)

    form = [dict(b, _orig_bar_i=idx) for idx, b in enumerate(recipe["form"])]

    intro_salt = int(salts.get("intro@intro", 0))
    if intro_salt > 0:
        orig_intro = [b for b in form if b["sec"] == "intro"]
        base_bars = len(orig_intro)
        base_dens = orig_intro[0].get("dens", 3) if orig_intro else 3
        n_bars, dens, intro_tex = _studio_intro(seed, base_bars, base_dens, intro_salt)
        non_intro = [b for b in form if b["sec"] != "intro"]
        harmony_a = recipe["harmony"]["A"]
        new_intro = [
            {
                "sec": "intro",
                "chord": harmony_a[i % len(harmony_a)],
                "prog": "intro",
                "dens": dens,
                "_intro_tex": intro_tex,
                "_orig_bar_i": i,
            }
            for i in range(n_bars)
        ]
        form = new_intro + non_intro

    intro_bars_opt = opts.get("intro_bars", "auto")
    if intro_bars_opt != "auto":
        n_bars = int(intro_bars_opt)
        orig_intro = [b for b in form if b["sec"] == "intro"]
        non_intro = [b for b in form if b["sec"] != "intro"]
        harmony_a = recipe["harmony"]["A"]
        template = {k: v for k, v in orig_intro[0].items() if k != "handover"} if orig_intro else {
            "sec": "intro",
            "prog": "intro",
            "dens": 2,
        }
        last_handover = orig_intro[-1].get("handover") if orig_intro else None
        new_intro = []
        for i in range(n_bars):
            bar = dict(template, sec="intro", chord=harmony_a[i % len(harmony_a)], prog="intro", _orig_bar_i=i)
            if last_handover and i == n_bars - 1:
                bar["handover"] = last_handover
            new_intro.append(bar)
        form = new_intro + non_intro

    L = METERS[it["meter"]][0]
    for sec in ("A", "A2"):
        csalt = int(salts.get(f"comp@{sec}", 0))
        if csalt > 0:
            sec_bars = [b for b in form if b["sec"] == sec]
            if sec_bars:
                doses = {
                    "octave_piece": float(comp_doses_map.get(sec, {}).get("octave_piece", opts.get("octave_piece", V8["octave_piece"]))),
                    "octave_sixteenths": float(comp_doses_map.get(sec, {}).get("octave_sixteenths", opts.get("octave_sixteenths", V8["octave_sixteenths"]))),
                    "dead_notes": float(comp_doses_map.get(sec, {}).get("dead_notes", opts.get("dead_notes", V8["dead_notes"]))),
                    "slide_max": int(comp_doses_map.get(sec, {}).get("slide_max", opts.get("slide_max", V8["slide_max"]))),
                }
                cplan = _studio_comp(seed, sec, csalt, it["grouping"], L, doses, sec_bars)
                ri = 0
                for b in form:
                    if b["sec"] == sec and b.get("role") in ("chords", "chatter", "octave"):
                        if cplan["kind"] == "octave":
                            b["role"] = "octave"
                            b["oct"] = dict(cplan["oct"])
                            b.pop("comp", None)
                            b.pop("_fallback_comp", None)
                        else:
                            b.pop("oct", None)
                            b["role"] = cplan["roles"][ri % len(cplan["roles"])]
                            b["comp"] = list(cplan["comp"])
                            b["_fallback_comp"] = list(cplan["comp"])
                        ri += 1

    raw_oct_share = opts.get("octave_share", None)
    octave_share = None if raw_oct_share is None else max(0.0, min(1.0, float(raw_oct_share)))
    if octave_share is not None:
        for sec in ("A", "A2"):
            comping_bars = [
                b for b in form
                if b["sec"] == sec and b.get("role") in ("chords", "chatter", "octave") and not b.get("tex")
            ]
            if not comping_bars:
                continue
            if "texture" in recipe and sec in recipe["texture"].get("role", {}):
                fallback_roles = [recipe["texture"]["role"][sec]]
            elif "repeat" in recipe and recipe["repeat"].get("roles"):
                fallback_roles = list(recipe["repeat"]["roles"])
            else:
                fallback_roles = ["chords"]
            base_oct = next((b.get("oct") for b in comping_bars if b.get("oct")), None)
            oct_spec = dict(base_oct) if base_oct is not None else _studio_octave_spec(seed, sec, comp_doses_map)
            for ri, b in enumerate(comping_bars):
                if _det_unit(f"{seed}:octave:{b['_orig_bar_i']}") < octave_share:
                    b["role"] = "octave"
                    b["oct"] = dict(b.get("oct") or oct_spec)
                    b.pop("comp", None)
                    b.pop("_fallback_comp", None)
                elif b.get("role") == "octave":
                    b.pop("oct", None)
                    b["role"] = fallback_roles[ri % len(fallback_roles)]

    sec_arp = {}
    for sec in ("A", "A2"):
        tsalt = int(salts.get(f"texture@{sec}", 0))
        if tsalt > 0:
            sec_arp[sec] = _studio_texture(seed, sec, tsalt, arp, v8)

    b_plan = None
    b_salt = int(salts.get("b@B", 0))
    if b_salt > 0:
        b_bars = [b for b in form if b["sec"] == "B"]
        base_vars = []
        for b in b_bars:
            v = b.get("var", "chug")
            if not base_vars or base_vars[-1] != v:
                base_vars.append(v)
        b_plan = _studio_b(
            seed, b_salt, it["b_meter"], it["b_grouping"],
            recipe["motifs"].get("chug"), arp, v8, base_vars,
        )
        n_b = len(b_bars)
        split = recipe.get("arrange", {}).get("template") == "double_b"
        for bi, b in enumerate(b_bars):
            b["var"] = b_plan["vars"][0] if (not split or bi < n_b // 2) else b_plan["vars"][1]

    end_salt = int(salts.get("ending@end", 0))
    if end_salt > 0:
        base_end = next((b.get("end") for b in form if b["sec"] == "end"), None)
        end_kind = _studio_ending(seed, end_salt, base_end)
        for b in form:
            if b["sec"] == "end":
                b["end"] = end_kind

    end_riff = None
    if "ending@end" in (studio.get("locks") or []):
        end_riff = (studio.get("locked_riffs") or {}).get("ending@end")
        if end_riff is None:
            end_riff = base["riff"].get("A3") or base["riff"].get("A") or []

    base_tempo = recipe.get("arrange", {}).get("tempo", it["tempo"])
    tempo_salt = int(salts.get("tempo@all", 0))
    tempo = _studio_tempo(seed, base_tempo, tempo_salt) if tempo_salt > 0 else base_tempo

    mute_share = max(0.0, min(1.0, float(opts.get("mute_share", 0.0))))
    square_repeat = int(opts.get("square_repeat", 1))

    return {
        "hook": hook,
        "lead_plan": lead_plan,
        "form": form,
        "sec_arp": sec_arp,
        "b_plan": b_plan,
        "b_lead": opts.get("b_lead", "augmented"),
        "base_tempo": base_tempo,
        "tempo": tempo,
        "base": base,
        "end_riff": end_riff,
        "mute_share": mute_share,
        "square_repeat": square_repeat,
    }


def compose(recipe, _trace=None):
    rng = random.Random(recipe["seed"] * 7919 + 17)
    it = recipe["intent"]
    th = Theory(TONICS[it["tonic"]], it["mode"])
    hook = recipe["motifs"]["hook"]
    arp = dict(recipe["motifs"]["arp"], texture=recipe["motifs"].get("texture"))
    if recipe.get("v8"):
        arp["cycle"] = leap_cycle(arp["cycle"])
    oct_state, entered = {}, set()
    lead, other = (("g1", "g2") if it["lead"] == "g1" else ("g2", "g1"))
    lead_plan = recipe.get("lead")
    v8 = bool(recipe.get("v8"))
    studio = recipe.get("studio")
    if _trace is not None:
        _trace["dense"] = {}
        _trace["riff"] = {}
        _trace["states"] = {}
    st_cfg = _prepare_studio(recipe, arp, v8) if studio is not None else None
    rec_hook, rec_lead_plan = hook, lead_plan
    if st_cfg is not None:
        hook = st_cfg["hook"]
        lead_plan = st_cfg["lead_plan"]
    strike = functools.partial(chord_hits, avoid_cluster=v8)
    rng_lead = dict(LEAD_RANGE_V7) if lead_plan else {"g1": (64, 86), "g2": (52, 74)}
    col = bool(lead_plan)
    rng_arp = {"g1": (59, 86), "g2": (40, 71)}
    parts = {"g1": [], "g2": []}
    chords = []
    meters = []
    t = 0
    phase = 0
    anchor_tone = hook["anchor"]
    sec_anchor = {}
    shape = {}
    tap_state = {}
    riff_cache = {}
    lead_center = (rng_lead[lead][0] + rng_lead[lead][1]) // 2 - 3

    def phrase_anchor(sec, root, lift, tone=None):
        if sec not in sec_anchor:
            cands = [root + (anchor_tone if tone is None else tone) + 7 * k for k in range(-3, 6)]
            target = th.deg_near(lead_center) + lift
            sec_anchor[sec] = min(cands, key=lambda d: abs(d - target))
        return lambda _root: sec_anchor[sec]

    base_ev = None
    last_meter = None
    form = st_cfg["form"] if st_cfg is not None else recipe["form"]
    sec_spans = []
    prev_sec = None
    for bar_i, bar in enumerate(form):
        if bar["sec"] != prev_sec and (st_cfg is not None or _trace is not None):
            prev_sec = bar["sec"]
            if st_cfg is not None:
                snap = st_cfg["base"]["states"].get(len(sec_spans))
                shape, tap_state, oct_state, phase = copy.deepcopy(snap) if snap else ({}, {}, {}, phase)
                sec_spans.append([bar["sec"], t, t])
            else:
                _trace.setdefault("states", {})[len(sec_spans)] = copy.deepcopy((shape, tap_state, oct_state, phase))
                sec_spans.append(None)
        meter = it["b_meter"] if bar["sec"] == "B" else it["meter"]
        grouping = it["b_grouping"] if bar["sec"] == "B" else it["grouping"]
        L, num, den, _ = METERS[meter]
        if meter != last_meter:
            meters.append((t * 120, num, den))
            last_meter = meter
        acc = accents_of(grouping)
        skel = recipe["motifs"].get("skeleton")
        if skel:
            acc = set(skel["B" if bar["sec"] == "B" else "A"])
        root = bar["chord"]
        chords.append({"bar": bar_i, "start": t, "root_deg": root, "sec": bar["sec"]})
        hook_L = METERS[it["meter"]][0]
        if base_ev is None:
            base_ev = motif_events(hook, hook_L)
            if lead_plan:
                base_ev = shape_lead(base_ev, lead_plan)
            call_ev = base_ev
            if hook is not rec_hook or lead_plan is not rec_lead_plan:
                call_ev = motif_events(rec_hook, hook_L)
                if rec_lead_plan:
                    call_ev = shape_lead(call_ev, rec_lead_plan)
        lr = rng_lead[lead]
        pr = bar["prog"]
        orig_i = bar.get("_orig_bar_i", bar_i)
        brng = random.Random(recipe["seed"] * 31 + orig_i)
        dens = bar.get("dens", 3)
        if "repeat" in recipe:
            phase = 0
        comp = bar.get("comp")
        if comp is not None and bar.get("comp_var"):
            comp = vary_pattern(comp, bar["comp_var"], L)
        bar_arp = st_cfg["sec_arp"].get(bar["sec"], arp) if st_cfg is not None else arp
        if pr == "intro" and comp is not None:
            if bar.get("handover") == "build":
                ramp = [(x, "hit") for x in range(0, L, 2)]
                notes = comp_strike(th, root, t, L, other, ramp, 56, v8)
                for n in notes:
                    n["vel"] += int(30 * (n["start"] - t) / L)
                parts[other] += notes
            else:
                n, phase = arp_bar(th, bar_arp, root, t, L, *rng_arp[other], phase, 1, 66, acc, None, shape,
                                   {p for p, k in comp if k == "hit"})
                if bar.get("handover") == "pause":
                    n = [x for x in n if x["start"] < t + L // 2]
                    for x in n:
                        x["dur"] = min(x["dur"], t + L // 2 - x["start"])
                parts[other] += n
        elif pr == "intro" and dens < 3:
            if dens == 1:
                parts[other] += strike(th, root, t, L, other, acc, 70)
            else:
                n, phase = arp_bar(th, bar_arp, root, t, L, *rng_arp[other], phase, 2, 64, acc, None, shape,
                                   eighths(acc, L))
                parts[other] += n
        elif pr == "intro":
            intro_tap = bar.get("_intro_tex") == "tap" if bar.get("_intro_tex") else (bar_arp.get("texture") == "tap")
            if intro_tap:
                n, phase = tap_bar(th, root, t, L, phase, 66, acc, None, tap_state, bool(skel))
            else:
                n, phase = arp_bar(th, bar_arp, root, t, L, *rng_arp[other], phase, 1, 66, acc, None, shape)
            parts[other] += n
        elif bar["sec"] in ("A", "A2", "A3"):
            name = pr.replace("_harm", "").replace("_open", "").replace("_final", "")
            ev = base_ev
            if name == "riff_var":
                ev = op("seq", ev, L, brng)
            elif name == "shift":
                ev = op("displace", ev, L, brng)
            elif name == "stop":
                ev = [e for e in ev if e["on"] < L // 2]
            elif name in ("stmt_orn", "seq_orn"):
                ev = op("orn", ev, L, brng)
                if name == "seq_orn":
                    ev = op("seq", ev, L, brng)
            elif name in ("invert", "retro", "frag", "call", "displace", "seq"):
                ev = op("seq" if skel and name == "displace" else name, ev, L, brng)
            elif name == "cad":
                ev = op("cad_run" if (bar["sec"] == "A2") != hook.get("run_first", False) else "cad", ev, L, brng)
            lift = 7 if bar["sec"] == "A3" else 0
            fn = phrase_anchor(bar["sec"], root, lift)
            if skel and name not in ("call",):
                ev = conform(ev, sorted(acc), L)
            if hook.get("push") and not skel and name in ("stmt", "seq", "stmt_orn", "seq_orn") and ev and ev[0]["on"] == 0:
                ev = [dict(ev[0], on=-1, dur=ev[0]["dur"] + 1)] + ev[1:]
                parts[lead] = [n for n in parts[lead] if n["start"] != t - 1]
            if name == "riff":
                if bar["sec"] not in riff_cache:
                    riff_cache[bar["sec"]] = place_line(th, ev, root, fn, 0, acc, lr[0] + (5 if bar["sec"] == "A3" else 0),
                                                        lr[1], 90, colors=col)
                notes = [dict(n, start=n["start"] + t) for n in riff_cache[bar["sec"]]]
                if v8 and bar["sec"] == "A" and "A" not in entered and len(notes) >= 0.6 * L:
                    notes = [n for n in notes if n["start"] - t < L // 2]
                entered.add(bar["sec"])
            else:
                notes = place_line(th, ev, root, fn, t, acc, lr[0] + (5 if bar["sec"] == "A3" else 0), lr[1], 90,
                                   colors=col)
            if pr == "cad_final":
                for nn in notes[-1:]:
                    nn["dur"] = L - (nn["start"] - t)
            parts[lead] += notes
            mask = {int(n["start"] - t) for n in notes}
            dense = len(mask) >= L / 2
            if _trace is not None:
                _trace.setdefault("dense", {})[orig_i] = dense
            if st_cfg is not None:
                dense = st_cfg["base"]["dense"].get(orig_i, dense)
            tex = bar.get("tex")
            if tex == "alone":
                pass
            elif tex:
                parts[other] += doubled(th, notes, root, tex, t, acc)
            elif pr == "stop":
                parts[other] += [dict(n, dur=2) for n in strike(th, root, t, L, other, {0}, 96)]
            elif pr.endswith("_harm"):
                hn = []
                for nn in notes:
                    d = th.deg_near(nn["pitch"]) - 2
                    if not stable(d, root) and int(nn["start"] - t) in acc:
                        d -= 1
                    hn.append(dict(nn, pitch=th.midi(d), vel=nn["vel"] - 6))
                lo2, hi2 = rng_lead[other][0] - 6, rng_lead[other][1] + 6
                if hn:
                    sh = 0
                    while max(x["pitch"] for x in hn) + sh > hi2:
                        sh -= 12
                    while min(x["pitch"] for x in hn) + sh < lo2:
                        sh += 12
                    for x in hn:
                        x["pitch"] += sh
                parts[other] += hn
            elif pr == "call":
                resp = op("invert", op("call", call_ev, L, brng), L, brng)
                resp = [dict(e, on=e["on"] + L // 2) for e in resp if e["on"] + L // 2 < L]
                fn2 = phrase_anchor("call", root, -5, rec_hook["anchor"])
                parts[other] += place_line(th, resp, root, fn2, t, acc, *rng_lead[other], 86)
            elif comp is not None and (pr == "cad_open" or bar.get("role") == "chords"):
                parts[other] += comp_strike(th, root, t, L, other, comp, 84 if pr == "cad_open" else 74, v8)
            elif pr == "cad_open":
                lo, hi = (52, 76) if other == "g1" else (40, 64)
                v = fretboard.voice_chord(th.chord_pcs(root, (0, 2, 4, 1)), th.midi(root) % 12, lo, hi, None, 4,
                                          avoid_cluster=v8)
                for a in sorted(acc):
                    for s, f, p in v or []:
                        parts[other].append({"start": t + a, "dur": 2, "pitch": p, "vel": 92 if a == 0 else 84,
                                             "fixed": (s, f)})
            elif bar.get("role") == "octave":
                parts[other] += octave_bar(th, root, t, L, acc, bar["oct"]["grid"], bar["oct"]["dead"], oct_state,
                                           slide_max=bar["oct"].get("slide_max"))
            elif bar.get("role") == "chords" or (bar.get("role") and dense):
                if bar.get("_fallback_comp") is not None:
                    parts[other] += comp_strike(th, root, t, L, other, bar["_fallback_comp"], 74, v8)
                else:
                    parts[other] += strike(th, root, t, L, other, acc, 74)
            elif bar.get("role") == "chatter" and bar_arp.get("texture") == "tap":
                n, phase = tap_bar(th, root, t, L, phase, 66, acc, mask, tap_state)
                parts[other] += n
            elif bar.get("role") == "chatter":
                n, phase = arp_bar(th, bar_arp, root, t, L, *rng_arp[other], phase, 1, 64, acc, mask, shape)
                parts[other] += n
            elif dens == 1:
                parts[other] += strike(th, root, t, L, other, acc, 72)
            elif dens == 2 and "dens" in bar:
                n, phase = arp_bar(th, bar_arp, root, t, L, *rng_arp[other], phase, 2, 64, acc, mask, shape,
                                   eighths(acc, L))
                parts[other] += n
            elif bar_arp.get("texture") == "tap" and bar["sec"] != "A2":
                n, phase = tap_bar(th, root, t, L, phase, 66, acc, mask, tap_state, bool(skel))
                parts[other] += n
            else:
                step = 1 if bar["sec"] != "A2" else 2
                n, phase = arp_bar(th, bar_arp, root, t, L, *rng_arp[other], phase, step, 64, acc, mask, shape)
                parts[other] += n
        elif bar["sec"] == "B":
            b_index = [i for i, x in enumerate(form) if x["sec"] == "B"].index(bar_i)
            n_b = sum(1 for x in form if x["sec"] == "B")
            low = th.midi(root)
            while low > 52:
                low -= 12
            while low < 40:
                low += 12
            b_plan = st_cfg["b_plan"] if st_cfg is not None else None
            chug = (b_plan["chug"] if b_plan else recipe["motifs"].get("chug")) or [0]
            turnaround = b_index == n_b - 1
            var = bar.get("var", "chug")
            if var == "half":
                hits = b_plan["half_hits"] if b_plan else {0, max(a for a in acc if a <= L // 2)}
                parts[other] += strike(th, root, t, L, other, hits, 90, low=True)
            elif var == "clean":
                b_arp = b_plan["arp"] if b_plan else bar_arp
                n, phase = arp_bar(th, b_arp, root, t, L, *rng_arp[other], phase, 2, 62, acc, None, shape,
                                   eighths(acc, L))
                parts[other] += n
            for x in range(L if var == "chug" else 0):
                last_group = turnaround and x >= max(acc)
                if x in acc or x in chug or last_group:
                    pitch = low
                    if last_group:
                        k = x - max(acc)
                        pitch = th.midi(th.deg_near(low) + [0, 2, 4, 7, 9, 11][k % 6])
                    parts[other].append({"start": t + x, "dur": 1, "pitch": pitch, "vel": 88 if x in acc else 72,
                                         "art": None if last_group else "pm"})
                    if x in acc and not last_group:
                        parts[other].append({"start": t + x, "dur": 1, "pitch": low + 7, "vel": 82, "art": "pm",
                                             "chordtone": True})
            b_lead = st_cfg["b_lead"] if st_cfg is not None else "augmented"
            if b_lead == "rest":
                pass
            elif b_lead == "same_speed":
                inv = op("invert", base_ev, hook_L, brng)
                seg2 = [
                    dict(e, dur=min(e["dur"], L - e["on"]))
                    for e in inv
                    if e["on"] < L
                ]
                fn = phrase_anchor("B", root, 4)
                parts[lead] += place_line(th, seg2, root, fn, t, acc, *rng_lead[lead], 84, colors=col)
            else:
                inv = op("invert", base_ev, hook_L, brng)
                span = 2 * hook_L
                inv_aug = [dict(e, on=e["on"] * 2, dur=min(e["dur"] * 2, 8), art="slide" if e["dur"] >= 2 else None)
                           for e in inv]
                off0 = (b_index % 2) * L
                seg2 = [dict(e, on=(e["on"] - off0) % span) for e in inv_aug]
                seg2 = sorted((e for e in seg2 if e["on"] < L), key=lambda e: e["on"])
                if not seg2:
                    seg2 = [dict(e, on=e["on"] % L) for e in inv_aug[:3]]
                fn = phrase_anchor("B", root, 4)
                parts[lead] += place_line(th, seg2, root, fn, t, acc, *rng_lead[lead], 84, colors=col)
        elif bar["sec"] == "end" and bar.get("end") == "busy":
            riff = riff_cache.get("A3") or riff_cache.get("A") or []
            if st_cfg is not None and st_cfg["end_riff"] is not None:
                riff = st_cfg["end_riff"]
            notes = [dict(n, start=n["start"] + t) for n in riff if n["start"] < L]
            if notes:
                notes[-1]["dur"] = 1
            parts[lead] += notes
            parts[other] += [dict(n, dur=min(n["dur"], 2)) for n in strike(th, root, t, L, other, acc, 84)]
        elif bar["sec"] == "end":
            for g, (lo, hi) in (("g1", (55, 84)), ("g2", (40, 67))):
                pcs = th.chord_pcs(root, (0, 2, 4, 1))
                v = fretboard.voice_chord(pcs, th.midi(root) % 12, lo, hi, None, 4, avoid_cluster=v8)
                if v:
                    for s, f, p in v:
                        stop = bar.get("end") == "stop"
                        parts[g].append({"start": t, "dur": 2 if stop else L, "pitch": p, "vel": 104 if stop else 96,
                                         "fixed": (s, f), "ring": not stop})
        t += L
        if st_cfg is not None:
            sec_spans[-1][2] = t

    if recipe.get("control") == "shuffle":
        crng = random.Random(recipe["seed"] + 99991)
        for g in ("g1", "g2"):
            for n in parts[g]:
                if n.get("fixed") or n.get("chordtone"):
                    continue
                opts = [p for p in range(n["pitch"] - 9, n["pitch"] + 10)
                        if (p - th.tonic_pc) % 12 in th.scale and 40 <= p <= 86]
                n["pitch"] = crng.choice(opts)
    for g in ("g1", "g2"):
        for n in parts[g]:
            while n["pitch"] > 86:
                n["pitch"] -= 12
            while n["pitch"] < 40:
                n["pitch"] += 12
    report = {"cost": {}}
    v6 = recipe.get("v6")
    fix = bool(v6 or recipe.get("fix"))
    tempo = st_cfg["tempo"] if st_cfg is not None else recipe.get("arrange", {}).get("tempo", it["tempo"])
    realize_tempo = st_cfg["base_tempo"] if st_cfg is not None else tempo
    if v6:
        spans = [(c["start"], chords[i + 1]["start"] if i + 1 < len(chords) else t) for i, c in enumerate(chords)]
        v6_plan = v6
        if st_cfg is not None:
            moved = []
            for i in v6["pushes"]:
                j = next((k for k, b in enumerate(form)
                          if b.get("_orig_bar_i") == i and b["sec"] == recipe["form"][i]["sec"]), None)
                if j is not None:
                    moved.append(j)
            v6_plan = dict(v6, pushes=moved)
        apply_rests_and_pushes(parts, lead, other, form, spans, v6_plan)
    if _trace is not None:
        _trace["pre"] = copy.deepcopy(parts)
        _trace["hook"] = hook
        _trace["form"] = form
        _trace["riff"] = {k: copy.deepcopy(v) for k, v in riff_cache.items()}
    if st_cfg is not None:
        bt = st_cfg["base"]
        canon = bt.setdefault("canon", {})
        out_parts = {"g1": [], "g2": []}
        report["cost"] = {"g1": 0.0, "g2": 0.0}
        for idx, (_sec, s0, s1) in enumerate(sec_spans):
            bsp = bt["spans"][idx] if idx < len(bt["spans"]) and bt["spans"][idx][0] == _sec else None
            sec_parts, kept = {}, {}
            for g in ("g1", "g2"):
                ns = sorted((n for n in parts[g] if s0 <= n["start"] < s1), key=lambda n: (n["start"], n["pitch"]))
                if bsp is not None:
                    if (g, idx) not in canon:
                        canon[(g, idx)] = _studio_canon(bt["pre"][g], bsp[1], bsp[2])
                    kept[g] = _studio_canon(ns, s0, s1) == canon[(g, idx)]
                else:
                    kept[g] = False
                if kept[g]:
                    delta = s0 - bsp[1]
                    sec_parts[g] = [dict(copy.deepcopy(n), start=n["start"] + delta)
                                    for n in bt["real"][g] if bsp[1] <= n["start"] < bsp[2]]
                    continue
                fixed = [n for n in ns if n.get("fixed") or n.get("chordtone")]
                line = [n for n in ns if not n.get("fixed") and not n.get("chordtone")]
                report["cost"][g] += fretboard.realize_line(
                    line, allow_tap=(g == "g1"), sec16=15 / realize_tempo if fix else None
                )
                for n in fixed:
                    if n.get("fixed"):
                        n["string"], n["fret"] = n["fixed"]
                    else:
                        base = next((m for m in line if m["start"] == n["start"]), None)
                        s = base["string"] + 1 if base and base["string"] < 5 else 1
                        n["string"], n["fret"] = s, n["pitch"] - fretboard.OPEN[s]
                        if n["fret"] < 0:
                            n["string"], n["fret"] = base["string"], base["fret"] + 7 if base else (0, 0)
                    n["tech"] = n.get("preset_tech") or (["pm"] if n.get("art") == "pm" else [])
                fretboard.apply_ring(ns, 6)
                for n in ns:
                    n["dur"] = min(n["dur"], max(1, s1 - n["start"]))
                sec_parts[g] = ns
            for g in ("g1", "g2"):
                out_parts[g].extend(sec_parts[g])
        parts = out_parts
        if fix:
            report["m9_resolved"] = resolve_minor_ninths(parts, lead, other)
        if st_cfg["square_repeat"] > 1:
            t = _apply_square_repeat(
                parts, chords, meters, t, st_cfg["square_repeat"],
                form=form if _trace is not None else None,
            )
            if _trace is not None:
                _trace["form"] = form
        if st_cfg["mute_share"] > 0.0:
            _apply_mute_share(parts, other, chords, t, recipe["seed"], st_cfg["mute_share"])
    else:
        for g in ("g1", "g2"):
            ns = sorted(parts[g], key=lambda n: (n["start"], n["pitch"]))
            fixed = [n for n in ns if n.get("fixed") or n.get("chordtone")]
            line = [n for n in ns if not n.get("fixed") and not n.get("chordtone")]
            report["cost"][g] = fretboard.realize_line(line, allow_tap=(g == "g1"),
                                                       sec16=15 / tempo if fix else None)
            for n in fixed:
                if n.get("fixed"):
                    n["string"], n["fret"] = n["fixed"]
                else:
                    base = next((m for m in line if m["start"] == n["start"]), None)
                    s = base["string"] + 1 if base and base["string"] < 5 else 1
                    n["string"], n["fret"] = s, n["pitch"] - fretboard.OPEN[s]
                    if n["fret"] < 0:
                        n["string"], n["fret"] = base["string"], base["fret"] + 7 if base else (0, 0)
                n["tech"] = n.get("preset_tech") or (["pm"] if n.get("art") == "pm" else [])
            fretboard.apply_ring(ns, 6)
            parts[g] = ns
        if _trace is not None:
            _trace["real"] = copy.deepcopy(parts)
        if fix:
            report["m9_resolved"] = resolve_minor_ninths(parts, lead, other)
    if _trace is not None:
        starts = [(c["sec"], c["start"]) for i, c in enumerate(chords) if i == 0 or chords[i - 1]["sec"] != c["sec"]]
        _trace["spans"] = [
            [sec, st0, starts[k + 1][1] if k + 1 < len(starts) else t] for k, (sec, st0) in enumerate(starts)
        ]
    return {"tempo": tempo, "meters": meters, "parts": parts,
            "chords": chords,
            "total16": t, "report": report, "theory": th}


def plan_skeleton(rng, intent):
    """Shared rhythm skeleton: per section, the sixteenth positions both
    guitars hit together (group accents plus 1-2 eighth-grid syncopations)."""
    out = {}
    for sec, meter, grouping in (("A", intent["meter"], intent["grouping"]),
                                 ("B", intent["b_meter"], intent["b_grouping"])):
        L = METERS[meter][0]
        acc = accents_of(grouping)
        off = [x for x in range(2, L, 2) if x not in acc and x - 1 not in acc]
        extra = rng.sample(off, min(len(off), rng.choice([1, 2])))
        out[sec] = sorted(acc | set(extra))
    return out


def conform(ev, hits, L):
    """Make the lead attack every skeleton hit: snap the nearest event within
    one sixteenth onto the hit, otherwise repeat the previous pitch there."""
    ev = sorted((dict(e) for e in ev), key=lambda e: e["on"])
    ons = {e["on"] for e in ev}
    for h in hits:
        if h in ons or h >= L:
            continue
        near = [e for e in ev if abs(e["on"] - h) == 1 and e["on"] not in hits]
        if near:
            e = near[0]
            ons.discard(e["on"])
            e["on"] = h
            ons.add(h)
        else:
            prev = [e for e in ev if e["on"] < h]
            ev.append({"on": h, "dur": 1, "off": prev[-1]["off"] if prev and prev[-1]["off"] is not None else 0,
                       "art": None})
            ons.add(h)
    ev.sort(key=lambda e: e["on"])
    for i, e in enumerate(ev):
        nxt = ev[i + 1]["on"] if i + 1 < len(ev) else L
        e["dur"] = max(1, min(e["dur"], nxt - e["on"]))
    return ev


def make_recipe(seed, args):
    rng = random.Random(seed)
    intent = plan_intent(rng, args)
    v9 = getattr(args, "v9", False)
    v8 = getattr(args, "v8", False) or v9
    if v8:
        args = argparse.Namespace(**{**vars(args), "v7": True})
    v7 = getattr(args, "v7", False)
    if v7 and not args.mode:
        intent["mode"] = pick(random.Random(seed * 86028121 + 13), MODES_V7)
    th = Theory(TONICS[intent["tonic"]], intent["mode"])
    harmony = plan_harmony(rng, th, intent)
    motifs = plan_motifs(rng, intent, tail_rest=not getattr(args, "notail", False))
    form = plan_form(rng, intent, harmony)
    version = "v1"
    rec = {"seed": seed, "intent": intent, "harmony": harmony, "motifs": motifs, "form": form}
    if getattr(args, "skeleton", False):
        motifs["skeleton"] = plan_skeleton(random.Random(seed * 104729 + 7), intent)
        version = "v2-skeleton"
    if any(getattr(args, f, False) for f in ("arrange", "repeat", "texture", "v6", "v7")):
        arr = plan_arrangement(random.Random(seed * 15485863 + 11), intent)
        base = form_seconds(form, intent, intent["tempo"])
        cap = args.tempo or intent["tempo"]
        for template in [arr["template"]] + [x for x in ARR_TEMPLATES if x != arr["template"]]:
            cand = dict(arr, template=template)
            f3 = arrange_form(form, harmony, cand)
            beats = form_seconds(f3, intent, 1)
            lo, hi = math.ceil(beats / (1.25 * base)), min(cap, math.floor(beats / (0.8 * base)))
            if lo <= hi:
                break
        cand["tempo"] = args.tempo or max(lo, min(hi, arr["tempo"]))
        rec["form"], rec["arrange"] = f3, cand
        version = "v3-arrange"
    if any(getattr(args, f, False) for f in ("repeat", "texture", "v6", "v7")):
        rec["repeat"] = {"roles": random.Random(seed * 32452843 + 5).choice(ROLE_PATTERNS)}
        rec["form"] = repeat_form(rec["form"], rec["repeat"])
        version = "v4-repeat"
    if getattr(args, "texture", False) or getattr(args, "v6", False):
        rec["texture"], rec["form"] = texture_form(random.Random(seed * 49979687 + 3), rec["form"])
        version = "v5-texture"
    if getattr(args, "v6", False):
        rec["v6"] = plan_v6(random.Random(seed * 67867967 + 9), rec["form"])
        rec["form"] = [dict(b, end=rec["v6"]["end"]) if b["sec"] == "end" else b for b in rec["form"]]
        version = "v6"
    if v7:
        rec["fix"] = True
        rec["lead"] = plan_lead(random.Random(seed * 49979693 + 21), motifs["hook"], intent)
        version = "v7"
    if v8:
        rec["v8"], rec["form"] = plan_v8(random.Random(seed * 32452867 + 17), rec["form"])
        version = "v8"
    if getattr(args, "notail", False):
        version += "-notail"
    if v9:
        rec["v9"], rec["form"] = plan_v9(random.Random(seed * 15485867 + 23), intent, rec["form"])
        version = "v9"
    return {"generator": f"composer-lab/motif-dev {version}", **rec}


MODES_V7 = {"aeolian": 0.3, "dorian": 0.3, "lydian": 0.15, "mixolydian": 0.15, "ionian": 0.1}
LEAD_RANGE_V7 = {"g1": (60, 82), "g2": (50, 72)}


def plan_lead(rng, hook, intent):
    """Phrase shape for the lead: start after a short rest instead of on the
    downbeat, tie one note across an accent, colour tones allowed on accents."""
    L = METERS[intent["meter"]][0]
    acc = sorted(accents_of(intent["grouping"]))
    r = hook["rhythm"]
    ties = [a for a in acc[1:] if a in r and r.index(a) > 0]
    return {"pickup": rng.choice([1, 2]) if r and r[0] == 0 and rng.random() < 0.7 else 0,
            "tie": rng.choice(ties) if ties and rng.random() < 0.6 else None,
            "colors": True, "L": L}


def shape_lead(ev, plan):
    """Apply plan_lead to the motif events (all bars use the same shaped riff)."""
    ev = [dict(e) for e in ev]
    if plan["pickup"] and ev and ev[0]["on"] == 0 and ev[0]["dur"] > plan["pickup"]:
        ev[0]["on"] = plan["pickup"]
        ev[0]["dur"] -= plan["pickup"]
    if plan["tie"] is not None:
        at = [i for i, e in enumerate(ev) if e["on"] == plan["tie"]]
        if at and at[0] > 0:
            i = at[0]
            ev[i - 1]["dur"] += ev[i]["dur"]
            del ev[i]
    return ev


ARR_SHAPES = {  # density of the accompanying guitar per section: 1 sparse hits, 2 eighths, 3 sixteenths
    "build": {"intro": 1, "A": 1, "A2": 2, "A3": 3},
    "hot_start": {"intro": 2, "A": 3, "A2": 1, "A3": 3},
    "breathe": {"intro": 1, "A": 2, "A2": 1, "A3": 3},
    "arch": {"intro": 2, "A": 2, "A2": 3, "A3": 3},
}


ARR_TEMPLATES = ["classic", "double_b", "no_a2"]


def plan_arrangement(rng, intent):
    """Arrangement plan; the template may later be swapped so that the piece
    keeps the length of its v1 twin without getting faster."""
    template = rng.choice(ARR_TEMPLATES)
    shape = rng.choice(sorted(ARR_SHAPES))
    b_vars = ["chug", "half", "clean"]
    b1 = rng.choice(b_vars)
    b2 = rng.choice([v for v in b_vars if v != b1])
    lo, hi = {"4/4": (96, 160), "7/8": (104, 176), "6/8": (92, 150), "5/4": (100, 160)}[intent["meter"]]
    third = (hi - lo) // 3
    lo = rng.choice([lo, lo + third, lo + 2 * third])
    return {"template": template, "shape": shape, "b": [b1, b2], "tempo": rng.randrange(lo, lo + third),
            "intro_bars": rng.choice([1, 2, 2, 4])}


ROLE_PATTERNS = [["chords", "chords", "chatter"], ["chatter", "chords", "chatter"], ["chords", "chatter", "chords"]]
REPEAT_PROGS = {"A": ["riff", "riff", "riff_var", "cad_open"], "A2": ["riff", "riff", "call", "cad_open"],
                "A3": ["riff", "riff_harm", "riff_var", "cad_final"]}


def repeat_form(form, plan):
    """Four-bar squares: the riff twice verbatim, a varied third bar, a cadence.
    The second guitar alternates between chord hits and chatter, and plays
    chords whenever the riff itself is busy; in A3 it joins the riff in
    harmony only in the second bar."""
    out, pos = [], {}
    for b in form:
        b = dict(b)
        if b["sec"] in REPEAT_PROGS:
            i = pos.get(b["sec"], 0)
            pos[b["sec"]] = i + 1
            b["prog"] = REPEAT_PROGS[b["sec"]][i % 4]
            if b["sec"] == "A3" and b["prog"] in ("riff", "riff_var"):
                b["role"] = "chords"
            elif b["sec"] != "A3" and i % 4 < 3 and b["prog"] != "call":
                b["role"] = plan["roles"][i % 4]
        out.append(b)
    return out


TEX_DOSE = {"alone": 0.25, "unison": 0.12, "harm3": 0.12, "rhythm": 0.11, "none": 0.40}  # corpus: CORPUS-STUDY.md
SQUARE_END = {"riff_var": 0.80, "shift": 0.15, "stop": 0.05}
ENDINGS = {"held": 0.35, "stop": 0.2, "busy": 0.45}
ENDINGS_V6 = {"held": 0.5, "stop": 0.3, "busy": 0.2}


def pick(rng, weights):
    x = rng.random() * sum(weights.values())
    for k, w in weights.items():
        x -= w
        if x <= 0:
            return k
    return k


def texture_form(rng, form):
    """Doses from the corpus study. A square keeps one texture throughout, as
    the corpus repeats whole bars: in about a fifth of the squares one guitar
    plays alone, in another fifth the second guitar doubles the lead's rhythm,
    otherwise it comps or chatters for the whole square. The 4th bar mostly
    keeps the riff's rhythm, sometimes shifts it or stops. An octave doubling
    appears in some pieces only, for one bar."""
    out = [dict(b) for b in form]
    plan = {"ends": {}, "tex": {}, "role": {}, "end": pick(rng, ENDINGS), "octave": rng.random() < 0.3}
    for sec in ("A", "A2"):
        plan["ends"][sec] = pick(rng, SQUARE_END)
        plan["tex"][sec] = pick(rng, TEX_DOSE)
        plan["role"][sec] = rng.choice(["chords", "chatter"])
    pos = {}
    for b in out:
        if b["sec"] in ("A", "A2"):
            i = pos.get(b["sec"], 0)
            pos[b["sec"]] = i + 1
            if i % 4 == 3:
                b["prog"] = plan["ends"][b["sec"]]
            if b["prog"] in ("riff", "riff_var", "shift"):
                b["role"] = plan["role"][b["sec"]]
                if plan["tex"][b["sec"]] != "none":
                    b["tex"] = plan["tex"][b["sec"]]
            else:
                b.pop("role", None)
    if plan["octave"]:
        cands = [b for b in out if b["sec"] in ("A2", "A3") and b["prog"] in ("riff", "riff_var") and "tex" not in b]
        if cands:
            rng.choice(cands)["tex"] = "octave"
    for b in out:
        if b["sec"] == "end":
            b["end"] = plan["end"]
    return plan, out


def form_seconds(form, intent, tempo):
    six = sum(METERS[intent["b_meter"] if b["sec"] == "B" else intent["meter"]][0] for b in form)
    return six * 15 / tempo


def arrange_form(form, harmony, arr):
    dens = ARR_SHAPES[arr["shape"]]
    body = [dict(b) for b in form if b["sec"] != "intro"]
    if arr["template"] == "no_a2":
        body = [b for b in body if b["sec"] != "A2"]
    n_intro = 4 if arr["template"] == "no_a2" else arr["intro_bars"]
    intro = [{"sec": "intro", "chord": harmony["A"][i % 4], "prog": "intro"} for i in range(n_intro)]
    out = []
    for b in intro + body:
        out.append(b)
        if b["sec"] == "B":
            b["var"] = arr["b"][0]
    if arr["template"] == "double_b":
        last_b = max(i for i, b in enumerate(out) if b["sec"] == "B")
        extra = [dict(b, var=arr["b"][1]) for b in out if b["sec"] == "B"]
        out[last_b + 1:last_b + 1] = extra
    for b in out:
        if b["sec"] in dens:
            b["dens"] = dens[b["sec"]]
    return out


# ---------------------------------------------------------------- critic

def critique(piece):
    th = piece["theory"]
    score = 0.0
    notes = {g: piece["parts"][g] for g in ("g1", "g2")}
    chord_at = []
    for c in piece["chords"]:
        chord_at.append((c["start"], c["root_deg"]))

    def root_at(t):
        r = chord_at[0][1]
        for s, rr in chord_at:
            if s <= t:
                r = rr
        return r

    clashes = 0
    pairs = 0
    for a in notes["g1"]:
        for b in notes["g2"]:
            if b["start"] == a["start"]:
                pairs += 1
                ic = abs(a["pitch"] - b["pitch"]) % 12
                if ic in (1, 11) or (ic == 6 and not stable(th.deg_near(a["pitch"]), root_at(a["start"]))):
                    clashes += 1
    score -= 3.0 * clashes / max(1, pairs)
    leaps = 0
    for g in notes:
        line = [n for n in notes[g] if not n.get("fixed") and not n.get("ring")]
        for x, y in zip(line, line[1:]):
            if abs(x["pitch"] - y["pitch"]) > 12:
                leaps += 1
    score -= 0.05 * leaps
    cost = sum(piece["report"]["cost"].values()) / max(1, sum(len(v) for v in notes.values()))
    score -= cost
    peak = max(notes["g1"] + notes["g2"], key=lambda n: n["pitch"])
    pos = peak["start"] / piece["total16"]
    score -= 0.8 * abs(pos - 0.72)
    return {"score": round(score, 4), "clash_ratio": round(clashes / max(1, pairs), 3),
            "big_leaps": leaps, "fret_cost_per_note": round(cost, 3), "peak_pos": round(pos, 3)}


# ---------------------------------------------------------------- output

def tab_text(piece):
    lines_out = []
    names = ["E", "A", "D", "G", "B", "e"]
    sym = {"tap": "t", "hammer": "h", "pull": "p", "slide": "/"}
    for g in ("g1", "g2"):
        lines_out.append(f"== {g} ==")
        ns = piece["parts"][g]
        for c_i, c in enumerate(piece["chords"]):
            start = c["start"]
            end = piece["chords"][c_i + 1]["start"] if c_i + 1 < len(piece["chords"]) else piece["total16"]
            width = end - start
            rows = [["---"] * width for _ in range(6)]
            pm = [" "] * width
            for n in ns:
                if start <= n["start"] < end and float(n["start"]).is_integer():
                    i = int(n["start"] - start)
                    mark = "".join(sym[x] for x in n.get("tech", []) if x in sym)
                    cell = (mark + str(n["fret"])).ljust(3, "-")[:3]
                    rows[n["string"]][i] = cell
                    if "pm" in n.get("tech", []):
                        pm[i] = "M"
            lines_out.append(f"bar {c_i + 1} [{c['sec']}] chord deg {c['root_deg'] + 1}")
            if any(x != " " for x in pm):
                lines_out.append("PM " + "".join(x.ljust(3) for x in pm))
            for s in range(5, -1, -1):
                lines_out.append(names[s] + "|" + "".join(rows[s]) + "|")
        lines_out.append("")
    lines_out.append("legend: t=tap h=hammer-on p=pull-off /=slide M=palm mute")
    return "\n".join(lines_out)


def midi_tracks(piece, programs=(27, 27)):
    tracks = []
    for i, g in enumerate(("g1", "g2")):
        norm = [(n["start"], n["dur"], n["pitch"], max(30, min(127, n["vel"]))) for n in piece["parts"][g]
                if "pm" not in n.get("tech", [])]
        pm = [(n["start"], n["dur"], n["pitch"], max(30, min(127, n["vel"]))) for n in piece["parts"][g]
              if "pm" in n.get("tech", [])]
        tracks.append({"name": f"Guitar {i + 1}", "channel": i * 2, "program": programs[i],
                       "pan": 34 if i == 0 else 94, "notes": norm})
        if pm:
            tracks.append({"name": f"Guitar {i + 1} (palm mute)", "channel": i * 2 + 1, "program": 28,
                           "pan": 34 if i == 0 else 94, "notes": pm})
    return tracks


def export(piece, out_base, programs=(27, 27)):
    write_midi(out_base.with_suffix(".mid"), piece["tempo"], piece["meters"], midi_tracks(piece, programs))
    out_base.with_suffix(".tab.txt").write_text(tab_text(piece), encoding="utf-8")
    musicxml.write_musicxml(piece, out_base.with_suffix(".musicxml"), out_base.name)


def generate(seed, args, k):
    best = None
    tried = []
    for j in range(k):
        rec = make_recipe(seed * 1000 + j, args)
        if args.control:
            rec["control"] = args.control
        piece = compose(rec)
        crit = critique(piece)
        tried.append({"subseed": rec["seed"], **crit})
        if best is None or crit["score"] > best[2]["score"]:
            best = (rec, piece, crit)
    return best, tried


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--seeds", default="1")
    ap.add_argument("--candidates", type=int, default=6)
    ap.add_argument("--out", default="out")
    ap.add_argument("--meter")
    ap.add_argument("--mode")
    ap.add_argument("--key")
    ap.add_argument("--tempo", type=int)
    ap.add_argument("--recipe")
    ap.add_argument("--prefix", default="new")
    ap.add_argument("--control", choices=["shuffle"])
    ap.add_argument("--skeleton", action="store_true", help="v2: shared rhythm skeleton for both guitars")
    ap.add_argument("--arrange", action="store_true", help="v3: tempo, density, B variant and form plan")
    ap.add_argument("--repeat", action="store_true", help="v4: v3 plus repeated riffs and alternating guitar roles")
    ap.add_argument("--texture", action="store_true", help="v5: v4 plus corpus-dosed square ends, doubling and endings")
    ap.add_argument("--notail", action="store_true",
                    help="no trailing rest of 3-4 sixteenths at the end of the riff bar (combine with a version flag)")
    ap.add_argument("--v9", action="store_true",
                    help="v9: v8 plus a comping vocabulary with square variations and intro hand-over")
    ap.add_argument("--v8", action="store_true",
                    help="v8: v7 plus octave-shape rhythm guitar, leaping arpeggios, no low clusters, soft riff entry")
    ap.add_argument("--v7", action="store_true",
                    help="v7: v4 plus minor-ninth resolution, tempo-aware fingering and the lead plan")
    ap.add_argument("--v6", action="store_true",
                    help="v6: v5 plus minor-ninth resolution, rests, pushes, tempo-aware fingering")
    args = ap.parse_args(argv)
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    manifest = []
    if args.recipe:
        rec = json.loads(Path(args.recipe).read_text(encoding="utf-8"))
        piece = compose(rec)
        export(piece, out / f"{args.prefix}_recipe")
        print(json.dumps(critique(piece)))
        return
    a, b = (args.seeds.split("-") + [None])[:2]
    seeds = range(int(a), int(b) + 1) if b else [int(a)]
    for seed in seeds:
        (rec, piece, crit), tried = generate(seed, args, args.candidates)
        base = out / f"{args.prefix}_{seed:03d}"
        export(piece, base)
        base.with_suffix(".recipe.json").write_text(json.dumps(rec, indent=1), encoding="utf-8")
        manifest.append({"file": base.name, "seed": seed, "chosen_subseed": rec["seed"],
                         "candidates_considered": len(tried), "critic": crit, "all_candidates": tried,
                         "intent": rec["intent"], "bars": len(rec["form"])})
    (out / f"{args.prefix}_manifest.json").write_text(
        json.dumps({"python": sys.version, "generator": rec["generator"], "pieces": manifest},
                   indent=1), encoding="utf-8")


if __name__ == "__main__":
    main()
