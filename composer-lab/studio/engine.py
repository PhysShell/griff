"""Pure recipe operations and filesystem-backed store for Studio."""
import argparse
import bisect
import copy
import datetime
import hashlib
import json
import math
import os
import random
import re
import shutil
import tempfile
import threading
import time
import uuid
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

LAYER_SHORT_LABELS = {
    "comp": "ритм-гитара",
    "texture": "фактура",
    "lead_rhythm": "новый риф",
    "b": "вариант середины",
    "intro": "вступление",
    "ending": "концовка",
    "tempo": "темп",
}

OPTION_LABELS = {
    "b_lead": "соло в середине B",
    "mute_share": "глухие ноты",
    "octave_share": "доля октав",
    "intro_bars": "длина вступления",
    "square_repeat": "повтор квадрата",
    "octave_piece": "вероятность октав",
    "octave_sixteenths": "октавы шестнадцатыми",
    "dead_notes": "глухие удары перед акцентами",
    "slide_max": "макс. слайд октав",
    "key": "тональность",
}

KEY_MODE_LABELS = {
    "ionian": "мажор",
    "dorian": "дорийский",
    "phrygian": "фригийский",
    "lydian": "лидийский",
    "mixolydian": "миксолидийский",
    "aeolian": "натуральный минор",
    "locrian": "локрийский",
}

B_LEAD_LABELS = {"augmented": "медленнее", "same_speed": "в темпе рифа", "rest": "без соло"}

VALID_B_LEAD = ("augmented", "same_speed", "rest")
VALID_DOSE_FLOATS = ("octave_piece", "octave_sixteenths", "dead_notes")
VALID_INTRO_BARS = ("auto", 1, 2, 4, 8)
VALID_SQUARE_REPEAT = (1, 2)
HISTORY_LIMIT = 50
PIECE_ID_RE = re.compile(r"^[A-Za-z0-9_-]+$")
MARK_ID_RE = re.compile(r"^[A-Za-z0-9_-]{1,40}$")
MARK_TEXT_MAX = 80
MARK_REV_RE = re.compile(r"^[0-9a-f]{12}$")
MARK_MAX_COUNT = 200
MARK_MAX_NOTES = 64
MARK_MAX_T = 3600.0
MARK_END_SLACK = 0.01
MARK_SNAP_SEC = 0.001
VALID_MARK_GUITARS = ("lead", "rhythm", "both")


def default_options():
    return {
        "b_lead": "augmented",
        "trailing_rest": True,
        "mute_share": 0.0,
        "octave_share": None,
        "intro_bars": "auto",
        "square_repeat": 1,
        "octave_piece": float(composer.V8["octave_piece"]),
        "octave_sixteenths": float(composer.V8["octave_sixteenths"]),
        "dead_notes": float(composer.V8["dead_notes"]),
        "slide_max": int(composer.V8["slide_max"]),
        "key": None,
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


def _layer_title(key):
    layer, _, section = key.partition("@")
    where = "Вся пьеса" if section == "all" else SECTION_LABELS.get(section, section)
    return f"«{where} · {LAYER_SHORT_LABELS.get(layer, layer)}»"


def _option_text(name, value):
    if name == "trailing_rest":
        return "пауза в конце такта" if value else "без паузы в конце такта"
    label = OPTION_LABELS.get(name, name)
    if name == "b_lead":
        return f"{label}: {B_LEAD_LABELS.get(value, value)}"
    if name == "key":
        return f"{label}: {value['tonic']} {KEY_MODE_LABELS[value['mode']]}" if value else f"{label}: как в пьесе"
    if value is None or value == "auto":
        return f"{label}: авто"
    return f"{label} {value:g}" if isinstance(value, float) else f"{label} {value}"


def change_label(before, after):
    old, new = _ensure_studio(before)["studio"], _ensure_studio(after)["studio"]
    salts = sorted(k for k in set(old["salts"]) | set(new["salts"]) if old["salts"].get(k, 0) != new["salts"].get(k, 0))
    locked = sorted(set(new["locks"]) - set(old["locks"]))
    unlocked = sorted(set(old["locks"]) - set(new["locks"]))
    opts = sorted(k for k in new["options"] if old["options"].get(k) != new["options"][k])
    if salts and not (locked or unlocked or opts):
        return f"перегенерация {_layer_title(salts[0])}" if len(salts) == 1 else "перегенерация всего незакреплённого"
    if len(locked) == 1 and not (salts or unlocked or opts):
        return f"закрепление {_layer_title(locked[0])}"
    if len(unlocked) == 1 and not (salts or locked or opts):
        return f"снятие закрепления {_layer_title(unlocked[0])}"
    if len(opts) == 1 and not (salts or locked or unlocked):
        return _option_text(opts[0], new["options"][opts[0]])
    return "изменение"


def history_labels(recipe, undo_stack, redo_stack):
    return {
        "can_undo": bool(undo_stack),
        "can_redo": bool(redo_stack),
        "undo_label": change_label(undo_stack[-1], recipe) if undo_stack else None,
        "redo_label": change_label(recipe, redo_stack[-1]) if redo_stack else None,
    }


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
    if (key in st["locks"]) == locked:
        return rec
    st["locks"] = [k for k in st["locks"] if k != key] + ([key] if locked else [])
    if key == "ending@end":
        riffs = st.setdefault("locked_riffs", {})
        if not locked:
            riffs.pop(key, None)
        elif key not in riffs:
            trace = {}
            unkeyed = copy.deepcopy(rec)
            unkeyed["studio"]["options"]["key"] = None
            composer.compose(unkeyed, _trace=trace)
            riffs[key] = json.loads(json.dumps(trace["riff"].get("A3") or trace["riff"].get("A") or []))
    return rec


def _number(value, what):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        raise ValueError(f"{what} must be a finite number")
    return value


def parse_key(value):
    if value is None:
        return None
    if (
        not isinstance(value, dict)
        or set(value) != {"tonic", "mode"}
        or not isinstance(value["tonic"], str)
        or not isinstance(value["mode"], str)
        or value["tonic"] not in composer.TONICS
        or value["mode"] not in composer.KEY_MODES
    ):
        raise ValueError(f"invalid key: {value}")
    return {"tonic": value["tonic"], "mode": value["mode"]}


def _parse_key(value, intent):
    key = parse_key(value)
    if key == {"tonic": intent["tonic"], "mode": intent["mode"]}:
        return None
    return key


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
    elif name == "mute_share":
        num = _number(value, name)
        opts["mute_share"] = max(0.0, min(1.0, float(num)))
    elif name == "octave_share":
        if value is None:
            opts["octave_share"] = None
        else:
            num = _number(value, name)
            opts["octave_share"] = max(0.0, min(1.0, float(num)))
    elif name == "intro_bars":
        if value == "auto":
            opts["intro_bars"] = "auto"
        elif isinstance(value, bool) or not isinstance(value, int) or value not in (1, 2, 4, 8):
            raise ValueError(f"invalid intro_bars: {value}")
        else:
            opts["intro_bars"] = value
    elif name == "square_repeat":
        if isinstance(value, bool) or not isinstance(value, int) or value not in (1, 2):
            raise ValueError(f"invalid square_repeat: {value}")
        opts["square_repeat"] = value
    elif name in VALID_DOSE_FLOATS:
        num = _number(value, name)
        clamped = max(0.0, min(1.0, float(num)))
        if opts[name] != clamped:
            _backfill_comp_doses(st)
            opts[name] = clamped
    elif name == "slide_max":
        num = _number(value, name)
        if num != int(num):
            raise ValueError("slide_max must be an integer")
        clamped = max(0, min(12, int(num)))
        if opts[name] != clamped:
            _backfill_comp_doses(st)
            opts[name] = clamped
    elif name == "key":
        opts["key"] = _parse_key(value, rec["intent"])
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


def piece_detail(piece_id, recipe, piece, undo_stack=(), redo_stack=(), marks=()):
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
        "mode": (st["options"]["key"] or rec["intent"])["mode"],
        "key": (st["options"]["key"] or rec["intent"])["tonic"],
        "own_key": {"tonic": rec["intent"]["tonic"], "mode": rec["intent"]["mode"]},
        "tempo": piece["tempo"],
        "length": round(piece["total16"] * 15.0 / piece["tempo"], 2),
        "sections": sections,
        "whole_layers": whole_layers,
        "options": dict(st["options"]),
        "rev": recipe_rev(rec),
        "marks": marks_view(rec, piece, marks, list(undo_stack) + list(redo_stack)),
        **history_labels(recipe, list(undo_stack), list(redo_stack)),
    }


def _canon(value):
    if isinstance(value, dict):
        return {k: _canon(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_canon(v) for v in value]
    if isinstance(value, float) and value.is_integer():
        return int(value)
    return value


def recipe_rev(recipe):
    raw = json.dumps(_canon(_ensure_studio(recipe)), sort_keys=True, separators=(",", ":"))
    return hashlib.sha1(raw.encode("ascii")).hexdigest()[:12]


def _bar_starts(piece):
    return [c["start"] for c in piece["chords"]]


def _bar_index(piece, t16):
    return max(0, bisect.bisect_right(_bar_starts(piece), t16) - 1)


def _bar_span(piece, i):
    chords = piece["chords"]
    end = chords[i + 1]["start"] if i + 1 < len(chords) else piece["total16"]
    return chords[i]["start"], end


def _clamped_t16(piece, t):
    t16 = max(0.0, min(t * piece["tempo"] / 15.0, piece["total16"] - 1e-6))
    tol = MARK_SNAP_SEC * piece["tempo"] / 15.0
    starts = _bar_starts(piece)
    k = bisect.bisect_left(starts, t16)
    for s in starts[max(0, k - 1):k + 1]:
        if abs(t16 - s) <= tol:
            return float(s)
    return t16


def bar_position(piece, t):
    t16 = _clamped_t16(piece, t)
    i = _bar_index(piece, t16)
    start, end = _bar_span(piece, i)
    return {"bar": i + 1, "sixteenth": min(int(t16 - start) + 1, end - start)}


def mark_notes(piece, t):
    tick = int(math.floor(_clamped_t16(piece, t) + 1e-6))
    lo, hi = tick - 1, tick + 1
    out = []
    for guitar, g in ((1, "g1"), (2, "g2")):
        for n in piece["parts"][g]:
            if n["start"] <= hi and n["start"] + n["dur"] > lo:
                out.append({
                    "guitar": guitar,
                    "pitch": int(n["pitch"]),
                    "start": n["start"],
                    "dur": n["dur"],
                    "string": n.get("string"),
                    "fret": n.get("fret"),
                    "tech": list(n.get("tech", [])),
                })
    out.sort(key=lambda n: (n["start"], n["guitar"], n["pitch"]))
    return out


def _mark_guitar(value):
    if value not in VALID_MARK_GUITARS:
        raise ValueError(f"Неверная гитара отметки: {value}")
    return value


def _mark_text(value):
    if not isinstance(value, str):
        raise ValueError("Текст отметки должен быть строкой")
    if len(value) > MARK_TEXT_MAX:
        raise ValueError(f"Текст отметки длиннее {MARK_TEXT_MAX} символов")
    return value


def _mark_id(value):
    if value is None:
        return "m" + uuid.uuid4().hex[:8]
    if not isinstance(value, str) or not MARK_ID_RE.match(value):
        raise ValueError("Неверный идентификатор отметки")
    return value


def _piece_length(piece):
    return piece["total16"] * 15.0 / piece["tempo"]


def make_mark(recipe, piece, t, guitar="both", text="", mark_id=None, rev=None, history=()):
    num = _number(t, "t")
    guitar = _mark_guitar(guitar)
    text = _mark_text(text)
    mid = _mark_id(mark_id)
    cur = recipe_rev(recipe)
    if rev is None:
        rev = cur
    elif not isinstance(rev, str) or not MARK_REV_RE.match(rev):
        raise ValueError("Неверная ревизия рецепта")
    base, notes = piece, None
    if rev != cur:
        old = next((r for r in history if recipe_rev(r) == rev), None)
        base = composer.compose(old) if old is not None else None
    if base is None:
        if not 0 <= num <= MARK_MAX_T:
            raise ValueError("Время отметки вне допустимого диапазона")
        base, notes = piece, []
    else:
        length = _piece_length(base)
        if not 0 <= num <= length + MARK_END_SLACK:
            raise ValueError("Время отметки за пределами пьесы")
        num = min(num, length)
    if notes is None:
        notes = mark_notes(base, num)
    pos = bar_position(base, num)
    sec = base["chords"][_bar_index(base, _clamped_t16(base, num))]["sec"]
    return {
        "id": mid,
        "t": float(num),
        "bar": pos["bar"],
        "sixteenth": pos["sixteenth"],
        "section": sec,
        "section_label": SECTION_LABELS.get(sec, sec),
        "guitar": guitar,
        "text": text,
        "notes": notes,
        "rev": rev,
    }


def _is_int(value):
    return isinstance(value, int) and not isinstance(value, bool)


def _is_num(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def _parse_note(raw, mid):
    where = f"Отметка {mid}: неверная нота"
    if not isinstance(raw, dict):
        raise ValueError(where)
    ok = (
        raw.get("guitar") in (1, 2) and _is_int(raw.get("guitar"))
        and _is_int(raw.get("pitch")) and 0 <= raw["pitch"] <= 127
        and _is_num(raw.get("start")) and raw["start"] >= 0
        and _is_num(raw.get("dur")) and raw["dur"] > 0
        and (raw.get("string") is None or _is_int(raw["string"]))
        and (raw.get("fret") is None or _is_int(raw["fret"]))
        and isinstance(raw.get("tech"), list) and all(isinstance(x, str) for x in raw["tech"])
    )
    if not ok:
        raise ValueError(where)
    return {k: copy.deepcopy(raw[k]) for k in ("guitar", "pitch", "start", "dur", "string", "fret", "tech")}


def _parse_mark(raw):
    if not isinstance(raw, dict):
        raise ValueError("Отметка должна быть объектом")
    mid = raw.get("id")
    if not isinstance(mid, str) or not MARK_ID_RE.match(mid):
        raise ValueError("Неверный идентификатор отметки")
    if not _is_int(raw.get("bar")) or raw["bar"] < 1 or not _is_int(raw.get("sixteenth")) or raw["sixteenth"] < 1:
        raise ValueError(f"Отметка {mid}: неверные такт или шестнадцатая")
    if not _is_num(raw.get("t")) or not 0 <= raw["t"] <= MARK_MAX_T:
        raise ValueError(f"Отметка {mid}: неверное время")
    for key in ("section", "section_label"):
        if not isinstance(raw.get(key), str):
            raise ValueError(f"Отметка {mid}: неверное поле {key}")
    if not isinstance(raw.get("rev"), str) or not MARK_REV_RE.match(raw["rev"]):
        raise ValueError(f"Отметка {mid}: неверная ревизия рецепта")
    notes = raw.get("notes")
    if not isinstance(notes, list) or len(notes) > MARK_MAX_NOTES:
        raise ValueError(f"Отметка {mid}: нот должно быть не больше {MARK_MAX_NOTES}")
    return {
        "id": mid,
        "t": raw["t"],
        "bar": raw["bar"],
        "sixteenth": raw["sixteenth"],
        "section": raw["section"],
        "section_label": raw["section_label"],
        "guitar": _mark_guitar(raw.get("guitar")),
        "text": _mark_text(raw.get("text")),
        "notes": [_parse_note(n, mid) for n in notes],
        "rev": raw["rev"],
    }


def parse_marks(raw):
    if raw is None:
        return []
    if not isinstance(raw, list):
        raise ValueError("Отметки должны быть списком")
    if len(raw) > MARK_MAX_COUNT:
        raise ValueError(f"Отметок должно быть не больше {MARK_MAX_COUNT}")
    marks = [_parse_mark(m) for m in raw]
    if len({m["id"] for m in marks}) != len(marks):
        raise ValueError("Идентификаторы отметок повторяются")
    return marks


def check_marks(recipe, piece, marks):
    cur = recipe_rev(recipe)
    length = _piece_length(piece)
    for m in marks:
        if m["rev"] != cur:
            continue
        if m["t"] > length + MARK_END_SLACK:
            raise ValueError(f"Отметка {m['id']}: время за пределами пьесы")
        pos = bar_position(piece, min(m["t"], length))
        if (pos["bar"], pos["sixteenth"]) != (m["bar"], m["sixteenth"]):
            raise ValueError(f"Отметка {m['id']}: такт и шестнадцатая не совпадают со временем")


def apply_mark_add(recipe, piece, marks, t, guitar="both", text="", mark_id=None, rev=None, history=()):
    existing = next((m for m in marks if m["id"] == mark_id), None) if mark_id is not None else None
    if existing is not None:
        return list(marks), existing, False
    if len(marks) >= MARK_MAX_COUNT:
        raise ValueError(f"Отметок должно быть не больше {MARK_MAX_COUNT}")
    mark = make_mark(recipe, piece, t, guitar, text, mark_id, rev, history)
    return list(marks) + [mark], mark, True


def _mark_index(marks, mark_id):
    i = next((k for k, m in enumerate(marks) if m["id"] == mark_id), None)
    if i is None:
        raise KeyError(f"unknown mark id: {mark_id}")
    return i


def apply_mark_edit(recipe, piece, marks, mark_id, guitar=None, text=None):
    i = _mark_index(marks, mark_id)
    mark = dict(marks[i])
    if guitar is not None:
        mark["guitar"] = _mark_guitar(guitar)
    if text is not None:
        mark["text"] = _mark_text(text)
    if mark == marks[i]:
        return list(marks), marks[i], False
    out = list(marks)
    out[i] = mark
    return out, mark, True


def apply_mark_delete(recipe, piece, marks, mark_id):
    i = _mark_index(marks, mark_id)
    return list(marks[:i]) + list(marks[i + 1:]), marks[i], True


def marks_view(recipe, piece, marks, history=()):
    cur = recipe_rev(recipe)
    length = _piece_length(piece)
    own = {}
    out = []
    for m in marks:
        stale = m["rev"] != cur
        src, reachable = piece, True
        if stale:
            if m["rev"] not in own:
                old = next((r for r in history if recipe_rev(r) == m["rev"]), None)
                own[m["rev"]] = composer.compose(old) if old is not None else None
            if own[m["rev"]] is not None:
                src = own[m["rev"]]
            else:
                reachable = m["t"] <= length + MARK_END_SLACK
        sec = 15.0 / src["tempo"]
        i = _bar_index(src, _clamped_t16(src, m["t"]))
        last = len(src["chords"]) - 1
        lo = src["chords"][max(0, i - 1)]["start"] * sec
        hi = _bar_span(src, min(last, i + 1))[1] * sec
        view = dict(m)
        view["stale"] = stale
        view["from_sec"] = round(min(lo, length), 2)
        view["to_sec"] = round(min(hi, length), 2)
        view["can_audition"] = reachable and view["from_sec"] < view["to_sec"]
        out.append(view)
    return out


def marks_export(piece_id, recipe, piece, marks):
    rev = recipe_rev(recipe)
    return {
        "version": 1,
        "id": piece_id,
        "seed": recipe["seed"],
        "rev": rev,
        "recipe": recipe,
        "marks": [{**m, "stale": m["rev"] != rev} for m in marks],
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

    def _read_redo(self, pdir):
        rpath = self._inside(pdir, "redo.json")
        if not rpath.exists():
            return []
        return json.loads(rpath.read_text(encoding="utf-8"))

    def _write_redo(self, pdir, stack):
        if not stack and not self._inside(pdir, "redo.json").exists():
            return
        tmp = self._inside(pdir, "redo.json.tmp")
        tmp.write_text(json.dumps(stack), encoding="utf-8")
        _replace(tmp, pdir / "redo.json")

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

    def _read_marks(self, pdir):
        path = self._inside(pdir, "marks.json")
        if not path.exists():
            return []
        return parse_marks(json.loads(path.read_text(encoding="utf-8")))

    def _write_marks(self, pdir, marks):
        target = self._inside(pdir, "marks.json")
        fd, name = tempfile.mkstemp(prefix="marks.", suffix=".tmp", dir=pdir)
        try:
            with os.fdopen(fd, "w", encoding="utf-8") as f:
                f.write(json.dumps(marks, ensure_ascii=False))
            _replace(name, target)
        except BaseException:
            Path(name).unlink(missing_ok=True)
            raise

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

    def _publish(self, pdir, stage, reuse, stack, redo=()):
        names = ["piece.musicxml", "piece.tab.txt", "piece.mid"]
        if not reuse:
            names.insert(0, "piece.mp3")
        tracked = names + ["undo.json", "redo.json", "recipe.json"]
        backup = stage / "backup"
        touched = False
        try:
            backup.mkdir()
            for name in tracked:
                if (pdir / name).is_file():
                    shutil.copy2(pdir / name, backup / name)
            touched = True
            for name in names:
                _replace(stage / name, pdir / name)
            self._write_undo(pdir, stack[-HISTORY_LIMIT:])
            self._write_redo(pdir, list(redo)[-HISTORY_LIMIT:])
            _replace(stage / "recipe.json", pdir / "recipe.json")
        except BaseException:
            if touched:
                for name in tracked:
                    if (backup / name).is_file():
                        _replace(backup / name, pdir / name)
                    else:
                        (pdir / name).unlink(missing_ok=True)
            raise
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
            return piece_detail(pid, rec, piece)
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
        return piece_detail(
            piece_id, rec, piece, self._read_undo(pdir), self._read_redo(pdir), self._read_marks(pdir)
        )

    def _change(self, piece_id, change, log):
        pdir = self._piece_dir(piece_id)
        with self._write_lock(piece_id):
            old_rec = self._read_recipe(pdir)
            base_rec = _ensure_studio(old_rec)
            marks = self._read_marks(pdir)
            new_rec = change(old_rec)
            if new_rec == base_rec:
                piece = composer.compose(old_rec)
                return piece_detail(piece_id, old_rec, piece, self._read_undo(pdir), self._read_redo(pdir), marks)
            stack = self._read_undo(pdir) + [old_rec]
            piece, stage, reuse = self._build(pdir, new_rec)
            self._publish(pdir, stage, reuse, stack)
            log(pdir, new_rec)
            return piece_detail(piece_id, new_rec, piece, stack[-HISTORY_LIMIT:], (), marks)

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
            cur_rec = self._read_recipe(pdir)
            redo = self._read_redo(pdir)
            marks = self._read_marks(pdir)
            if not stack:
                return piece_detail(piece_id, cur_rec, composer.compose(cur_rec), [], redo, marks)
            prev_rec = stack.pop()
            redo = redo + [cur_rec]
            piece, stage, reuse = self._build(pdir, prev_rec)
            self._publish(pdir, stage, reuse, stack, redo)
            self._log_history(pdir, "undo")
            return piece_detail(piece_id, prev_rec, piece, stack, redo[-HISTORY_LIMIT:], marks)

    def redo(self, piece_id):
        pdir = self._piece_dir(piece_id)
        with self._write_lock(piece_id):
            redo = self._read_redo(pdir)
            cur_rec = self._read_recipe(pdir)
            stack = self._read_undo(pdir)
            marks = self._read_marks(pdir)
            if not redo:
                return piece_detail(piece_id, cur_rec, composer.compose(cur_rec), stack, [], marks)
            next_rec = redo.pop()
            stack = stack + [cur_rec]
            piece, stage, reuse = self._build(pdir, next_rec)
            self._publish(pdir, stage, reuse, stack, redo)
            self._log_history(pdir, "redo")
            return piece_detail(piece_id, next_rec, piece, stack[-HISTORY_LIMIT:], redo, marks)

    def _mark_change(self, piece_id, action, change):
        pdir = self._piece_dir(piece_id)
        with self._write_lock(piece_id):
            rec = self._read_recipe(pdir)
            piece = composer.compose(rec)
            old_marks = self._read_marks(pdir)
            history = self._read_undo(pdir) + self._read_redo(pdir)
            marks, mark, changed = change(rec, piece, old_marks, history)
            if changed:
                existed = (pdir / "marks.json").exists()
                self._write_marks(pdir, marks)
                try:
                    self._log_history(pdir, action, value=mark)
                except BaseException:
                    if existed:
                        self._write_marks(pdir, old_marks)
                    else:
                        (pdir / "marks.json").unlink(missing_ok=True)
                    raise
            det = piece_detail(piece_id, rec, piece, self._read_undo(pdir), self._read_redo(pdir), marks)
            return {**det, "mark": mark}

    def add_mark(self, piece_id, t, guitar="both", text="", mark_id=None, rev=None):
        return self._mark_change(
            piece_id,
            "mark_add",
            lambda rec, piece, marks, hist: apply_mark_add(rec, piece, marks, t, guitar, text, mark_id, rev, hist),
        )

    def edit_mark(self, piece_id, mark_id, guitar=None, text=None):
        return self._mark_change(
            piece_id,
            "mark_edit",
            lambda rec, piece, marks, hist: apply_mark_edit(rec, piece, marks, mark_id, guitar, text),
        )

    def delete_mark(self, piece_id, mark_id):
        return self._mark_change(
            piece_id, "mark_delete", lambda rec, piece, marks, hist: apply_mark_delete(rec, piece, marks, mark_id)
        )

    def marks_export(self, piece_id):
        pdir = self._piece_dir(piece_id)
        rec = self._read_recipe(pdir)
        return marks_export(piece_id, rec, composer.compose(rec), self._read_marks(pdir))

    def all_marks_export(self):
        pieces = []
        for pdir in sorted(self.out_dir.iterdir(), key=lambda p: p.name):
            try:
                if pdir.is_dir() and PIECE_ID_RE.match(pdir.name) and (pdir / "recipe.json").exists():
                    pieces.append(self.marks_export(pdir.name))
            except (KeyError, OSError, ValueError):
                continue
        return {"version": 1, "pieces": pieces}

    def file_path(self, piece_id, filename, log_export=False):
        pdir = self._piece_dir(piece_id)
        path = self._inside(pdir, filename)
        if not path.is_file():
            raise FileNotFoundError(filename)
        if log_export:
            self._log_history(pdir, "export", value=filename)
        return path
