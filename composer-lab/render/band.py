"""Bass and drums under a composed piece, for listening only (not part of the
composition): python band.py <recipe.json> <out.mid>

Bass plays the chord root on the group accents; drums play kick on the
accents, snare on the accent nearest the middle of the bar, closed hi-hat on
eighths, a crash at each section start. The intro has hi-hat only.
"""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import composer
from midiio import write_midi


def band_tracks(rec):
    p = composer.compose(rec)
    th, it = p["theory"], rec["intent"]
    bass, drums = [], []
    prev_sec = None
    for i, c in enumerate(p["chords"]):
        sec = rec["form"][i]["sec"]
        start = c["start"]
        end = p["chords"][i + 1]["start"] if i + 1 < len(p["chords"]) else p["total16"]
        L = end - start
        grouping = it["b_grouping"] if sec == "B" else it["grouping"]
        acc = sorted(composer.accents_of(grouping))
        if sec != prev_sec and sec != "intro":
            drums.append((start, 2, 49, 92))
        prev_sec = sec
        if sec == "end":
            drums.append((start, 2, 36, 100))
            root = th.midi(c["root_deg"])
            while root > 43:
                root -= 12
            bass.append((start, L, root, 96))
            continue
        for x in range(0, L, 2):
            drums.append((start + x, 1, 42, 64 if x in acc else 52))
        if sec == "intro":
            continue
        snare = min(acc[1:] or [L // 2], key=lambda a: abs(a - L // 2))
        for a in acc:
            if a != snare:
                drums.append((start + a, 1, 36, 96 if a == 0 else 84))
        drums.append((start + snare, 1, 38, 92))
        root = th.midi(c["root_deg"])
        while root > 43:
            root -= 12
        while root < 28:
            root += 12
        for k, a in enumerate(acc):
            nxt = acc[k + 1] if k + 1 < len(acc) else L
            bass.append((start + a, max(1, nxt - a - 1), root, 92 if a == 0 else 80))
    tracks = [{"name": "Bass", "channel": 0, "program": 33, "pan": 64, "notes": bass},
              {"name": "Drums", "channel": 9, "program": 0, "pan": 64, "notes": drums}]
    return p, tracks


def main():
    rec = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    p, tracks = band_tracks(rec)
    write_midi(Path(sys.argv[2]), p["tempo"], p["meters"], tracks)


if __name__ == "__main__":
    main()
