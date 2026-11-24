"""Guest-side checks on rendered files: onsets against the MIDI, run-to-run differences, stem alignment.

python analyze.py onsets <di.wav> <expected.json>
python analyze.py compare <dirA> <dirB> [...]
"""
import json
import os
import sys

import numpy as np
import soundfile as sf


def envelope(x, sr, win=0.003):
    n = max(1, int(sr * win))
    e = np.sqrt(np.convolve(x.astype(np.float64) ** 2, np.ones(n) / n, mode="same"))
    return e


def onsets(wav, expected):
    x, sr = sf.read(wav, dtype="float32")
    if x.ndim == 2:
        x = x[:, 0]
    exp = json.load(open(expected))
    e = envelope(x, sr)
    rep = []
    seen = set()
    for nt in exp["notes"]:
        t = nt["t_on"]
        if t in seen:
            continue
        seen.add(t)
        i = int(t * sr)
        pre = e[max(0, i - int(0.06 * sr)):max(1, i - int(0.002 * sr))]
        post = e[i:i + int(0.04 * sr)]
        ratio = float(post.max() / max(pre.mean(), 1e-9)) if len(post) and len(pre) else 0.0
        d = np.diff(e[max(0, i - int(0.06 * sr)):i + int(0.06 * sr)])
        lag = (int(np.argmax(d)) - int(0.06 * sr)) / sr * 1000 if len(d) else None
        rep.append({"t": t, "tag": nt["tag"], "rise_ratio": round(ratio, 2), "steepest_rise_ms": round(lag, 1)})
    last = np.nonzero(np.abs(x) > 10 ** (-60 / 20))[0]
    out = {"file": wav, "notes": rep, "detected": sum(1 for r in rep if r["rise_ratio"] > 1.5),
           "checked": len(rep), "last_signal_s": round(last[-1] / sr, 3) if len(last) else None,
           "expected_last_note_off_s": max(n["t_off"] for n in exp["notes"]), "end_s": round(len(x) / sr, 3)}
    print(json.dumps(out, indent=1))


def compare(dirs):
    base = dirs[0]
    out = {}
    for name in sorted(os.listdir(base)):
        if not name.endswith(".wav"):
            continue
        a, sr = sf.read(os.path.join(base, name), dtype="float64", always_2d=True)
        row = {}
        for d in dirs[1:]:
            b, _ = sf.read(os.path.join(d, name), dtype="float64", always_2d=True)
            same_bytes = open(os.path.join(base, name), "rb").read() == open(os.path.join(d, name), "rb").read()
            if a.shape != b.shape:
                row[d] = {"bytes_equal": same_bytes, "shape": [list(a.shape), list(b.shape)]}
                continue
            diff = a - b
            peak = float(np.max(np.abs(a))) or 1e-12
            row[os.path.basename(d.rstrip("/"))] = {
                "bytes_equal": same_bytes, "max_abs_diff": float(np.max(np.abs(diff))),
                "max_abs_diff_db_rel_peak": round(20 * np.log10(max(float(np.max(np.abs(diff))), 1e-15) / peak), 1),
                "rms_diff_db_rel_signal": round(10 * np.log10(max(float(np.mean(diff ** 2)), 1e-30) / max(float(np.mean(a ** 2)), 1e-30)), 1),
                "first_diff_s": round(int(np.argmax(np.any(np.abs(diff) > 0, axis=1))) / sr, 4) if np.any(diff) else None,
            }
        out[name] = row
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    if sys.argv[1] == "onsets":
        onsets(sys.argv[2], sys.argv[3])
    else:
        compare(sys.argv[2:])
