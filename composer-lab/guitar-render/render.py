"""Guest-side renderer: DI (sfizz_render) -> NAM (NAudio.Nam via namrender) -> cabinet IR -> EQ/FX -> mix.

Runs inside the offline auditbox. Usage: python render.py plan.json OUTDIR
Environment: SFIZZ_RENDER, DOTNET_BIN, NAMRENDER (set by guest_setup.sh).
"""
import hashlib
import json
import math
import os
import resource
import subprocess
import sys
import time

import numpy as np
import pedalboard as pb
import pyloudnorm
import soundfile as sf
from scipy.signal import fftconvolve

SR = 48000
LOG = []


def log(msg):
    line = f"[{time.strftime('%H:%M:%S')}] {msg}"
    LOG.append(line)
    print(line, flush=True)


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def db(x):
    return 20 * math.log10(max(x, 1e-12))


def metrics(x, sr=SR):
    x2 = x if x.ndim == 2 else x[:, None]
    finite = np.isfinite(x2)
    y = np.where(finite, x2, 0.0)
    peak = float(np.max(np.abs(y))) if y.size else 0.0
    rms = float(np.sqrt(np.mean(y.astype(np.float64) ** 2))) if y.size else 0.0
    env = np.max(np.abs(y), axis=1)
    above = np.nonzero(env > 10 ** (-60 / 20))[0]
    try:
        lufs = float(pyloudnorm.Meter(sr).integrated_loudness(y.astype(np.float64)))
    except Exception:
        lufs = None
    return {
        "seconds": round(len(y) / sr, 4), "channels": y.shape[1],
        "peak_dbfs": round(db(peak), 2), "rms_dbfs": round(db(rms), 2),
        "lufs_integrated": None if lufs is None or not math.isfinite(lufs) else round(lufs, 2),
        "clipped_samples": int(np.sum(np.abs(y) >= 1.0)),
        "nan_or_inf": int(np.sum(~finite)),
        "dc_offset": [round(float(v), 6) for v in np.mean(y, axis=0)],
        "first_signal_s": round(above[0] / sr, 4) if len(above) else None,
        "last_signal_s": round(above[-1] / sr, 4) if len(above) else None,
        "signal_present": bool(len(above)),
    }


def child_maxrss_mb():
    return round(resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss / 1024, 1)


def self_maxrss_mb():
    return round(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1024, 1)


def render_di(stem, plan, tmp, timings):
    lib = plan["library_root"]
    sfz = os.path.join(lib, stem["instrument"]["library_dir"], stem["instrument"]["sfz"])
    raw = os.path.join(tmp, f"{stem['id']}.sfizz.wav")
    cmd = [os.environ["SFIZZ_RENDER"], "--sfz", sfz, "--midi", stem["midi"], "--wav", raw,
           "--samplerate", str(SR), "--use-eot"]
    t0 = time.perf_counter()
    p = subprocess.run(cmd, capture_output=True, text=True)
    dt = time.perf_counter() - t0
    if p.returncode != 0:
        raise RuntimeError(f"sfizz_render failed for {stem['id']}: {p.stdout[-800:]} {p.stderr[-800:]}")
    x, sr = sf.read(raw, dtype="float32", always_2d=True)
    info = sf.info(raw)
    if sr != SR:
        raise RuntimeError(f"sfizz_render wrote {sr} Hz")
    ch = stem["instrument"].get("di_channel", "left")
    mono = x[:, 0] if ch == "left" or x.shape[1] == 1 else x.mean(axis=1)
    n = plan["total_frames"]
    raw_frames = len(mono)
    adv = int(round(stem["instrument"].get("di_advance_ms", 0) * SR / 1000))
    mono = mono[adv:]
    mono = mono[:n] if len(mono) >= n else np.pad(mono, (0, n - len(mono)))
    timings[f"sfizz_{stem['id']}"] = {"seconds": round(dt, 3), "rtf": round(dt / (n / SR), 4),
                                      "child_maxrss_mb_cumulative": child_maxrss_mb()}
    lr_diff = float(np.max(np.abs(x[:, 0] - x[:, 1]))) if x.shape[1] == 2 else None
    return mono.astype(np.float32), {"sfizz_subtype": info.subtype, "sfizz_channels": x.shape[1],
                                     "sfizz_frames": raw_frames, "padded_or_trimmed_to": n, "advanced_samples": adv,
                                     "left_right_max_abs_difference": lr_diff,
                                     "stdout_tail": p.stdout[-300:]}


def load_ir(path):
    h, sr = sf.read(path, dtype="float64", always_2d=True)
    h = h[:, 0]
    if sr != SR:
        raise RuntimeError(f"IR {path} is {sr} Hz; resampling is not implemented")
    return h / np.sqrt(np.sum(h ** 2))


def run_nam(x, stage, plan, tmp, tag, timings):
    model = plan["assets"]["nam"][stage["model"]]["file"]
    src = os.path.join(tmp, f"{tag}.nam_in.wav")
    dst = os.path.join(tmp, f"{tag}.nam_out.wav")
    sf.write(src, x.astype(np.float32), SR, subtype="FLOAT")
    cmd = [os.environ["DOTNET_BIN"], os.environ["NAMRENDER"], model, str(stage.get("size", 1.0)), src, dst,
           str(stage.get("input_db", 0))]
    t0 = time.perf_counter()
    p = subprocess.run(cmd, capture_output=True, text=True)
    dt = time.perf_counter() - t0
    if p.returncode != 0:
        raise RuntimeError(f"namrender failed: {p.stdout[-500:]} {p.stderr[-800:]}")
    info = json.loads(p.stdout)
    y, _ = sf.read(dst, dtype="float32")
    timings[f"nam_{tag}"] = {"wall_seconds_incl_dotnet_start": round(dt, 3),
                             "process_seconds": round(info["files"][0]["seconds_process"], 3),
                             "rtf_process": round(info["files"][0]["rtf"], 4),
                             "rtf_wall": round(dt / (len(x) / SR), 4),
                             "model_load_seconds": round(info["load_seconds"], 3),
                             "child_maxrss_mb_cumulative": child_maxrss_mb()}
    meta = {k: info[k] for k in ("version", "architecture", "a2_full", "a2_lite", "channels", "weights",
                                 "sample_rate", "loudness", "gain", "input_level_dbu", "output_level_dbu")}
    return y, meta


def pb_stage(stage):
    t = stage["type"]
    if t == "hpf":
        return pb.HighpassFilter(cutoff_frequency_hz=stage["hz"])
    if t == "lpf":
        return pb.LowpassFilter(cutoff_frequency_hz=stage["hz"])
    if t == "peak":
        return pb.PeakFilter(cutoff_frequency_hz=stage["hz"], gain_db=stage["gain_db"], q=stage.get("q", 0.7))
    if t == "comp":
        return pb.Compressor(threshold_db=stage["threshold_db"], ratio=stage["ratio"],
                             attack_ms=stage["attack_ms"], release_ms=stage["release_ms"])
    if t == "chorus":
        return pb.Chorus(rate_hz=stage["rate_hz"], depth=stage["depth"], mix=stage["mix"])
    if t == "phaser":
        return pb.Phaser(rate_hz=stage["rate_hz"], mix=stage["mix"])
    if t == "distortion":
        return pb.Distortion(drive_db=stage["drive_db"])
    if t == "gain":
        return pb.Gain(gain_db=stage["db"])
    raise ValueError(f"unknown stage {t}")


def process_stem(di, stem, plan, tmp, timings):
    chain = stem["chain"]
    g = 10 ** (chain["di_peak_dbfs"] / 20) / max(float(np.max(np.abs(di))), 1e-9)
    x = (di * g).astype(np.float32)
    notes = {"di_gain_db": round(db(g), 2), "stages": []}
    key = f"chain_seconds_{stem['id']}"
    timings[key] = 0.0
    for i, st in enumerate(chain["stages"]):
        if not st.get("on", True):
            notes["stages"].append({"type": st["type"], "on": False})
            continue
        t0 = time.perf_counter()
        if st["type"] == "nam":
            x, meta = run_nam(x, st, plan, tmp, f"{stem['id']}_{i}", timings)
            notes["stages"].append({"type": "nam", "model": st["model"], "size": st.get("size", 1.0),
                                    "input_db": st.get("input_db", 0), "meta": meta,
                                    "out_peak_dbfs": round(db(float(np.max(np.abs(x)))), 2)})
        elif st["type"] == "ir":
            h = load_ir(plan["assets"]["ir"][st["ir"]]["file"])
            x = fftconvolve(x.astype(np.float64), h)[:len(x)].astype(np.float32)
            notes["stages"].append({"type": "ir", "ir": st["ir"], "ir_taps": len(h)})
        else:
            x = pb_stage(st)(x[None, :], SR, reset=True)[0]
            notes["stages"].append({"type": st["type"]})
        timings[key] = round(timings[key] + time.perf_counter() - t0, 3)
    target = plan.get("stem_lufs", -20.0)
    try:
        lufs = pyloudnorm.Meter(SR).integrated_loudness(x.astype(np.float64))
    except Exception:
        lufs = float("-inf")
    if math.isfinite(lufs):
        trim = target - lufs
        x = x * np.float32(10 ** (trim / 20))
        notes["output_trim_db"] = round(trim, 2)
        notes["output_trim_note"] = f"one gain for the whole stem to {target} LUFS"
    return x.astype(np.float32), notes


def pan_gains(p):
    a = (p + 1) * math.pi / 4
    return math.cos(a), math.sin(a)


def mix(stems_out, plan):
    n = plan["total_frames"]
    out = np.zeros((n, 2), np.float64)
    send_d = np.zeros(n, np.float64)
    send_r = np.zeros(n, np.float64)
    for s in stems_out:
        x = s["processed"].astype(np.float64) * 10 ** (s["gain_db"] / 20)
        gl, gr = pan_gains(s["pan"])
        out[:, 0] += x * gl
        out[:, 1] += x * gr
        if s["chain"].get("send_delay_db") is not None:
            send_d += x * 10 ** (s["chain"]["send_delay_db"] / 20)
        if s["chain"].get("send_reverb_db") is not None:
            send_r += x * 10 ** (s["chain"]["send_reverb_db"] / 20)
    fx = plan["fx"]
    if fx.get("delay_on", True) and np.any(send_d):
        beat = 60.0 / plan["tempo_bpm"]
        sd = send_d[None, :].astype(np.float32)
        dl = pb.Delay(delay_seconds=beat * fx["delay_note"], feedback=0.32, mix=1.0)(sd, SR)[0]
        dr = pb.Delay(delay_seconds=beat * fx["delay_note"] * 2, feedback=0.25, mix=1.0)(sd, SR)[0]
        out[:, 0] += dl
        out[:, 1] += dr
        send_r += 0.5 * (dl + dr)
    if fx.get("reverb_on", True) and np.any(send_r):
        rv = fx["reverb"]
        st = np.stack([send_r, send_r]).astype(np.float32)
        wet = pb.Reverb(room_size=rv["room_size"], damping=rv["damping"], width=rv["width"],
                        wet_level=1.0, dry_level=0.0)(st, SR)
        out += wet.T
    peak = float(np.max(np.abs(out)))
    master_db = 0.0
    if peak > 10 ** (-1 / 20):
        master_db = db(10 ** (-1 / 20) / peak)
        out *= 10 ** (master_db / 20)
    return out.astype(np.float32), round(master_db, 2)


def main():
    plan_path, outdir = sys.argv[1], sys.argv[2]
    plan = json.load(open(plan_path))
    os.makedirs(outdir, exist_ok=True)
    tmp = os.path.join(os.environ.get("TMPDIR", "/tmp"), "gr01_" + str(os.getpid()))
    os.makedirs(tmp, exist_ok=True)
    t_all = time.perf_counter()
    timings, files = {}, {}
    stems_out = []
    for stem in plan["stems"]:
        log(f"DI {stem['id']} ({stem['instrument']['id']}, {stem['midi']})")
        di, di_info = render_di(stem, plan, tmp, timings)
        p = os.path.join(outdir, f"{stem['id']}.di.wav")
        sf.write(p, di, SR, subtype="FLOAT")
        files[os.path.basename(p)] = {"metrics": metrics(di), "info": di_info}
        if plan.get("di_only"):
            continue
        log(f"chain {stem['id']} {stem['preset']}")
        y, notes = process_stem(di, stem, plan, tmp, timings)
        p = os.path.join(outdir, f"{stem['id']}.processed.wav")
        sf.write(p, y, SR, subtype="FLOAT")
        files[os.path.basename(p)] = {"metrics": metrics(y), "chain": notes}
        stems_out.append({**stem, "processed": y})
    master_db = None
    if stems_out:
        log("mix")
        t0 = time.perf_counter()
        m, master_db = mix(stems_out, plan)
        timings["mix_seconds"] = round(time.perf_counter() - t0, 3)
        p = os.path.join(outdir, "guitars.mix.wav")
        sf.write(p, m, SR, subtype="PCM_24")
        files["guitars.mix.wav"] = {"metrics": metrics(sf.read(p, dtype="float32")[0])}
        if plan.get("mp3", True):
            subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-i", p, "-codec:a", "libmp3lame", "-b:a", "256k",
                            os.path.join(outdir, "guitars.mix.mp3")], check=True)
    timings["total_wall_seconds"] = round(time.perf_counter() - t_all, 3)
    timings["audio_seconds"] = round(plan["total_frames"] / SR, 3)
    timings["rtf_total"] = round(timings["total_wall_seconds"] / timings["audio_seconds"], 4)
    timings["python_maxrss_mb"] = self_maxrss_mb()
    timings["children_maxrss_mb"] = child_maxrss_mb()
    hashes = {}
    for name in sorted(os.listdir(outdir)):
        if name.endswith((".wav", ".mp3")):
            hashes[name] = sha256(os.path.join(outdir, name))
    manifest = {
        "plan_sha256": sha256(plan_path), "plan": plan, "master_gain_db": master_db,
        "outputs_sha256": hashes, "files": files, "timings": timings,
        "versions": {"pedalboard": pb.__version__, "numpy": np.__version__, "soundfile": sf.__version__,
                     "libsndfile": sf.__libsndfile_version__, "sfizz_render": os.environ["SFIZZ_RENDER"],
                     "dotnet": os.environ["DOTNET_BIN"], "python": sys.version.split()[0]},
    }
    json.dump(manifest, open(os.path.join(outdir, "render-manifest.json"), "w"), indent=1)
    open(os.path.join(outdir, "render.log"), "w").write("\n".join(LOG) + "\n")
    log(f"done in {timings['total_wall_seconds']} s, RTF {timings['rtf_total']}")


if __name__ == "__main__":
    main()
