"""DRAFT, NOT RUN: render stem MIDI files through Unirock HumStrat with pedalboard inside Windows Sandbox.

python -I render_unirock.py JOB_DIR OUT_DIR
JOB_DIR holds "Unirock/Pro2Max Unirock HumStrat.vst3" (with SampleLibrary beside it), plan.json and midi/.
Checks to run on the first go: load by absolute path, sample library found, parameters, state save/restore,
three independent instances, repeat renders, and the keyswitch table from the README.
"""
import json
import os
import sys
import time

import numpy as np
import pedalboard
from pedalboard.io import AudioFile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from grender import smf  # noqa: E402

SR = 48000


def midi_messages(path):
    song = smf.read(path)
    msgs = []
    for tr in song.tracks:
        for tick, _seq, kind, pl in sorted(tr.events, key=lambda e: (e[0], e[1])):
            if kind[0] == "ch":
                msgs.append((bytes([kind[1]]) + pl, song.seconds(tick)))
    return msgs


def main():
    job, out = sys.argv[1], sys.argv[2]
    plan = json.load(open(os.path.join(job, "plan.json")))
    vst = os.path.join(job, "Unirock", "Pro2Max Unirock HumStrat.vst3")
    report = {"plugin": vst, "stems": {}}
    t0 = time.perf_counter()
    inst = pedalboard.load_plugin(vst)
    report["load_seconds"] = round(time.perf_counter() - t0, 3)
    report["is_instrument"] = inst.is_instrument
    report["parameters"] = {k: str(v) for k, v in inst.parameters.items()}
    for stem in plan["stems"]:
        inst.reset()
        msgs = midi_messages(os.path.join(job, stem["midi"]))
        dur = plan["end_seconds"]
        t1 = time.perf_counter()
        audio = inst(msgs, duration=dur, sample_rate=SR, num_channels=2, buffer_size=512, reset=True)
        dt = time.perf_counter() - t1
        p = os.path.join(out, f"{stem['id']}.unirock.wav")
        with AudioFile(p, "w", SR, audio.shape[0], bit_depth=32) as f:
            f.write(audio.astype(np.float32))
        report["stems"][stem["id"]] = {"seconds": round(dt, 3), "rtf": round(dt / dur, 4),
                                       "peak": float(np.max(np.abs(audio))), "nan": int(np.isnan(audio).sum())}
    json.dump(report, open(os.path.join(out, "unirock-report.json"), "w"), indent=1)


if __name__ == "__main__":
    main()
