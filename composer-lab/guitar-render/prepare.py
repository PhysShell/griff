"""Host-side step (standard library only): MIDI (+ MusicXML) -> articulation JSON -> per-stem MIDI -> plan.json.

python prepare.py --input song.mid [--musicxml song.musicxml] --song v8_002 --mode strict --out JOBDIR
python prepare.py --input g1.mid --input g2.mid --input g3.mid --song v8_002 --out JOBDIR
"""
import argparse
import json
import os
import shutil
from pathlib import Path

from grender import articulate, ir, smf

HERE = Path(__file__).resolve().parent


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--input", action="append", required=True)
    ap.add_argument("--musicxml")
    ap.add_argument("--song", required=True, help="key in presets/songs.json")
    ap.add_argument("--mode", choices=["strict", "expressive"], default="strict")
    ap.add_argument("--preset-all", help="use one chain preset for every stem")
    ap.add_argument("--instrument-all", help="use one instrument for every stem")
    ap.add_argument("--preset", action="append", default=[], metavar="STEM=PRESET", help="override one stem's preset")
    ap.add_argument("--instrument", action="append", default=[], metavar="STEM=ID", help="override one stem's instrument")
    ap.add_argument("--stems", help="comma-separated stem ids to keep")
    ap.add_argument("--window", nargs=2, type=float, metavar=("START_S", "END_S"),
                    help="keep only notes starting in this window and shift them to 0 (for A/B clips)")
    ap.add_argument("--di-only", action="store_true")
    ap.add_argument("--no-fx", action="store_true", help="switch off delay and reverb buses")
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--out", required=True)
    a = ap.parse_args(argv)

    songs = json.load(open(HERE / "presets/songs.json"))
    song = songs[a.song]
    chains = json.load(open(HERE / "presets/chains.json"))
    assets = json.load(open(HERE / "presets/assets.json"))
    rep = ir.build(a.input, a.musicxml)
    checks = ir.midi_checks(rep)
    out = Path(a.out)
    (out / "midi").mkdir(parents=True, exist_ok=True)
    json.dump(rep, open(out / "articulations.json", "w"), indent=1)
    json.dump(checks, open(out / "midi_checks.json", "w"), indent=1)

    ppq = rep["ppq"]
    tempos, meters = rep["tempos"], rep["meters"]
    shift_ticks = 0
    if a.window:
        start_tick = _tick_at(rep, a.window[0])
        end_tick = _tick_at(rep, a.window[1])
        bar = ppq * 4 * meters[0][1] // meters[0][2]
        start_tick -= start_tick % (ppq // 4)
        shift_ticks = start_tick
        for st in rep["stems"]:
            st["notes"] = [dict(n, on=n["on"] - start_tick, off=min(n["off"], end_tick + bar) - start_tick)
                           for n in st["notes"] if start_tick <= n["on"] < end_tick]
        tempos = [(max(0, t - start_tick), us) for t, us in tempos if t <= start_tick][-1:] + \
                 [(t - start_tick, us) for t, us in tempos if start_tick < t < end_tick]
        tempos[0] = (0, tempos[0][1])
        meters = [(0, meters[0][1], meters[0][2])]
    tail_s = song.get("tail_seconds", 3.0)
    stems_plan, mapping_logs = [], {}
    keep = set(a.stems.split(",")) if a.stems else None
    last_off = max((n["off"] for st in rep["stems"] for n in st["notes"]), default=0)
    song_t = smf.Song(1, ppq, [], tempos, meters)
    end_s = song_t.seconds(last_off) + tail_s
    end_tick = last_off + _ticks_for_seconds(song_t, last_off, tail_s)
    for st in rep["stems"]:
        if keep and st["id"] not in keep:
            continue
        if not st["notes"]:
            continue
        cfg = song["stems"][st["id"]]
        over_p = dict(x.split("=", 1) for x in a.preset)
        over_i = dict(x.split("=", 1) for x in a.instrument)
        inst_id = over_i.get(st["id"]) or a.instrument_all or cfg["instrument"]
        inst = json.load(open(HERE / f"instruments/{inst_id}.json"))
        preset = over_p.get(st["id"]) or a.preset_all or cfg["preset"]
        events, mlog = articulate.stem_events(st, inst, a.mode, ppq, meters)
        events.append((end_tick, 9, b"\xff\x06\x03end"))
        mid = out / "midi" / f"{st['id']}.mid"
        smf.write(mid, ppq, tempos, meters, [(st["id"], events)])
        mapping_logs[st["id"]] = mlog
        stems_plan.append({"id": st["id"], "midi": f"midi/{st['id']}.mid", "role": cfg["role"],
                           "instrument": inst, "preset": preset, "chain": chains[preset],
                           "pan": cfg["pan"], "gain_db": cfg["gain_db"]})
    used_nam = {s["model"] for p in stems_plan for s in p["chain"]["stages"] if s["type"] == "nam"}
    used_ir = {s["ir"] for p in stems_plan for s in p["chain"]["stages"] if s["type"] == "ir"}
    plan = {
        "song": a.song, "mode": a.mode, "seed": a.seed,
        "seed_note": "no random numbers are drawn; EXPRESSIVE rules are fixed",
        "window": a.window, "shift_ticks": shift_ticks,
        "tempo_bpm": round(60e6 / tempos[0][1], 3), "sample_rate": 48000,
        "end_seconds": round(end_s, 4), "total_frames": int(round(end_s * 48000)),
        "library_root": "libs", "di_only": a.di_only,
        "fx": {"delay_note": song.get("delay_note", 0.75), "reverb": song["reverb"],
               "delay_on": not a.no_fx, "reverb_on": not a.no_fx},
        "assets": {"nam": {k: v for k, v in assets["nam"].items() if k in used_nam},
                   "ir": {k: v for k, v in assets["ir"].items() if k in used_ir}},
        "stems": stems_plan, "mapping_logs": mapping_logs,
        "input_hashes": {os.path.basename(s["path"]): s["sha256"] for s in rep["sources"]},
    }
    json.dump(plan, open(out / "plan.json", "w"), indent=1)
    print(json.dumps({"stems": [s["id"] for s in stems_plan], "end_seconds": plan["end_seconds"],
                      "mapping_logs": mapping_logs, "match": rep["match_report"]["matched"]}, indent=None))


def _tick_at(rep, seconds):
    song = smf.Song(1, rep["ppq"], [], rep["tempos"], rep["meters"])
    lo, hi = 0, 10 ** 9
    while lo < hi:
        mid = (lo + hi) // 2
        if song.seconds(mid) < seconds:
            lo = mid + 1
        else:
            hi = mid
    return lo


def _ticks_for_seconds(song, from_tick, seconds):
    target = song.seconds(from_tick) + seconds
    t = from_tick
    step = song.ppq
    while song.seconds(t) < target:
        t += step
    return t - from_tick


if __name__ == "__main__":
    main()
