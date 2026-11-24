"""Map the articulation representation onto one instrument's MIDI controls.

STRICT: notes, ticks and velocities are copied unchanged; only keyswitches for techniques the
sources confirm are added. EXPRESSIVE: STRICT plus fixed, rule-based changes (no random numbers):
softer legato arrivals, a small down/up strum spread on chords of three or more notes, and
overlap for overlap-driven legato keyswitches.
"""
PRIORITY = ["dead", "pm", "slide", "hammer", "pull", "tap"]


def note_articulation(n, inst):
    for t in PRIORITY:
        if t in n["tech"]:
            return inst["articulation_map"].get(t, "sustain"), t
    return "sustain", None


def stem_events(stem, inst, mode, ppq, meters, channel=0, ks_channel=None):
    ksch = channel if ks_channel is None else ks_channel
    ks = inst["keyswitch"]
    events = []  # (tick, order, bytes)
    log = {"keyswitches": 0, "approximated": {}, "unsupported_on_instrument": {}, "chord_conflicts": 0,
           "out_of_range": 0, "velocity_changes": 0, "timing_changes": 0, "duration_changes": 0}
    by_on = {}
    for n in stem["notes"]:
        by_on.setdefault(n["on"], []).append(n)
    current = "sustain"
    lo, hi = inst["playable"]
    extend = {}
    if mode == "expressive" and inst.get("overlap_driven"):
        extend = _legato_overlaps(stem, inst, ppq)
        log["duration_changes"] = len(extend)
    bar_ticks = None
    for on in sorted(by_on):
        group = sorted(by_on[on], key=lambda n: n["pitch"])
        arts = [note_articulation(n, inst) for n in group]
        names = {a for a, _ in arts}
        if len(names) > 1:
            log["chord_conflicts"] += 1
        art, tech = max(arts, key=lambda a: sum(1 for b in arts if b == a))
        for _a, t in arts:
            if t and t in inst.get("approximations", {}):
                log["approximated"][t] = log["approximated"].get(t, 0) + 1
        if art not in ks:
            log["unsupported_on_instrument"][art] = log["unsupported_on_instrument"].get(art, 0) + 1
            art = "sustain"
        if art != current:
            kt = max(0, on - 2)
            events.append((kt, 0, bytes([0x90 | ksch, ks[art], 100])))
            events.append((kt + 1, 1, bytes([0x80 | ksch, ks[art], 0])))
            log["keyswitches"] += 1
            current = art
        strum = 0
        if mode == "expressive" and len(group) >= 3:
            beat = ppq // 2 if meters and meters[0][2] == 8 else ppq
            down = (on % beat) == 0
            strum = 1 if down else -1
        for i, n in enumerate(group):
            if not lo <= n["pitch"] <= hi:
                log["out_of_range"] += 1
            vel, start, off = n["vel"], n["on"], n["off"]
            if mode == "expressive":
                if tech in ("hammer", "pull", "slide") and art in ("legato", "legato_slide", "sustain"):
                    vel = max(1, round(vel * 0.8))
                if strum:
                    k = i if strum > 0 else len(group) - 1 - i
                    start = on + k * max(1, round(ppq * 0.008))
            if vel != n["vel"]:
                log["velocity_changes"] += 1
            if start != n["on"]:
                log["timing_changes"] += 1
            events.append((start, 3, bytes([0x90 | channel, n["pitch"], vel])))
            off = extend.get(id(n), off)
            events.append((max(off, start + 1), 2, bytes([0x80 | channel, n["pitch"], 0])))
    events.append((0, -1, bytes([0xB0 | channel, 7, 100])))
    return sorted(events, key=lambda e: (e[0], e[1])), log


def _legato_overlaps(stem, inst, ppq):
    """For overlap-driven legato: the note sounding just before each legato arrival (same string when known)
    is held until 1/64 of a quarter after the arrival, so that the two notes overlap."""
    notes = sorted(stem["notes"], key=lambda n: n["on"])
    out = {}
    for k, n in enumerate(notes):
        if note_articulation(n, inst)[0] not in inst["overlap_driven"]:
            continue
        prev = [p for p in notes[:k] if p["on"] < n["on"] and n["on"] - p["off"] <= ppq // 8
                and (n.get("string") is None or p.get("string") == n.get("string"))]
        if prev:
            p = max(prev, key=lambda p: p["on"])
            out[id(p)] = max(p["off"], n["on"] + ppq // 64)
    return out
