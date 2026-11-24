"""Host-side summary of render results (standard library only): python summarize.py RESULT_DIR..."""
import json
import os
import sys


def main():
    for root in sys.argv[1:]:
        for name in sorted(os.listdir(root)):
            mf = os.path.join(root, name, "run1", "render-manifest.json")
            if not os.path.exists(mf):
                continue
            m = json.load(open(mf))
            t = m["timings"]
            mix = m["files"].get("guitars.mix.wav", {}).get("metrics", {})
            print(f"{os.path.basename(root)}/{name}: audio {t['audio_seconds']} s, wall {t['total_wall_seconds']} s, "
                  f"RTF {t['rtf_total']}, py RSS {t['python_maxrss_mb']} MB, child RSS {t['children_maxrss_mb']} MB, "
                  f"mix peak {mix.get('peak_dbfs')} dBFS, {mix.get('lufs_integrated')} LUFS, clip {mix.get('clipped_samples')}, "
                  f"nan {mix.get('nan_or_inf')}, master {m['master_gain_db']} dB")
            for f, v in m["files"].items():
                mm = v.get("metrics", {})
                if f.endswith(".di.wav"):
                    print(f"   {f}: peak {mm['peak_dbfs']} rms {mm['rms_dbfs']} first {mm['first_signal_s']} last {mm['last_signal_s']} "
                          f"nan {mm['nan_or_inf']} dc {mm['dc_offset']} sfizz_frames {v['info']['sfizz_frames']} adv {v['info'].get('advanced_samples')}")
            for k, v in t.items():
                if k.startswith(("sfizz_", "nam_")):
                    print(f"   {k}: {v}")
            rc = os.path.join(root, name, "repeat_compare.json")
            if os.path.exists(rc):
                d = json.load(open(rc))
                for f, row in d.items():
                    print(f"   repeat {f}: " + "; ".join(f"{k}: bytes_equal={r.get('bytes_equal')} max_diff={r.get('max_abs_diff')}" for k, r in row.items()))


if __name__ == "__main__":
    main()
