"""Pure in-memory JSON-in/JSON-out Studio API for browser/Pyodide execution."""
import base64
import copy
import json
import math

import composer
import midiio
import musicxml
from studio import engine

UNDO_LIMIT = 50
MAX_SEED = 10**15
VALID_SECTIONS = frozenset(engine.SECTION_LABELS)


def _is_int(value):
    return isinstance(value, int) and not isinstance(value, bool)


def _validate_recipe(rec):
    if not isinstance(rec, dict):
        raise ValueError("recipe must be an object")
    seed = rec.get("seed")
    if not _is_int(seed) or not 0 <= seed < MAX_SEED:
        raise ValueError("recipe.seed must be a non-negative integer")
    form = rec.get("form")
    if not isinstance(form, list) or not form:
        raise ValueError("recipe.form must be a non-empty list")
    for bar in form:
        if not isinstance(bar, dict) or not isinstance(bar.get("sec"), str) or bar["sec"] not in VALID_SECTIONS:
            raise ValueError("recipe.form contains an invalid section name")
    intent = rec.get("intent")
    if not isinstance(intent, dict):
        raise ValueError("recipe.intent must be an object")
    if intent.get("meter") not in composer.METERS:
        raise ValueError("recipe.intent.meter is invalid")
    if intent.get("b_meter", intent.get("meter")) not in composer.METERS:
        raise ValueError("recipe.intent.b_meter is invalid")
    if intent.get("mode") not in composer.SCALES:
        raise ValueError("recipe.intent.mode is invalid")
    if intent.get("tonic") not in composer.TONICS:
        raise ValueError("recipe.intent.tonic is invalid")
    tempo = intent.get("tempo")
    if isinstance(tempo, bool) or not isinstance(tempo, (int, float)) or not math.isfinite(tempo) or tempo <= 0:
        raise ValueError("recipe.intent.tempo must be a positive finite number")
    st = rec.get("studio")
    if st is not None:
        if not isinstance(st, dict):
            raise ValueError("recipe.studio must be an object")
        if "salts" in st:
            salts = st["salts"]
            if not isinstance(salts, dict) or any(not isinstance(k, str) or not _is_int(v) for k, v in salts.items()):
                raise ValueError("recipe.studio.salts is invalid")
        if "locks" in st:
            locks = st["locks"]
            if not isinstance(locks, list) or any(not isinstance(k, str) for k in locks):
                raise ValueError("recipe.studio.locks is invalid")
        if "options" in st:
            opts = st["options"]
            if not isinstance(opts, dict):
                raise ValueError("recipe.studio.options must be an object")
            if "b_lead" in opts and opts["b_lead"] not in engine.VALID_B_LEAD:
                raise ValueError("invalid studio.options.b_lead")
            if "trailing_rest" in opts and not isinstance(opts["trailing_rest"], bool):
                raise ValueError("invalid studio.options.trailing_rest")
            for k in engine.VALID_DOSE_FLOATS:
                if k in opts:
                    v = opts[k]
                    if isinstance(v, bool) or not isinstance(v, (int, float)) or not math.isfinite(v) or not 0.0 <= float(v) <= 1.0:
                        raise ValueError(f"invalid studio.options.{k}")
            if "slide_max" in opts:
                sm = opts["slide_max"]
                if not _is_int(sm) or not 0 <= sm <= 12:
                    raise ValueError("invalid studio.options.slide_max")
    return copy.deepcopy(rec)


def _validate_state(state):
    if not isinstance(state, dict) or not isinstance(state.get("recipe"), dict):
        raise ValueError("state must be an object with a recipe")
    rec = _validate_recipe(state["recipe"])
    raw_id = state.get("id")
    pid = f"piece_{rec['seed']}" if raw_id is None or raw_id == "" else raw_id
    if not isinstance(pid, str) or not engine.PIECE_ID_RE.match(pid):
        raise ValueError("state.id must match piece id rules")
    undo_raw = state.get("undo", [])
    if not isinstance(undo_raw, list):
        raise ValueError("state.undo must be a list")
    undo_stack = [_validate_recipe(entry) for entry in undo_raw][-UNDO_LIMIT:]
    return pid, rec, undo_stack


def _apply_change(state, fn):
    pid, old_rec, undo_stack = _validate_state(state)
    new_rec = fn(old_rec)
    next_undo = (undo_stack + [old_rec])[-UNDO_LIMIT:]
    return {"id": pid, "recipe": new_rec, "undo": next_undo}


def new_piece(seed=None):
    if seed is not None and (
        isinstance(seed, bool) or not isinstance(seed, int) or not 0 <= seed < MAX_SEED
    ):
        raise ValueError("seed must be a non-negative integer or null")
    rec = engine.new_recipe(seed)
    return {"id": f"piece_{rec['seed']}", "recipe": rec, "undo": []}


def reroll(state, layer, section):
    return _apply_change(state, lambda rec: engine.reroll(rec, layer, section))


def lock(state, layer, section, locked):
    return _apply_change(state, lambda rec: engine.set_lock(rec, layer, section, locked))


def option(state, name, value):
    return _apply_change(state, lambda rec: engine.set_option(rec, name, value))


def undo(state):
    pid, rec, undo_stack = _validate_state(state)
    if not undo_stack:
        return {"id": pid, "recipe": rec, "undo": []}
    prev_rec = undo_stack.pop()
    return {"id": pid, "recipe": prev_rec, "undo": undo_stack}


def _events_from_piece(piece):
    sec_per_tick = 15.0 / piece["tempo"]
    out = []
    for g_num, g in ((1, "g1"), (2, "g2")):
        for n in piece["parts"][g]:
            out.append({
                "t": float(n["start"] * sec_per_tick),
                "dur": float(n["dur"] * sec_per_tick),
                "pitch": int(n["pitch"]),
                "vel": int(max(30, min(127, n["vel"]))),
                "guitar": g_num,
                "dead": "dead" in n.get("tech", []),
            })
    out.sort(key=lambda e: (e["t"], e["guitar"], e["pitch"]))
    return out


def detail(state):
    pid, rec, undo_stack = _validate_state(state)
    piece = composer.compose(rec)
    return engine.piece_detail(pid, rec, piece, can_undo=bool(undo_stack))


def events(state):
    _, rec, _ = _validate_state(state)
    piece = composer.compose(rec)
    return _events_from_piece(piece)


def snapshot(state):
    pid, rec, undo_stack = _validate_state(state)
    piece = composer.compose(rec)
    return {
        "state": {"id": pid, "recipe": rec, "undo": undo_stack},
        "detail": engine.piece_detail(pid, rec, piece, can_undo=bool(undo_stack)),
        "events": _events_from_piece(piece),
    }


def export_midi(state):
    _, rec, _ = _validate_state(state)
    piece = composer.compose(rec)
    raw = midiio.midi_bytes(piece["tempo"], piece["meters"], composer.midi_tracks(piece))
    return base64.b64encode(raw).decode("ascii")


def export_musicxml(state):
    pid, rec, _ = _validate_state(state)
    piece = composer.compose(rec)
    return musicxml.musicxml_text(piece, pid)


def export_tab(state):
    _, rec, _ = _validate_state(state)
    piece = composer.compose(rec)
    return composer.tab_text(piece)


def handle_json(req_json):
    req = json.loads(req_json)
    cmd = req.get("cmd")
    payload = req.get("payload") or {}
    if cmd == "new_piece":
        return json.dumps(snapshot(new_piece(payload.get("seed"))), ensure_ascii=False)
    if cmd == "reroll":
        return json.dumps(
            snapshot(reroll(payload["state"], payload["layer"], payload["section"])),
            ensure_ascii=False,
        )
    if cmd == "lock":
        return json.dumps(
            snapshot(lock(payload["state"], payload["layer"], payload["section"], payload["locked"])),
            ensure_ascii=False,
        )
    if cmd == "option":
        return json.dumps(
            snapshot(option(payload["state"], payload["name"], payload["value"])),
            ensure_ascii=False,
        )
    if cmd == "undo":
        return json.dumps(snapshot(undo(payload["state"])), ensure_ascii=False)
    if cmd == "snapshot":
        return json.dumps(snapshot(payload["state"]), ensure_ascii=False)
    if cmd == "detail":
        return json.dumps(detail(payload["state"]), ensure_ascii=False)
    if cmd == "events":
        return json.dumps(events(payload["state"]), ensure_ascii=False)
    if cmd == "export_midi":
        return json.dumps(export_midi(payload["state"]), ensure_ascii=False)
    if cmd == "export_musicxml":
        return json.dumps(export_musicxml(payload["state"]), ensure_ascii=False)
    if cmd == "export_tab":
        return json.dumps(export_tab(payload["state"]), ensure_ascii=False)
    raise ValueError(f"unknown command: {cmd}")
