"""Synthetic smoke MIDI for the sfizz backend: tempo change inside a bar, chords, fast repeats,
same-pitch overlap, pitch bend, keyswitch round trips (staccato -> sustain, legato -> sustain) and a final
held note. Writes a prepared job directory (plan.json, midi/smoke.mid, expected.json).

python smoke.py --instrument bg_green --out DIR
"""
import argparse
import json
from pathlib import Path

from grender import smf

HERE = Path(__file__).resolve().parent
PPQ = 480


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--instrument", default="bg_green")
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    inst = json.load(open(HERE / f"instruments/{a.instrument}.json"))
    ks = inst["keyswitch"]
    q = PPQ
    ev, notes = [], []

    def note(on, dur, pitch, vel, tag):
        ev.append((on, 3, bytes([0x90, pitch, vel])))
        ev.append((on + dur, 2, bytes([0x80, pitch, 0])))
        notes.append({"on": on, "off": on + dur, "pitch": pitch, "vel": vel, "tag": tag})

    def switch(t, name):
        ev.append((max(0, t - 2), 0, bytes([0x90, ks[name], 100])))
        ev.append((max(0, t - 2) + 1, 1, bytes([0x80, ks[name], 0])))

    bar = 4 * q
    for p in (40, 47, 52):
        note(0, q, p, 100, "E5 chord")
    for p in (55, 59, 62):
        note(q, q, p, 90, "G triad")
    for i in range(8):
        note(2 * q + i * q // 4, q // 4 - 10, 52, 100 if i % 2 == 0 else 80, "16th repeats")
    note(bar, 3 * q // 2, 57, 95, "overlap A")
    note(bar + q // 2, q, 57, 95, "overlap B (same pitch, starts while A rings)")
    tempos = [(0, 500000), (bar + 2 * q, 666667)]
    note(bar + 2 * q, 2 * q, 59, 100, "pitch bend")
    for i in range(9):
        v = 8192 + int(8191 * min(i, 8 - i) / 4)
        v = min(v, 16383)
        ev.append((bar + 2 * q + q // 2 + i * q // 8, 2, bytes([0xE0, v & 0x7F, v >> 7])))
    ev.append((bar + 4 * q - 1, 2, bytes([0xE0, 0, 64])))
    short = "pm" if "pm" in ks else "staccato"
    switch(2 * bar, short)
    for i in range(4):
        note(2 * bar + i * q // 2, q // 2 - 20, 45, 95, f"{short} (palm-mute or its stand-in)")
    switch(2 * bar + 2 * q, "sustain")
    for i in range(4):
        note(2 * bar + 2 * q + i * q // 2, q // 2 - 20, 45, 95, "back to sustain")
    if "legato" in ks:
        note(3 * bar, q // 2, 52, 100, "picked")
        switch(3 * bar + q // 2, "legato")
        note(3 * bar + q // 2, q // 2, 54, 85, "legato arrival")
        switch(3 * bar + q, "sustain")
    note(3 * bar + q, 2 * q, 40, 100, "final held note")
    end_tick = 4 * bar + 4 * q
    ev.append((end_tick, 9, b"\xff\x06\x03end"))
    out = Path(a.out)
    (out / "midi").mkdir(parents=True, exist_ok=True)
    meters = [(0, 4, 4)]
    smf.write(out / "midi/smoke.mid", PPQ, tempos, meters, [("smoke", ev)])
    song = smf.Song(1, PPQ, [], tempos, meters)
    for n in notes:
        n["t_on"] = round(song.seconds(n["on"]), 4)
        n["t_off"] = round(song.seconds(n["off"]), 4)
    end_s = song.seconds(end_tick)
    chains = json.load(open(HERE / "presets/chains.json"))
    assets = json.load(open(HERE / "presets/assets.json"))
    plan = {"song": "smoke", "mode": "strict", "seed": 1, "tempo_bpm": 120.0, "sample_rate": 48000,
            "end_seconds": round(end_s, 4), "total_frames": int(round(end_s * 48000)), "library_root": "libs",
            "di_only": False, "fx": {"delay_note": 0.75, "reverb": {"room_size": 0.4, "damping": 0.5, "width": 1.0},
                                     "delay_on": False, "reverb_on": False},
            "assets": {"nam": {"SB50-Ch2-Trans-DI": assets["nam"]["SB50-Ch2-Trans-DI"]},
                       "ir": {"V30-2x12": assets["ir"]["V30-2x12"]}},
            "stems": [{"id": "smoke", "midi": "midi/smoke.mid", "role": "synthetic test", "instrument": inst,
                       "preset": "SWAN-HEAVY", "chain": chains["SWAN-HEAVY"], "pan": 0.0, "gain_db": 0}],
            "mapping_logs": {}, "input_hashes": {}}
    json.dump(plan, open(out / "plan.json", "w"), indent=1)
    json.dump({"tempos": tempos, "notes": notes, "end_seconds": end_s}, open(out / "expected.json", "w"), indent=1)
    print("smoke end", round(end_s, 3), "s,", len(notes), "notes")


if __name__ == "__main__":
    main()
