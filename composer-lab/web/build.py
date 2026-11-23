"""Copy Python generator sources into composer-lab/web/py/ for static hosting."""
import shutil
from pathlib import Path

WEB_DIR = Path(__file__).resolve().parent
LAB_DIR = WEB_DIR.parent
PY_DIR = WEB_DIR / "py"

FILES = [
    ("composer.py", "composer.py"),
    ("fretboard.py", "fretboard.py"),
    ("midiio.py", "midiio.py"),
    ("musicxml.py", "musicxml.py"),
    ("studio/engine.py", "studio/engine.py"),
    ("studio/web_api.py", "web_api.py"),
]


def build():
    if PY_DIR.exists():
        shutil.rmtree(PY_DIR)
    PY_DIR.mkdir(parents=True, exist_ok=True)
    for src_rel, dst_rel in FILES:
        src = LAB_DIR / src_rel
        dst = PY_DIR / dst_rel
        if not src.is_file():
            raise FileNotFoundError(f"missing source file: {src}")
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, dst)
    return [PY_DIR / dst_rel for _, dst_rel in FILES]


if __name__ == "__main__":
    copied = build()
    print(f"Copied {len(copied)} Python files into {PY_DIR}")
