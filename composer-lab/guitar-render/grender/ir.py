"""Instrument-independent articulation representation built from MIDI plus (optionally) composer-lab MusicXML."""
import hashlib
import re

from . import mxl, smf


def sha256(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest()


def _part_for(track_name):
    m = re.match(r"Guitar (\d+)", track_name or "")
    return f"P{m.group(1)}" if m else None


def build(midi_paths, musicxml=None, stem_names=None, dead_velocity=None):
    """midi_paths: one type-1 file (one stem per note track) or several files (one stem each, first note track)."""
    songs = [smf.read(p) for p in midi_paths]
    base = songs[0]
    for s in songs[1:]:
        if s.ppq != base.ppq or s.tempos != base.tempos:
            raise ValueError("separate MIDI files disagree on PPQ or tempo map; refusing to merge them")
    stems = []
    for p, song in zip(midi_paths, songs):
        for tr in song.tracks:
            ns, hanging = smf.notes(tr)
            if not ns:
                continue
            stems.append({"midi_file": p, "track": tr.name, "notes": ns, "hanging_note_ons": hanging,
                          "controls": [(t, st, pl.hex()) for t, st, pl in smf.channel_controls(tr)]})
            if len(midi_paths) > 1:
                break
    for i, st in enumerate(stems):
        st["id"] = stem_names[i] if stem_names else f"guitar{i + 1}"
    xml = mxl.read(musicxml) if musicxml else {}
    report = {"musicxml": bool(xml), "matched": 0, "unmatched": 0, "unmatched_examples": []}
    six = base.ppq // 4
    for st in stems:
        part = _part_for(st["track"])
        st["part"] = part
        palm_track = "palm mute" in (st["track"] or "").lower()
        index = {}
        for xn in xml.get(part, {}).get("notes", []):
            index.setdefault((round(xn["onset16"] * 4), xn["pitch"]), []).append(xn)
        for n in st["notes"]:
            n["t_on"] = round(base.seconds(n["on"]), 6)
            n["t_off"] = round(base.seconds(n["off"]), 6)
            n["tech"], n["tech_source"] = [], {}
            if palm_track:
                n["tech"].append("pm")
                n["tech_source"]["pm"] = "midi track name (composer-lab exports palm-muted notes to their own track)"
            if dead_velocity is not None and n["vel"] == dead_velocity and n["off"] - n["on"] <= six:
                n["tech"].append("dead")
                n["tech_source"]["dead"] = (f"velocity {dead_velocity} and at most one sixteenth long "
                                            "(composer-lab writes dead strokes this way)")
            if not xml or part not in xml:
                continue
            key = (round(n["on"] * 4 / six), n["pitch"])
            cand = index.get(key)
            if not cand:
                report["unmatched"] += 1
                if len(report["unmatched_examples"]) < 8:
                    report["unmatched_examples"].append({"stem": st["id"], "tick": n["on"], "pitch": n["pitch"]})
                continue
            xn = cand.pop(0)
            report["matched"] += 1
            n["string"], n["fret"] = xn["string"], xn["fret"]
            for t in xn["tech"]:
                if t not in n["tech"]:
                    n["tech"].append(t)
                    n["tech_source"][t] = "musicxml technical/notations"
    counts = {}
    for st in stems:
        for n in st["notes"]:
            for t in n["tech"]:
                counts.setdefault(st["id"], {}).setdefault(t, 0)
                counts[st["id"]][t] += 1
    report["tech_counts"] = counts
    return {
        "sources": [{"path": p, "sha256": sha256(p)} for p in midi_paths]
        + ([{"path": musicxml, "sha256": sha256(musicxml)}] if musicxml else []),
        "ppq": base.ppq, "tempos": base.tempos, "meters": base.meters,
        "end_seconds": max(n["t_off"] for st in stems for n in st["notes"]),
        "stems": stems, "match_report": report,
    }


def midi_checks(ir):
    """Facts the render depends on: tempo map, overlaps, chords, repeated notes, controls, hanging notes."""
    out = {"ppq": ir["ppq"], "tempo_changes": len(ir["tempos"]), "tempos": ir["tempos"], "meters": ir["meters"], "stems": {}}
    for st in ir["stems"]:
        ns = st["notes"]
        chords = 0
        by_on = {}
        for n in ns:
            by_on.setdefault(n["on"], []).append(n)
        chords = sum(1 for v in by_on.values() if len(v) > 1)
        same_pitch_overlap = 0
        last = {}
        for n in sorted(ns, key=lambda n: n["on"]):
            p = last.get(n["pitch"])
            if p and p["off"] > n["on"]:
                same_pitch_overlap += 1
            last[n["pitch"]] = n
        poly = 0
        ev = sorted([(n["on"], 1) for n in ns] + [(n["off"], -1) for n in ns], key=lambda e: (e[0], e[1]))
        cur = 0
        for _t, d in ev:
            cur += d
            poly = max(poly, cur)
        channels = sorted({n["ch"] for n in ns})
        out["stems"][st["id"]] = {
            "track": st["track"], "notes": len(ns), "channels": channels, "chord_onsets": chords,
            "max_polyphony": poly, "same_pitch_overlaps": same_pitch_overlap,
            "hanging_note_ons": st["hanging_note_ons"],
            "velocity_min": min(n["vel"] for n in ns), "velocity_max": max(n["vel"] for n in ns),
            "pitch_min": min(n["pitch"] for n in ns), "pitch_max": max(n["pitch"] for n in ns),
            "pitch_bend_events": sum(1 for c in st["controls"] if (c[1] & 0xF0) == 0xE0),
            "cc_events": sum(1 for c in st["controls"] if (c[1] & 0xF0) == 0xB0),
            "first_on_s": ns[0]["t_on"], "last_off_s": max(n["t_off"] for n in ns),
        }
    return out
