"""Usage log summary across all curated pieces in a Studio output directory."""
import argparse
from collections import Counter
import json
from pathlib import Path


def _collect_log_files(target: Path):
    if target.is_file():
        return [(target, target.stem)]
    files = []
    root_hist = target / "history.jsonl"
    if root_hist.is_file():
        files.append((root_hist, target.name))
    for pdir in sorted(target.iterdir()):
        hpath = pdir / "history.jsonl"
        if pdir.is_dir() and hpath.is_file():
            files.append((hpath, pdir.name))
    return files


def summarize(out_dir):
    out_dir = Path(out_dir)
    rerolls_per_layer = Counter()
    rerolls_per_target = Counter()
    options_used = Counter()
    marks_per_piece = {}
    rerolls_before_first_export = []
    pieces_total = 0
    pieces_exported = 0

    if not out_dir.exists():
        return {
            "pieces_total": 0,
            "pieces_exported": 0,
            "rerolls_per_layer": {},
            "rerolls_per_target": {},
            "rerolls_before_first_export": [],
            "options_used": {},
            "marks_per_piece": {},
        }

    groups = {}
    for hpath, fallback_key in _collect_log_files(out_dir):
        for line in hpath.read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            ev = json.loads(line)
            raw_pid = ev.get("piece_id")
            pid = raw_pid if isinstance(raw_pid, str) and raw_pid else fallback_key
            groups.setdefault(pid, []).append(ev)

    for pid in sorted(groups):
        pieces_total += 1
        rerolls_so_far = 0
        exported = False
        mark_ids = set()
        for ev in groups[pid]:
            act = ev.get("action")
            val = ev.get("value")
            if act in ("mark_add", "mark_delete") and isinstance(val, dict):
                (mark_ids.add if act == "mark_add" else mark_ids.discard)(val.get("id"))
            if act == "reroll":
                rerolls_so_far += 1
                layer = ev.get("layer") or "?"
                sec = ev.get("section") or "?"
                rerolls_per_layer[layer] += 1
                rerolls_per_target[f"{layer}@{sec}"] += 1
            elif act == "option":
                val = ev.get("value")
                if isinstance(val, dict):
                    for k, v in val.items():
                        options_used[f"{k}={v}"] += 1
                        options_used[k] += 1
            elif act == "export" and not exported:
                exported = True
                pieces_exported += 1
                rerolls_before_first_export.append(rerolls_so_far)
        marks_per_piece[pid] = len(mark_ids)

    return {
        "pieces_total": pieces_total,
        "pieces_exported": pieces_exported,
        "rerolls_per_layer": dict(rerolls_per_layer),
        "rerolls_per_target": dict(rerolls_per_target),
        "rerolls_before_first_export": rerolls_before_first_export,
        "options_used": dict(options_used),
        "marks_per_piece": marks_per_piece,
    }


def format_summary(stats):
    lines = [
        f"Pieces: {stats['pieces_total']} total, {stats['pieces_exported']} exported",
        f"Re-rolls before first export per piece: {stats['rerolls_before_first_export']}",
        "Re-rolls per layer:",
    ]
    for k, v in sorted(stats["rerolls_per_layer"].items(), key=lambda kv: (-kv[1], kv[0])):
        lines.append(f"  {k}: {v}")
    lines.append("Re-rolls per target:")
    for k, v in sorted(stats["rerolls_per_target"].items(), key=lambda kv: (-kv[1], kv[0])):
        lines.append(f"  {k}: {v}")
    lines.append("Options used:")
    for k, v in sorted(stats["options_used"].items(), key=lambda kv: (-kv[1], kv[0])):
        if "=" in k:
            lines.append(f"  {k}: {v}")
    lines.append("Marks per piece:")
    for k, v in sorted(stats["marks_per_piece"].items()):
        lines.append(f"  {k}: {v}")
    return "\n".join(lines)


def main(argv=None):
    ap = argparse.ArgumentParser(description="Summarize Studio usage logs")
    ap.add_argument("dir", help="Studio output directory or history.jsonl file")
    args = ap.parse_args(argv)
    print(format_summary(summarize(args.dir)))


if __name__ == "__main__":
    main()
