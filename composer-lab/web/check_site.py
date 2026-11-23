"""Verify composer-lab/web/ is self-contained and within size budgets."""
import json
import re
import sys
from pathlib import Path

WEB_DIR = Path(__file__).resolve().parent
PINNED_CDN_PREFIX = "https://cdn.jsdelivr.net/pyodide/v0.27.7/full/"
MAX_SAMPLES_BYTES = 8 * 1024 * 1024
MAX_SITE_BYTES = 12 * 1024 * 1024


def _fail(msg):
    print(f"FAIL: {msg}", file=sys.stderr)
    return 1


def _dir_size(path):
    return sum(p.stat().st_size for p in path.rglob("*") if p.is_file())


def _resolve_contained(base_dir: Path, rel_path: str, root_dir: Path):
    if not rel_path or rel_path.startswith(("http://", "https://", "/")):
        return None
    rel = rel_path.split("?", 1)[0].split("#", 1)[0]
    if not rel:
        return None
    target = (base_dir / rel).resolve()
    if not target.is_relative_to(root_dir):
        return None
    return target


def check(web_dir=None):
    root = Path(web_dir or WEB_DIR).resolve()
    errors = []

    for required in ("index.html", "app.js", "worker.js"):
        if not (root / required).is_file():
            errors.append(f"missing required file: {required}")
    if errors:
        for e in errors:
            _fail(e)
        return 1

    html = (root / "index.html").read_text(encoding="utf-8")
    app_js = (root / "app.js").read_text(encoding="utf-8")
    worker_js = (root / "worker.js").read_text(encoding="utf-8")

    # 1. Check external URLs: only the pinned Pyodide CDN is allowed
    for name, text in (("index.html", html), ("app.js", app_js), ("worker.js", worker_js)):
        for url in re.findall(r"""https?://[^\s"'`<>]+""", text):
            if not url.startswith(PINNED_CDN_PREFIX):
                errors.append(f"disallowed external URL in {name}: {url}")

    # 2. Check relative URLs in index.html (src=, href=)
    for attr_val in re.findall(r"""(?:src|href)\s*=\s*["']([^"']+)["']""", html):
        if attr_val.startswith(("http://", "https://", "data:", "#", "mailto:")):
            continue
        target = _resolve_contained(root, attr_val, root)
        if target is None or not target.is_file():
            errors.append(f"broken relative URL in index.html: {attr_val}")

    # 3. Check relative URLs in app.js (new Worker(...), fetch(...))
    for match in re.findall(r"""(?:new\s+Worker|fetch)\(\s*["']([^"']+)["']""", app_js):
        if match.startswith(("http://", "https://")):
            continue
        target = _resolve_contained(root, match, root)
        if target is None or not target.is_file():
            errors.append(f"broken relative URL in app.js: {match}")

    # 4. Check PY_BASE + PY_FILES in worker.js
    base_m = re.search(r"""PY_BASE\s*=\s*["']([^"']+)["']""", worker_js)
    files_m = re.search(r"""PY_FILES\s*=\s*\[([^\]]+)\]""", worker_js, re.S)
    if not base_m or not files_m:
        errors.append("worker.js missing PY_BASE or PY_FILES")
    else:
        py_base = base_m.group(1)
        py_files = re.findall(r"""["']([^"']+)["']""", files_m.group(1))
        if not py_files:
            errors.append("worker.js PY_FILES is empty")
        py_dir = _resolve_contained(root, py_base, root)
        if py_dir is None or not py_dir.is_dir():
            errors.append(f"broken PY_BASE in worker.js: {py_base}")
        else:
            for f in py_files:
                target = _resolve_contained(py_dir, f, py_dir)
                if target is None or not target.is_file():
                    errors.append(f"missing Python file referenced by worker.js: {py_base}{f}")

    # 5. Check SAMPLES_BASE in app.js, samples manifest, files, and license
    samples_m = re.search(r"""SAMPLES_BASE\s*=\s*["']([^"']+)["']""", app_js)
    samples_dir = None
    if not samples_m:
        errors.append("app.js missing SAMPLES_BASE")
    else:
        samples_base = samples_m.group(1)
        samples_dir = _resolve_contained(root, samples_base, root)
        if samples_dir is None or not samples_dir.is_dir():
            errors.append(f"broken SAMPLES_BASE in app.js: {samples_base}")

    if samples_dir is not None and samples_dir.is_dir():
        manifest_path = samples_dir / "manifest.json"
        if not manifest_path.is_file():
            errors.append("missing samples/manifest.json")
        else:
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            lic_name = manifest.get("license") or "fsbs_LICENSE.txt"
            lic_path = _resolve_contained(samples_dir, lic_name, samples_dir)
            if lic_path is None or not lic_path.is_file() or lic_path.stat().st_size == 0:
                errors.append(f"missing or empty SoundFont license: samples/{lic_name}")
            files_map = manifest.get("files") or {}
            for pitch in range(40, 89):
                p_entry = files_map.get(str(pitch))
                if not isinstance(p_entry, dict):
                    errors.append(f"manifest missing pitch {pitch}")
                    continue
                for vel in ("72", "100"):
                    fname = p_entry.get(vel)
                    if not fname:
                        errors.append(f"manifest missing pitch {pitch} velocity {vel}")
                        continue
                    spath = _resolve_contained(samples_dir, fname, samples_dir)
                    if spath is None or not spath.is_file() or spath.stat().st_size == 0:
                        errors.append(f"missing or empty sample file: samples/{fname}")

    # 6. Check size budgets
    samples_bytes = _dir_size(samples_dir) if (samples_dir is not None and samples_dir.is_dir()) else 0
    site_bytes = _dir_size(root)
    if samples_bytes >= MAX_SAMPLES_BYTES:
        errors.append(f"samples/ size {samples_bytes} bytes exceeds 8 MB limit ({MAX_SAMPLES_BYTES})")
    if site_bytes >= MAX_SITE_BYTES:
        errors.append(f"web/ size {site_bytes} bytes exceeds 12 MB limit ({MAX_SITE_BYTES})")

    if errors:
        for e in errors:
            _fail(e)
        return 1

    print(
        f"OK: site self-contained; samples={samples_bytes / (1024 * 1024):.2f} MiB "
        f"({samples_bytes} B), total={site_bytes / (1024 * 1024):.2f} MiB ({site_bytes} B)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(check())
