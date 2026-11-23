"""Pure recipe operations and filesystem-backed store for Studio."""
import argparse
import copy
import datetime
import json
import math
import os
import random
import re
import shutil
import tempfile
import threading
import time
from pathlib import Path

import composer
try:
    from studio.render import default_render
except ImportError:
    default_render = None

SECTION_LABELS = {
    "intro": "Вступление",
    "A": "Куплет A",
    "A2": "Куплет A2",
    "B": "Середина B",
    "A3": "Кульминация",
    "end": "Концовка",
}

LAYER_LABELS = {
    "comp": "Перегенерировать ритм-гитару",
    "texture": "Фактура",
    "lead_rhythm": "Новый риф",
    "b": "Вариант середины",
    "intro": "Вступление",
    "ending": "Концовка",
    "tempo": "Темп",
}

VALID_B_LEAD = ("augmented", "same_speed", "rest")
VALID_DOSE_FLOATS = ("octave_piece", "octave_sixteenths", "dead_notes")
PIECE_ID_RE = re.compile(r"^[A-Za-z0-9_-]+$")


def default_options():
    return {
        "b_lead": "augmented",
        "trailing_rest": True,
        "octave_piece": float(composer.V8["octave_piece"]),
        "octave_sixteenths": float(composer.V8["octave_sixteenths"]),
        "dead_notes": float(composer.V8["dead_notes"]),
        "slide_max": int(composer.V8["slide_max"]),
    }


def _ensure_studio(rec):
    out = copy.deepcopy(rec)
    st = out.setdefault("studio", {})
    st.setdefault("salts", {})
    st.setdefault("locks", [])
    opts = st.setdefault("options", {})
    for k, v in default_options().items():
        if k == "trailing_rest" and k not in opts:
            v = composer.studio_base_trailing_rest(out)
        opts.setdefault(k, v)
    st.setdefault("comp_doses", {})
    return out


def new_recipe(seed=None):
    if seed is None:
        seed = random.SystemRandom().randrange(100000, 9999999)
    args = argparse.Namespace(meter=None, mode=None, key=None, tempo=None, v8=True)
    rec = composer.make_recipe(int(seed), args)
    return _ensure_studio(rec)


def available_layers(recipe):
    secs = []
    for b in recipe["form"]:
        if b["sec"] not in secs:
            secs.append(b["sec"])
    items = []
    if "intro" in secs:
        items.append({"layer": "intro", "section": "intro"})
    for s in ("A", "A2"):
        if s in secs:
            items.append({"layer": "comp", "section": s})
            items.append({"layer": "texture", "section": s})
    if "B" in secs:
        items.append({"layer": "b", "section": "B"})
    if "end" in secs:
        items.append({"layer": "ending", "section": "end"})
    items.append({"layer": "lead_rhythm", "section": "all"})
    items.append({"layer": "tempo", "section": "all"})
    return items


def layer_key(layer, section):
    return f"{layer}@{section}"


def _snapshot_comp_doses(st, sec):
    opts = st["options"]
    st.setdefault("comp_doses", {})[sec] = {
        "octave_piece": float(opts["octave_piece"]),
        "octave_sixteenths": float(opts["octave_sixteenths"]),
        "dead_notes": float(opts["dead_notes"]),
        "slide_max": int(opts["slide_max"]),
    }


def _backfill_comp_doses(st):
    for sec in ("A", "A2"):
        if int(st["salts"].get(layer_key("comp", sec), 0)) > 0 and sec not in st["comp_doses"]:
            _snapshot_comp_doses(st, sec)


def _require_text(value, what):
    if not isinstance(value, str):
        raise ValueError(f"{what} must be a string")
    return value


def reroll(recipe, layer, section):
    _require_text(layer, "layer")
    _require_text(section, "section")
    rec = _ensure_studio(recipe)
    st = rec["studio"]
    locks = set(st["locks"])
    if layer == "all" and section == "all":
        _backfill_comp_doses(st)
        for item in available_layers(rec):
            l, s = item["layer"], item["section"]
            key = layer_key(l, s)
            if key not in locks:
                st["salts"][key] = int(st["salts"].get(key, 0)) + 1
                if l == "comp":
                    _snapshot_comp_doses(st, s)
        return rec

    valid = {(x["layer"], x["section"]) for x in available_layers(rec)}
    if (layer, section) not in valid:
        raise ValueError(f"invalid layer@section: {layer}@{section}")
    key = layer_key(layer, section)
    if key in locks:
        return rec
    st["salts"][key] = int(st["salts"].get(key, 0)) + 1
    if layer == "comp":
        _snapshot_comp_doses(st, section)
    return rec


def set_lock(recipe, layer, section, locked):
    _require_text(layer, "layer")
    _require_text(section, "section")
    if not isinstance(locked, bool):
        raise ValueError("locked must be a boolean")
    rec = _ensure_studio(recipe)
    if (layer, section) not in {(x["layer"], x["section"]) for x in available_layers(rec)}:
        raise ValueError(f"invalid layer@section: {layer}@{section}")
    key = layer_key(layer, section)
    st = rec["studio"]
    st["locks"] = [k for k in st["locks"] if k != key] + ([key] if locked else [])
    if key == "ending@end":
        riffs = st.setdefault("locked_riffs", {})
        if not locked:
            riffs.pop(key, None)
        elif key not in riffs:
            trace = {}
            composer.compose(rec, _trace=trace)
            riffs[key] = json.loads(json.dumps(trace["riff"].get("A3") or trace["riff"].get("A") or []))
    return rec


def _number(value, what):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        raise ValueError(f"{what} must be a finite number")
    return value


def set_option(recipe, name, value):
    _require_text(name, "name")
    rec = _ensure_studio(recipe)
    st = rec["studio"]
    opts = st["options"]
    if name == "b_lead":
        if value not in VALID_B_LEAD:
            raise ValueError(f"invalid b_lead: {value}")
        opts["b_lead"] = value
    elif name == "trailing_rest":
        if not isinstance(value, bool):
            raise ValueError("trailing_rest must be a boolean")
        opts["trailing_rest"] = value
    elif name in VALID_DOSE_FLOATS:
        num = _number(value, name)
        _backfill_comp_doses(st)
        opts[name] = max(0.0, min(1.0, float(num)))
    elif name == "slide_max":
        num = _number(value, name)
        if num != int(num):
            raise ValueError("slide_max must be an integer")
        _backfill_comp_doses(st)
        opts["slide_max"] = max(0, min(12, int(num)))
    else:
        raise ValueError(f"unknown option: {name}")
    return rec


def piece_sections(piece):
    sec_per_tick = 15.0 / piece["tempo"]
    ch = piece["chords"]
    spans = []
    for i, c in enumerate(ch):
        s0 = c["start"]
        s1 = ch[i + 1]["start"] if i + 1 < len(ch) else piece["total16"]
        if spans and spans[-1]["section"] == c["sec"]:
            spans[-1]["end16"] = s1
        else:
            spans.append({"section": c["sec"], "start16": s0, "end16": s1})
    for sp in spans:
        sp["start_sec"] = round(sp["start16"] * sec_per_tick, 2)
        sp["end_sec"] = round(sp["end16"] * sec_per_tick, 2)
        sp["label"] = SECTION_LABELS.get(sp["section"], sp["section"])
    return spans


def piece_detail(piece_id, recipe, piece, can_undo=False):
    rec = _ensure_studio(recipe)
    st = rec["studio"]
    salts = st["salts"]
    locks = set(st["locks"])
    avail = available_layers(rec)

    def layer_info(layer, section):
        k = layer_key(layer, section)
        return {
            "layer": layer,
            "section": section,
            "label": LAYER_LABELS.get(layer, layer),
            "salt": int(salts.get(k, 0)),
            "locked": k in locks,
        }

    sections = piece_sections(piece)
    for sp in sections:
        sec = sp["section"]
        sp["layers"] = [
            layer_info(item["layer"], sec)
            for item in avail
            if item["section"] == sec
        ]

    whole_layers = [
        layer_info(item["layer"], "all")
        for item in avail
        if item["section"] == "all"
    ]
    return {
        "id": piece_id,
        "seed": rec["seed"],
        "meter": rec["intent"]["meter"],
        "mode": rec["intent"]["mode"],
        "key": rec["intent"]["tonic"],
        "tempo": piece["tempo"],
        "length": round(piece["total16"] * 15.0 / piece["tempo"], 2),
        "sections": sections,
        "whole_layers": whole_layers,
        "options": dict(st["options"]),
        "can_undo": can_undo,
    }


def _now_iso():
    return datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")


def _replace(src, dst):
    for attempt in range(60):
        try:
            os.replace(src, dst)
            return
        except PermissionError:
            if attempt == 59:
                raise
            time.sleep(0.02)


class StudioStore:
    """Filesystem-backed persistence for Studio pieces.

    Writes to one piece are serialized and staged: a revision is published only
    after it has rendered. Reads never wait for a render."""

    def __init__(self, out_dir, render_fn=None):
        if render_fn is None:
            if default_render is None:
                from studio.render import default_render as _default_render
                render_fn = _default_render
            else:
                render_fn = default_render
        self.out_dir = Path(out_dir)
        self.out_dir.mkdir(parents=True, exist_ok=True)
        self._root = self.out_dir.resolve()
        self.render_fn = render_fn
        self._guard = threading.Lock()
        self._history_lock = threading.Lock()
        self._piece_locks = {}

    def _write_lock(self, piece_id):
        with self._guard:
            return self._piece_locks.setdefault(piece_id, threading.Lock())

    def _piece_dir(self, piece_id):
        if not isinstance(piece_id, str) or not PIECE_ID_RE.match(piece_id):
            raise KeyError(f"invalid piece id: {piece_id}")
        try:
            real = (self.out_dir / piece_id).resolve()
        except OSError:
            raise KeyError(f"unknown piece id: {piece_id}")
        if real.parent != self._root or not real.is_dir():
            raise KeyError(f"unknown piece id: {piece_id}")
        return real

    def _inside(self, pdir, name):
        path = pdir / name
        if path.resolve().parent != pdir:
            raise KeyError(f"path escapes the piece directory: {name}")
        return path

    def _allocate(self, seed):
        base = f"piece_{seed}"
        with self._guard:
            n = 1
            while True:
                pid = base if n == 1 else f"{base}_{n}"
                try:
                    (self.out_dir / pid).mkdir()
                    return pid
                except FileExistsError:
                    n += 1

    def _read_recipe(self, pdir):
        return json.loads(self._inside(pdir, "recipe.json").read_text(encoding="utf-8"))

    def _read_undo(self, pdir):
        upath = self._inside(pdir, "undo.json")
        if not upath.exists():
            return []
        return json.loads(upath.read_text(encoding="utf-8"))

    def _write_undo(self, pdir, stack):
        tmp = self._inside(pdir, "undo.json.tmp")
        tmp.write_text(json.dumps(stack), encoding="utf-8")
        _replace(tmp, pdir / "undo.json")

    def _log_history(self, pdir, action, layer=None, section=None, salt=None, value=None):
        row = {
            "t": _now_iso(),
            "action": action,
            "layer": layer,
            "section": section,
            "salt": salt,
            "value": value,
        }
        path = self._inside(pdir, "history.jsonl")
        with self._history_lock, path.open("a", encoding="utf-8") as f:
            f.write(json.dumps(row, ensure_ascii=False) + "\n")

    def _build(self, pdir, recipe):
        stage = Path(tempfile.mkdtemp(prefix=".stage-", dir=pdir))
        try:
            piece = composer.compose(recipe)
            composer.export(piece, stage / "piece")
            (stage / "recipe.json").write_text(json.dumps(recipe, indent=1), encoding="utf-8")
            cur_mid, cur_mp3 = pdir / "piece.mid", pdir / "piece.mp3"
            reuse = (
                cur_mid.is_file() and cur_mp3.is_file()
                and self._inside(pdir, "piece.mid").read_bytes() == (stage / "piece.mid").read_bytes()
            )
            if not reuse:
                self.render_fn(stage / "piece.mid", stage / "piece.mp3")
                if not (stage / "piece.mp3").is_file():
                    raise RuntimeError("renderer produced no audio")
            return piece, stage, reuse
        except BaseException:
            shutil.rmtree(stage, ignore_errors=True)
            raise

    def _publish(self, pdir, stage, reuse, stack):
        names = ["piece.musicxml", "piece.tab.txt", "piece.mid"]
        if not reuse:
            names.insert(0, "piece.mp3")
        try:
            for name in names:
                _replace(stage / name, pdir / name)
            self._write_undo(pdir, stack)
            _replace(stage / "recipe.json", pdir / "recipe.json")
        finally:
            shutil.rmtree(stage, ignore_errors=True)

    def create_piece(self, seed=None):
        rec = new_recipe(seed)
        pid = self._allocate(rec["seed"])
        pdir = self.out_dir / pid
        try:
            pdir = self._piece_dir(pid)
            with self._write_lock(pid):
                piece, stage, reuse = self._build(pdir, rec)
                self._publish(pdir, stage, reuse, [])
                self._log_history(pdir, "new", value=rec["seed"])
            return piece_detail(pid, rec, piece, can_undo=False)
        except BaseException:
            shutil.rmtree(pdir, ignore_errors=True)
            raise

    def list_pieces(self):
        entries = []
        for pdir in self.out_dir.iterdir():
            try:
                if pdir.is_dir() and PIECE_ID_RE.match(pdir.name) and (pdir / "recipe.json").exists():
                    entries.append((pdir.stat().st_mtime, pdir.name))
            except OSError:
                continue
        out = []
        for _, name in sorted(entries, reverse=True):
            try:
                det = self.get_piece(name)
            except (KeyError, OSError, ValueError):
                continue
            out.append({
                "id": det["id"],
                "seed": det["seed"],
                "meter": det["meter"],
                "mode": det["mode"],
                "key": det["key"],
                "tempo": det["tempo"],
                "length": det["length"],
                "sections": [s["section"] for s in det["sections"]],
            })
        return out

    def get_piece(self, piece_id):
        pdir = self._piece_dir(piece_id)
        rec = self._read_recipe(pdir)
        piece = composer.compose(rec)
        return piece_detail(piece_id, rec, piece, can_undo=bool(self._read_undo(pdir)))

    def _change(self, piece_id, change, log):
        pdir = self._piece_dir(piece_id)
        with self._write_lock(piece_id):
            old_rec = self._read_recipe(pdir)
            new_rec = change(old_rec)
            stack = self._read_undo(pdir) + [old_rec]
            piece, stage, reuse = self._build(pdir, new_rec)
            self._publish(pdir, stage, reuse, stack)
            log(pdir, new_rec)
            return piece_detail(piece_id, new_rec, piece, can_undo=True)

    def reroll(self, piece_id, layer, section):
        def log(pdir, rec):
            whole = layer == "all" and section == "all"
            salt = dict(rec["studio"]["salts"]) if whole else rec["studio"]["salts"].get(layer_key(layer, section), 0)
            self._log_history(pdir, "reroll", layer=layer, section=section, salt=salt)

        return self._change(piece_id, lambda rec: reroll(rec, layer, section), log)

    def set_lock(self, piece_id, layer, section, locked):
        return self._change(
            piece_id,
            lambda rec: set_lock(rec, layer, section, locked),
            lambda pdir, rec: self._log_history(pdir, "lock", layer=layer, section=section, value=locked),
        )

    def set_option(self, piece_id, name, value):
        return self._change(
            piece_id,
            lambda rec: set_option(rec, name, value),
            lambda pdir, rec: self._log_history(pdir, "option", value={name: rec["studio"]["options"][name]}),
        )

    def undo(self, piece_id):
        pdir = self._piece_dir(piece_id)
        with self._write_lock(piece_id):
            stack = self._read_undo(pdir)
            if not stack:
                rec = self._read_recipe(pdir)
                return piece_detail(piece_id, rec, composer.compose(rec), can_undo=False)
            prev_rec = stack.pop()
            piece, stage, reuse = self._build(pdir, prev_rec)
            self._publish(pdir, stage, reuse, stack)
            self._log_history(pdir, "undo")
            return piece_detail(piece_id, prev_rec, piece, can_undo=bool(stack))

    def file_path(self, piece_id, filename, log_export=False):
        pdir = self._piece_dir(piece_id)
        path = self._inside(pdir, filename)
        if not path.is_file():
            raise FileNotFoundError(filename)
        if log_export:
            self._log_history(pdir, "export", value=filename)
        return path
