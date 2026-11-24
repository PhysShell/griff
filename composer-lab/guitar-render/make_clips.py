"""Cut blind A/B clips from full renders, matched to the same integrated loudness, with neutral names.

python make_clips.py spec.json OUTDIR
spec: {"seed": 7, "lufs": -20, "windows": {"name": [start, end], ...},
       "groups": [{"window": "name", "arms": {"arm": "path/to/file.wav", ...}}, ...]}
Writes OUTDIR/listen/<window>_<letter>.wav (+ .mp3) and OUTDIR/KEY.json (which arm each letter is).
"""
import json
import os
import random
import subprocess
import sys

import numpy as np
import pyloudnorm
import soundfile as sf


def main():
    spec = json.load(open(sys.argv[1]))
    out = sys.argv[2]
    listen = os.path.join(out, "listen")
    os.makedirs(listen, exist_ok=True)
    rng = random.Random(spec.get("seed", 7))
    key, missing = {}, []
    for g in spec["groups"]:
        w0, w1 = spec["windows"][g["window"]]
        arms = [(a, p) for a, p in sorted(g["arms"].items()) if os.path.exists(p)]
        missing += [p for _a, p in g["arms"].items() if not os.path.exists(p)]
        rng.shuffle(arms)
        for i, (arm, path) in enumerate(arms):
            x, sr = sf.read(path, dtype="float64", always_2d=True)
            if x.shape[1] == 1:
                x = np.repeat(x, 2, axis=1)  # centred mono for listening only; stems themselves stay mono
            seg = x[int(w0 * sr):int(w1 * sr)].copy()
            fade = int(0.01 * sr)
            ramp = np.linspace(0, 1, fade)[:, None]
            seg[:fade] *= ramp
            seg[-fade:] *= ramp[::-1]
            lufs = pyloudnorm.Meter(sr).integrated_loudness(seg)
            gain = spec.get("lufs", -20) - lufs
            seg *= 10 ** (gain / 20)
            peak = float(np.max(np.abs(seg)))
            name = f"{g['window']}_{chr(65 + i)}"
            p = os.path.join(listen, name + ".wav")
            sf.write(p, seg.astype(np.float32), sr, subtype="PCM_24")
            subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-i", p, "-codec:a", "libmp3lame", "-b:a", "256k",
                            os.path.join(listen, name + ".mp3")], check=True)
            key[name] = {"arm": arm, "source": path, "window_s": [w0, w1], "gain_db": round(gain, 2),
                         "lufs_before": round(lufs, 2), "peak_after_dbfs": round(20 * np.log10(max(peak, 1e-12)), 2),
                         "over_full_scale": peak >= 1.0}
    json.dump({"key": key, "missing_sources": missing, "spec": spec}, open(os.path.join(out, "KEY.json"), "w"), indent=1)
    print(len(key), "clips;", len(missing), "missing sources")


if __name__ == "__main__":
    main()
