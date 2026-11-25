import base64
import copy
import importlib.util
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import composer
import midiio
import musicxml
from studio import engine as studio_engine
from studio import render as studio_render
from studio import summary as studio_summary

try:
    from studio import web_api
except ImportError:
    web_api = None


class TestWebApi(unittest.TestCase):
    def setUp(self):
        if web_api is None:
            self.fail("studio.web_api is not implemented yet")

    def test_events_match_engine_piece(self):
        st = web_api.new_piece(801000)
        evs = web_api.events(st)

        rec = studio_engine.new_recipe(801000)
        piece = composer.compose(rec)
        sec_per_tick = 15.0 / piece["tempo"]
        expected = []
        for g_num, g in ((1, "g1"), (2, "g2")):
            for n in piece["parts"][g]:
                expected.append({
                    "t": n["start"] * sec_per_tick,
                    "dur": n["dur"] * sec_per_tick,
                    "pitch": int(n["pitch"]),
                    "vel": int(max(30, min(127, n["vel"]))),
                    "guitar": g_num,
                    "dead": "dead" in n.get("tech", []),
                })
        expected.sort(key=lambda e: (e["t"], e["guitar"], e["pitch"]))

        self.assertEqual(len(evs), len(expected))
        self.assertEqual([e["t"] for e in evs], sorted(e["t"] for e in evs))
        for actual, exp in zip(evs, expected):
            self.assertAlmostEqual(actual["t"], exp["t"], places=9)
            self.assertAlmostEqual(actual["dur"], exp["dur"], places=9)
            self.assertEqual(actual["pitch"], exp["pitch"])
            self.assertEqual(actual["vel"], exp["vel"])
            self.assertEqual(actual["guitar"], exp["guitar"])
            self.assertEqual(actual["dead"], exp["dead"])

    def test_fixed_sequence_matches_studio_store(self):
        with tempfile.TemporaryDirectory() as d:
            store = studio_engine.StudioStore(d, render_fn=studio_render.fake_render)
            det = store.create_piece(801000)
            pid = det["id"]
            store.reroll(pid, "comp", "A")
            store.set_lock(pid, "b", "B", True)
            store.set_option(pid, "b_lead", "same_speed")
            store.reroll(pid, "all", "all")
            store.undo(pid)

            st = web_api.new_piece(801000)
            st = web_api.reroll(st, "comp", "A")
            st = web_api.lock(st, "b", "B", True)
            st = web_api.option(st, "b_lead", "same_speed")
            st = web_api.reroll(st, "all", "all")
            st = web_api.undo(st)

            pdir = Path(d) / pid
            store_rec = json.loads((pdir / "recipe.json").read_text(encoding="utf-8"))
            self.assertEqual(st["recipe"], store_rec)
            self.assertEqual(
                base64.b64decode(web_api.export_midi(st)),
                (pdir / "piece.mid").read_bytes(),
            )

    def test_state_json_roundtrip(self):
        def rt(obj):
            return json.loads(json.dumps(obj))

        st = rt(web_api.new_piece(801000))
        snap = copy.deepcopy(st)

        st1 = rt(web_api.reroll(st, "comp", "A"))
        self.assertEqual(st, snap)

        st2 = rt(web_api.lock(st1, "b", "B", True))
        st3 = rt(web_api.option(st2, "b_lead", "same_speed"))
        st4 = rt(web_api.reroll(st3, "all", "all"))
        st5 = rt(web_api.undo(st4))

        det = rt(web_api.detail(st5))
        evs = rt(web_api.events(st5))
        self.assertTrue(det["can_undo"])
        self.assertTrue(len(evs) > 0)
        self.assertTrue(base64.b64decode(web_api.export_midi(st5)).startswith(b"MThd"))
        self.assertIn("<score-partwise", web_api.export_musicxml(st5))
        self.assertIn("== g1 ==", web_api.export_tab(st5))

    def test_undo_stack_bound_50(self):
        st = web_api.new_piece(801000)
        for _ in range(60):
            st = web_api.reroll(st, "comp", "A")

        self.assertEqual(len(st["undo"]), 50)
        self.assertTrue(web_api.detail(st)["can_undo"])
        self.assertEqual(st["recipe"]["studio"]["salts"]["comp@A"], 60)

        for expected_salt in range(59, 9, -1):
            st = web_api.undo(st)
            self.assertEqual(st["recipe"]["studio"]["salts"]["comp@A"], expected_salt)

        self.assertFalse(web_api.detail(st)["can_undo"])
        self.assertEqual(len(st["undo"]), 0)

        st51 = web_api.undo(st)
        self.assertEqual(st51["recipe"], st["recipe"])
        self.assertFalse(web_api.detail(st51)["can_undo"])

    def test_export_midi_musicxml_tab_match_composer_export(self):
        st = web_api.new_piece(801000)
        st = web_api.reroll(st, "comp", "A")
        piece = composer.compose(st["recipe"])

        with tempfile.TemporaryDirectory() as d:
            base = Path(d) / st["id"]
            composer.export(piece, base)
            self.assertEqual(
                base64.b64decode(web_api.export_midi(st)),
                base.with_suffix(".mid").read_bytes(),
            )
            self.assertEqual(
                web_api.export_musicxml(st),
                base.with_suffix(".musicxml").read_text(encoding="utf-8"),
            )
            self.assertEqual(
                web_api.export_tab(st),
                base.with_suffix(".tab.txt").read_text(encoding="utf-8"),
            )

    def test_engine_importable_without_studio_render(self):
        code = (
            "import sys; "
            "sys.modules['studio.render'] = None; "
            "from studio import engine, web_api; "
            "st = web_api.new_piece(801000); "
            "d = web_api.detail(st); "
            "e = web_api.events(st); "
            "m = web_api.export_midi(st); "
            "assert d['seed'] == 801000 and len(e) > 0 and len(m) > 0"
        )
        res = subprocess.run(
            [sys.executable, "-c", code],
            cwd=str(Path(__file__).resolve().parent),
            capture_output=True,
            text=True,
            timeout=30,
        )
        self.assertEqual(res.returncode, 0, res.stderr)

    def test_new_controls_sequence_matches_studio_store(self):
        with tempfile.TemporaryDirectory() as d:
            store = studio_engine.StudioStore(d, render_fn=studio_render.fake_render)
            det = store.create_piece(801000)
            pid = det["id"]
            st = web_api.new_piece(801000)

            steps = [
                ("mute_share", 0.4),
                ("octave_share", 1.0),
                ("intro_bars", 8),
                ("square_repeat", 2),
                ("octave_share", 0.0),
                ("octave_share", None),
            ]
            pdir = Path(d) / pid
            for name, val in steps:
                s_det = store.set_option(pid, name, val)
                st = json.loads(json.dumps(web_api.option(st, name, val)))
                w_det = web_api.detail(st)
                self.assertEqual(w_det, s_det, (name, val))
                self.assertEqual(
                    st["recipe"],
                    json.loads((pdir / "recipe.json").read_text(encoding="utf-8")),
                    (name, val),
                )
                self.assertEqual(
                    base64.b64decode(web_api.export_midi(st)),
                    (pdir / "piece.mid").read_bytes(),
                    (name, val),
                )

            s_det = store.undo(pid)
            st = json.loads(json.dumps(web_api.undo(st)))
            self.assertEqual(web_api.detail(st), s_det)
            self.assertEqual(base64.b64decode(web_api.export_midi(st)), (pdir / "piece.mid").read_bytes())

    def test_new_controls_validation(self):
        st = web_api.new_piece(801000)
        for name, bad in (
            ("mute_share", True),
            ("mute_share", "0.5"),
            ("octave_share", False),
            ("intro_bars", 3),
            ("intro_bars", True),
            ("intro_bars", "8"),
            ("square_repeat", 3),
            ("square_repeat", True),
        ):
            with self.assertRaises(ValueError, msg=(name, bad)):
                web_api.option(st, name, bad)
            bad_st = copy.deepcopy(st)
            bad_st["recipe"]["studio"]["options"][name] = bad
            with self.assertRaises(ValueError, msg=("validate", name, bad)):
                web_api.detail(bad_st)

        for name, out_of_range in (("mute_share", -0.1), ("mute_share", 1.5), ("octave_share", 2.0)):
            bad_st = copy.deepcopy(st)
            bad_st["recipe"]["studio"]["options"][name] = out_of_range
            with self.assertRaises(ValueError, msg=("validate", name, out_of_range)):
                web_api.detail(bad_st)
        self.assertEqual(web_api.option(st, "mute_share", -0.1)["recipe"]["studio"]["options"]["mute_share"], 0.0)
        self.assertEqual(web_api.option(st, "octave_share", 2.0)["recipe"]["studio"]["options"]["octave_share"], 1.0)

    def test_square_repeat_repeats_disjoint_a_runs_independently(self):
        import xml.etree.ElementTree as ET

        st = web_api.new_piece(802000)
        form = st["recipe"]["form"]
        a_bars = [copy.deepcopy(b) for b in form if b["sec"] == "A"]
        b_end = max(i for i, b in enumerate(form) if b["sec"] == "B")
        st["recipe"]["form"] = form[:b_end + 1] + a_bars + form[b_end + 1:]

        snap1 = web_api.snapshot(st)
        b_secs_1 = [s for s in snap1["detail"]["sections"] if s["section"] == "B"]
        self.assertEqual(len(b_secs_1), 1)
        b_dur_1 = b_secs_1[0]["end16"] - b_secs_1[0]["start16"]

        st2 = web_api.option(st, "square_repeat", 2)
        snap2 = web_api.snapshot(st2)
        b_secs_2 = [s for s in snap2["detail"]["sections"] if s["section"] == "B"]
        self.assertEqual(len(b_secs_2), 1)
        self.assertEqual(b_secs_2[0]["end16"] - b_secs_2[0]["start16"], b_dur_1)

        a_secs_1 = [s for s in snap1["detail"]["sections"] if s["section"] == "A"]
        a_secs_2 = [s for s in snap2["detail"]["sections"] if s["section"] == "A"]
        self.assertEqual(len(a_secs_2), 2)
        for s1, s2 in zip(a_secs_1, a_secs_2):
            self.assertEqual(s2["end16"] - s2["start16"], 2 * (s1["end16"] - s1["start16"]))

        root = ET.fromstring(web_api.export_musicxml(st2))
        for part in root.iter("part"):
            num = den = None
            for m in part.iter("measure"):
                t = m.find("attributes/time")
                if t is not None:
                    num, den = int(t.findtext("beats")), int(t.findtext("beat-type"))
                total = sum(int(n.findtext("duration")) for n in m.iter("note") if n.find("chord") is None)
                self.assertEqual(total, num * 16 // den, (part.get("id"), m.get("number")))

    def test_noop_option_does_not_push_undo(self):
        st0 = web_api.new_piece(801000)
        for name, val in (
            ("intro_bars", "auto"),
            ("square_repeat", 1),
            ("mute_share", 0.0),
            ("octave_share", None),
            ("b_lead", "augmented"),
            ("trailing_rest", st0["recipe"]["studio"]["options"]["trailing_rest"]),
            ("octave_piece", composer.V8["octave_piece"]),
            ("slide_max", composer.V8["slide_max"]),
            ("mute_share", -0.1),
        ):
            st1 = web_api.option(st0, name, val)
            self.assertEqual(st1["undo"], [], (name, val))
            self.assertEqual(st1["recipe"], st0["recipe"], (name, val))
            self.assertFalse(web_api.detail(st1)["can_undo"], (name, val))

        st_changed = web_api.option(st0, "intro_bars", 8)
        self.assertEqual(len(st_changed["undo"]), 1)

        st_same = web_api.option(st_changed, "intro_bars", 8)
        self.assertEqual(len(st_same["undo"]), 1)
        self.assertEqual(st_same["recipe"], st_changed["recipe"])

        st_undone = web_api.undo(st_same)
        self.assertEqual(st_undone["recipe"]["studio"]["options"]["intro_bars"], "auto")
        self.assertEqual(st_undone["undo"], [])

        legacy_st = copy.deepcopy(st0)
        for key in ("mute_share", "intro_bars", "square_repeat", "octave_share"):
            legacy_st["recipe"]["studio"]["options"].pop(key, None)
        legacy_after = web_api.option(legacy_st, "intro_bars", "auto")
        self.assertEqual(legacy_after["undo"], [])


class TestWebApiKey(unittest.TestCase):
    D_MINOR = {"tonic": "D", "mode": "aeolian"}

    def test_key_sequence_with_undo_and_redo_matches_studio_store(self):
        with tempfile.TemporaryDirectory() as d:
            store = studio_engine.StudioStore(d, render_fn=studio_render.fake_render)
            pid = store.create_piece(801000)["id"]
            pdir = Path(d) / pid
            st = web_api.new_piece(801000)
            steps = [
                ("option", ("key", self.D_MINOR)),
                ("option", ("key", {"tonic": "F#", "mode": "locrian"})),
                ("option", ("intro_bars", 4)),
                ("option", ("key", None)),
                ("undo", ()),
                ("undo", ()),
                ("redo", ()),
            ]
            for kind, args in steps:
                s_det = getattr(store, "set_option" if kind == "option" else kind)(pid, *args)
                st = json.loads(json.dumps(getattr(web_api, kind)(st, *args)))
                self.assertEqual(web_api.detail(st), s_det, (kind, args))
                self.assertEqual(st["recipe"], json.loads((pdir / "recipe.json").read_text(encoding="utf-8")), (kind, args))
                self.assertEqual(
                    base64.b64decode(web_api.export_midi(st)), (pdir / "piece.mid").read_bytes(), (kind, args)
                )

    def test_key_detail_labels_and_handle_json(self):
        st = web_api.new_piece(801000)
        own = {"tonic": web_api.detail(st)["key"], "mode": web_api.detail(st)["mode"]}
        self.assertEqual(web_api.option(st, "key", own)["undo"], [])
        st1 = web_api.option(st, "key", self.D_MINOR)
        det = web_api.detail(st1)
        self.assertEqual((det["key"], det["mode"]), ("D", "aeolian"))
        self.assertEqual(det["undo_label"], "тональность: D натуральный минор")
        out = json.loads(web_api.handle_json(json.dumps({"cmd": "option", "payload": {"state": st, "name": "key", "value": self.D_MINOR}})))
        self.assertEqual(out["detail"]["options"]["key"], self.D_MINOR)
        undone = web_api.undo(st1)
        self.assertEqual(web_api.detail(undone)["redo_label"], "тональность: D натуральный минор")
        self.assertIsNone(web_api.detail(undone)["options"]["key"])

    def test_key_validation_on_option_and_on_imported_state(self):
        st = web_api.new_piece(801000)
        for bad in ("D", {"tonic": "D"}, {"tonic": "D", "mode": "blues"}, {"tonic": "H", "mode": "aeolian"},
                    {"tonic": "D", "mode": "aeolian", "x": 1}, True, 3, []):
            with self.assertRaises(ValueError, msg=bad):
                web_api.option(st, "key", bad)
            bad_st = copy.deepcopy(st)
            bad_st["recipe"]["studio"]["options"]["key"] = bad
            with self.assertRaises(ValueError, msg=("validate", bad)):
                web_api.detail(bad_st)
        ok_st = copy.deepcopy(st)
        ok_st["recipe"]["studio"]["options"]["key"] = self.D_MINOR
        self.assertEqual(web_api.detail(ok_st)["key"], "D")
        legacy = copy.deepcopy(st)
        del legacy["recipe"]["studio"]["options"]["key"]
        self.assertIsNone(web_api.detail(legacy)["options"]["key"])


    def test_import_turns_an_explicit_own_key_into_unset(self):
        st = web_api.new_piece(801000)
        det = web_api.detail(st)
        explicit = copy.deepcopy(st)
        explicit["recipe"]["studio"]["options"]["key"] = {"tonic": det["key"], "mode": det["mode"]}
        explicit["undo"] = [copy.deepcopy(explicit["recipe"])]
        snap = web_api.import_state(explicit)
        self.assertIsNone(snap["state"]["recipe"]["studio"]["options"]["key"])
        self.assertIsNone(snap["detail"]["options"]["key"])
        self.assertIsNone(snap["state"]["undo"][0]["studio"]["options"]["key"])
        same = web_api.option(snap["state"], "key", None)
        self.assertEqual(len(same["undo"]), 1)


class TestReviewFixesPython(unittest.TestCase):
    def _load_check_site(self):
        path = Path(__file__).resolve().parent / "web" / "check_site.py"
        spec = importlib.util.spec_from_file_location("check_site", path)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        return mod

    def _make_minimal_web_dir(self, root: Path, samples_base="./samples/", py_base="./py/", sample_fname="p040_v072.mp3"):
        root.mkdir(parents=True, exist_ok=True)
        (root / "index.html").write_text('<!DOCTYPE html><script src="./app.js"></script>', encoding="utf-8")
        (root / "app.js").write_text(
            f'const SAMPLES_BASE = "{samples_base}";\nconst w = new Worker("./worker.js");\n',
            encoding="utf-8",
        )
        (root / "worker.js").write_text(
            f'const PY_BASE = "{py_base}";\nconst PY_FILES = ["web_api.py"];\n',
            encoding="utf-8",
        )
        py_dir = root / "py"
        py_dir.mkdir(parents=True, exist_ok=True)
        (py_dir / "web_api.py").write_text("# stub\n", encoding="utf-8")
        sdir = root / "samples"
        sdir.mkdir(parents=True, exist_ok=True)
        (sdir / "fsbs_LICENSE.txt").write_text("CC0", encoding="utf-8")
        files_map = {}
        for pitch in range(40, 89):
            files_map[str(pitch)] = {"72": sample_fname, "100": sample_fname}
        (sdir / sample_fname).write_bytes(b"MP3DATA")
        (sdir / "manifest.json").write_text(
            json.dumps({"license": "fsbs_LICENSE.txt", "files": files_map}),
            encoding="utf-8",
        )

    def test_summary_accepts_downloaded_jsonl_file_grouped_by_piece_id(self):
        rows = [
            {"t": "2026-11-23T00:00:00+00:00", "action": "new", "layer": None, "section": None, "salt": None, "value": 801000, "piece_id": "piece_1"},
            {"t": "2026-11-23T00:00:01+00:00", "action": "reroll", "layer": "comp", "section": "A", "salt": 1, "value": None, "piece_id": "piece_1"},
            {"t": "2026-11-23T00:00:02+00:00", "action": "new", "layer": None, "section": None, "salt": None, "value": 802000, "piece_id": "piece_2"},
            {"t": "2026-11-23T00:00:03+00:00", "action": "option", "layer": None, "section": None, "salt": None, "value": {"b_lead": "rest"}, "piece_id": "piece_2"},
            {"t": "2026-11-23T00:00:04+00:00", "action": "export", "layer": None, "section": None, "salt": None, "value": "piece.mid", "piece_id": "piece_1"},
            {"t": "2026-11-23T00:00:05+00:00", "action": "reroll", "layer": "b", "section": "B", "salt": 1, "value": None, "piece_id": "piece_2"},
            {"t": "2026-11-23T00:00:06+00:00", "action": "reroll", "layer": "tempo", "section": "all", "salt": 1, "value": None, "piece_id": "piece_2"},
            {"t": "2026-11-23T00:00:07+00:00", "action": "export", "layer": None, "section": None, "salt": None, "value": "piece.musicxml", "piece_id": "piece_2"},
        ]
        with tempfile.TemporaryDirectory() as d:
            jpath = Path(d) / "history.jsonl"
            jpath.write_text("\n".join(json.dumps(r) for r in rows) + "\n", encoding="utf-8")
            stats = studio_summary.summarize(jpath)
        self.assertEqual(stats["pieces_total"], 2)
        self.assertEqual(stats["pieces_exported"], 2)
        self.assertEqual(stats["rerolls_before_first_export"], [1, 2])
        self.assertEqual(stats["rerolls_per_layer"], {"comp": 1, "b": 1, "tempo": 1})
        self.assertEqual(stats["options_used"], {"b_lead=rest": 1, "b_lead": 1})

    def test_summary_directory_with_root_or_child_downloaded_jsonl(self):
        rows = [
            {"t": "2026-11-23T00:00:00+00:00", "action": "reroll", "layer": "comp", "section": "A", "salt": 1, "value": None, "piece_id": "pA"},
            {"t": "2026-11-23T00:00:01+00:00", "action": "export", "layer": None, "section": None, "salt": None, "value": "piece.mid", "piece_id": "pA"},
            {"t": "2026-11-23T00:00:02+00:00", "action": "reroll", "layer": "b", "section": "B", "salt": 1, "value": None, "piece_id": "pB"},
        ]
        text = "\n".join(json.dumps(r) for r in rows) + "\n"
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / "history.jsonl").write_text(text, encoding="utf-8")
            s_root = studio_summary.summarize(root)
            self.assertEqual(s_root["pieces_total"], 2)
            self.assertEqual(s_root["pieces_exported"], 1)

        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / "web").mkdir()
            (root / "web" / "history.jsonl").write_text(text, encoding="utf-8")
            s_child = studio_summary.summarize(root)
            self.assertEqual(s_child["pieces_total"], 2)
            self.assertEqual(s_child["pieces_exported"], 1)

    def test_summary_preserves_per_piece_directory_layout(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / "piece_1").mkdir()
            (root / "piece_1" / "history.jsonl").write_text(
                json.dumps({"t": "2026-11-23T00:00:00+00:00", "action": "reroll", "layer": "comp", "section": "A", "salt": 1, "value": None}) + "\n"
                + json.dumps({"t": "2026-11-23T00:00:01+00:00", "action": "export", "layer": None, "section": None, "salt": None, "value": "piece.mid"}) + "\n",
                encoding="utf-8",
            )
            (root / "piece_2").mkdir()
            (root / "piece_2" / "history.jsonl").write_text(
                json.dumps({"t": "2026-11-23T00:00:02+00:00", "action": "new", "layer": None, "section": None, "salt": None, "value": 802000}) + "\n",
                encoding="utf-8",
            )
            stats = studio_summary.summarize(root)
        self.assertEqual(stats["pieces_total"], 2)
        self.assertEqual(stats["pieces_exported"], 1)
        self.assertEqual(stats["rerolls_before_first_export"], [1])

    def test_rejects_arbitrary_section_names_in_recipe_and_undo(self):
        st = web_api.new_piece(801000)
        bad = copy.deepcopy(st)
        bad["recipe"]["form"][0]["sec"] = "<img src=x onerror=alert(1)>"
        for fn in (web_api.snapshot, web_api.detail, web_api.events):
            with self.assertRaises(ValueError):
                fn(bad)

        bad_undo = copy.deepcopy(st)
        bad_undo_rec = copy.deepcopy(st["recipe"])
        bad_undo_rec["form"][0]["sec"] = "<svg onload=alert(1)>"
        bad_undo["undo"] = [bad_undo_rec]
        with self.assertRaises(ValueError):
            web_api.snapshot(bad_undo)

    def test_rejects_invalid_piece_id(self):
        st = web_api.new_piece(801000)
        for bad_id in ("../bad", "<script>", "piece with spaces"):
            bad = copy.deepcopy(st)
            bad["id"] = bad_id
            with self.assertRaises(ValueError):
                web_api.snapshot(bad)

    def test_accepts_and_bounds_oversized_undo_stack_to_50(self):
        st = web_api.new_piece(801000)
        base_rec = copy.deepcopy(st["recipe"])
        undo_entries = []
        for i in range(53):
            r = copy.deepcopy(base_rec)
            r["studio"]["salts"]["comp@A"] = i
            undo_entries.append(r)
        cur_rec = copy.deepcopy(base_rec)
        cur_rec["studio"]["salts"]["comp@A"] = 53
        oversized = {"id": st["id"], "recipe": cur_rec, "undo": undo_entries}

        snap = web_api.snapshot(oversized)
        self.assertEqual(len(snap["state"]["undo"]), 50)
        self.assertEqual(snap["state"]["undo"][0]["studio"]["salts"]["comp@A"], 3)
        self.assertEqual(snap["state"]["undo"][-1]["studio"]["salts"]["comp@A"], 52)

        after_undo = web_api.undo(oversized)
        self.assertEqual(len(after_undo["undo"]), 49)
        self.assertEqual(after_undo["recipe"]["studio"]["salts"]["comp@A"], 52)

    def test_rejects_malformed_undo_entries(self):
        st = web_api.new_piece(801000)
        for bad_undo in ([{"not": "a recipe"}], ["bad"]):
            bad = copy.deepcopy(st)
            bad["undo"] = bad_undo
            with self.assertRaises(ValueError):
                web_api.snapshot(bad)

    def test_check_site_detects_broken_samples_base(self):
        check_site = self._load_check_site()
        with tempfile.TemporaryDirectory() as d:
            tmp_web = Path(d) / "web"
            self._make_minimal_web_dir(tmp_web, samples_base="./nonexistent/")
            self.assertNotEqual(check_site.check(tmp_web), 0)

    def test_check_site_detects_path_traversal_in_py_and_manifest(self):
        check_site = self._load_check_site()
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / "web_api.py").write_text("# outside\n", encoding="utf-8")
            tmp_web = root / "web"
            self._make_minimal_web_dir(tmp_web, py_base="../")
            self.assertNotEqual(check_site.check(tmp_web), 0)

        with tempfile.TemporaryDirectory() as d:
            tmp_web = Path(d) / "web"
            self._make_minimal_web_dir(tmp_web, sample_fname="../index.html")
            self.assertNotEqual(check_site.check(tmp_web), 0)

    def test_write_midi_does_not_truncate_on_failure(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "existing.mid"
            path.write_bytes(b"KEEP-MIDI")
            with self.assertRaises(KeyError):
                midiio.write_midi(path, 120, [(0, 4, 99)], [])
            self.assertEqual(path.read_bytes(), b"KEEP-MIDI")

    def test_write_musicxml_does_not_truncate_on_failure(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "existing.musicxml"
            path.write_text("KEEP-XML", encoding="utf-8")
            with self.assertRaises(KeyError):
                musicxml.write_musicxml({"bad": True}, path, "t")
            self.assertEqual(path.read_text(encoding="utf-8"), "KEEP-XML")


class TestRedo(unittest.TestCase):
    def setUp(self):
        if web_api is None:
            self.fail("studio.web_api is not implemented yet")

    @staticmethod
    def _rt(obj):
        return json.loads(json.dumps(obj))

    def test_new_piece_has_empty_redo(self):
        st = web_api.new_piece(801000)
        self.assertEqual(st["redo"], [])
        det = web_api.detail(st)
        self.assertFalse(det["can_redo"])
        self.assertIsNone(det["redo_label"])
        self.assertIsNone(det["undo_label"])

    def test_undo_then_redo_restores_the_undone_state(self):
        st0 = web_api.new_piece(801000)
        st1 = web_api.reroll(st0, "comp", "A")
        st2 = web_api.option(st1, "mute_share", 0.3)
        undone = web_api.undo(st2)
        self.assertEqual(undone["recipe"], st1["recipe"])
        self.assertEqual(undone["redo"], [st2["recipe"]])
        self.assertTrue(web_api.detail(undone)["can_redo"])

        redone = web_api.redo(self._rt(undone))
        self.assertEqual(redone["recipe"], st2["recipe"])
        self.assertEqual(redone["undo"], st2["undo"])
        self.assertEqual(redone["redo"], [])
        self.assertFalse(web_api.detail(redone)["can_redo"])

    def test_multiple_undos_redo_in_reverse_order(self):
        st = web_api.new_piece(801000)
        recipes = [st["recipe"]]
        for _ in range(3):
            st = web_api.reroll(st, "comp", "A")
            recipes.append(st["recipe"])
        for _ in range(3):
            st = web_api.undo(st)
        self.assertEqual(st["recipe"], recipes[0])
        self.assertEqual(len(st["redo"]), 3)
        for expected in recipes[1:]:
            st = web_api.redo(st)
            self.assertEqual(st["recipe"], expected)
        self.assertEqual(st["redo"], [])

    def test_new_change_clears_redo_but_a_noop_change_does_not(self):
        st = web_api.new_piece(801000)
        st = web_api.reroll(st, "comp", "A")
        st = web_api.undo(st)
        self.assertEqual(len(st["redo"]), 1)

        same = web_api.option(st, "intro_bars", "auto")
        self.assertEqual(same["redo"], st["redo"])
        self.assertEqual(same["undo"], st["undo"])

        self.assertEqual(web_api.option(st, "mute_share", 0.5)["redo"], [])
        self.assertEqual(web_api.reroll(st, "comp", "A")["redo"], [])
        self.assertEqual(web_api.lock(st, "b", "B", True)["redo"], [])

    def test_undo_and_redo_on_empty_stacks_are_noops(self):
        st = web_api.new_piece(801000)
        self.assertEqual(web_api.undo(st), st)
        self.assertEqual(web_api.redo(st), st)
        st = web_api.undo(web_api.reroll(st, "comp", "A"))
        again = web_api.undo(st)
        self.assertEqual(again["recipe"], st["recipe"])
        self.assertEqual(again["redo"], st["redo"])

    def test_redo_stack_is_bounded_to_50(self):
        st = web_api.new_piece(801000)
        for _ in range(60):
            st = web_api.reroll(st, "comp", "A")
        for _ in range(50):
            st = web_api.undo(st)
        self.assertEqual(len(st["redo"]), 50)
        self.assertEqual(st["recipe"]["studio"]["salts"]["comp@A"], 10)

        oversized = copy.deepcopy(st)
        extra = []
        for n in range(1, 61):
            rec = copy.deepcopy(st["recipe"])
            rec["studio"]["salts"]["comp@A"] = 100 + n
            extra.append(rec)
        oversized["redo"] = extra
        snap = web_api.snapshot(oversized)
        self.assertEqual(len(snap["state"]["redo"]), 50)
        self.assertEqual(snap["state"]["redo"][0]["studio"]["salts"]["comp@A"], 111)
        self.assertEqual(snap["state"]["redo"][-1]["studio"]["salts"]["comp@A"], 160)

    def test_unchanged_lock_requests_keep_history_and_redo(self):
        st = web_api.new_piece(801000)
        st = web_api.lock(st, "comp", "A", True)
        st = web_api.lock(st, "b", "B", True)
        st = web_api.option(st, "mute_share", 0.3)
        st = web_api.undo(st)
        self.assertTrue(st["redo"])
        for layer, section, locked in (("comp", "A", True), ("ending", "end", False), ("comp", "A2", False)):
            again = web_api.lock(self._rt(st), layer, section, locked)
            self.assertEqual(again["recipe"], st["recipe"])
            self.assertEqual(again["undo"], st["undo"])
            self.assertEqual(again["redo"], st["redo"])
        self.assertNotIn("locked_riffs", st["recipe"]["studio"])

    def test_import_state_rejects_unusable_history_entries(self):
        st = web_api.undo(web_api.reroll(web_api.new_piece(801000), "comp", "A"))
        st = web_api.reroll(st, "comp", "A")
        st = web_api.undo(web_api.reroll(st, "texture", "A"))
        self.assertEqual(web_api.import_state(self._rt(st))["state"]["redo"], st["redo"])
        for key in ("undo", "redo"):
            broken = self._rt(st)
            del broken[key][-1]["intent"]["grouping"]
            with self.assertRaisesRegex(ValueError, key):
                web_api.import_state(broken)
        out = json.loads(web_api.handle_json(json.dumps({"cmd": "import_state", "payload": {"state": st}})))
        self.assertEqual(out["state"]["redo"], st["redo"])

    def test_undo_stack_is_bounded_to_50(self):
        st = web_api.new_piece(801000)
        for _ in range(60):
            st = web_api.reroll(st, "comp", "A")
        self.assertEqual(len(st["undo"]), 50)

    def test_redo_entries_are_validated_and_state_without_redo_is_accepted(self):
        st = web_api.new_piece(801000)
        for bad in ([{"not": "a recipe"}], ["bad"], "nope"):
            broken = copy.deepcopy(st)
            broken["redo"] = bad
            with self.assertRaises(ValueError):
                web_api.snapshot(broken)
        legacy = {"id": st["id"], "recipe": st["recipe"], "undo": []}
        self.assertEqual(web_api.snapshot(legacy)["state"]["redo"], [])
        self.assertEqual(web_api.redo(legacy)["redo"], [])

    def test_handle_json_redo_and_snapshot_carry_redo(self):
        st = web_api.undo(web_api.reroll(web_api.new_piece(801000), "comp", "A"))
        out = json.loads(web_api.handle_json(json.dumps({"cmd": "redo", "payload": {"state": st}})))
        self.assertEqual(out["state"]["recipe"]["studio"]["salts"]["comp@A"], 1)
        self.assertEqual(out["state"]["redo"], [])
        self.assertTrue(out["detail"]["can_undo"])
        self.assertFalse(out["detail"]["can_redo"])
        snap = json.loads(web_api.handle_json(json.dumps({"cmd": "snapshot", "payload": {"state": st}})))
        self.assertEqual(snap["state"]["redo"], st["redo"])
        self.assertTrue(snap["detail"]["can_redo"])

    def test_matches_the_server_store_sequence(self):
        with tempfile.TemporaryDirectory() as d:
            store = studio_engine.StudioStore(d, render_fn=studio_render.fake_render)
            pid = store.create_piece(801000)["id"]
            store.reroll(pid, "comp", "A")
            store.set_option(pid, "b_lead", "rest")
            store.undo(pid)
            store.undo(pid)
            store.redo(pid)

            st = web_api.new_piece(801000)
            st = web_api.reroll(st, "comp", "A")
            st = web_api.option(st, "b_lead", "rest")
            st = web_api.undo(web_api.undo(st))
            st = web_api.redo(st)
            pdir = store._piece_dir(pid)
            self.assertEqual(st["recipe"], json.loads((pdir / "recipe.json").read_text(encoding="utf-8")))
            self.assertEqual(st["redo"], json.loads((pdir / "redo.json").read_text(encoding="utf-8")))

    def test_labels_name_the_last_action(self):
        st = web_api.new_piece(801000)
        st = web_api.option(st, "mute_share", 0.3)
        self.assertEqual(web_api.detail(st)["undo_label"], "глухие ноты 0.3")
        st = web_api.reroll(st, "comp", "A")
        self.assertEqual(web_api.detail(st)["undo_label"], "перегенерация «Куплет A · ритм-гитара»")
        st = web_api.reroll(st, "lead_rhythm", "all")
        self.assertEqual(web_api.detail(st)["undo_label"], "перегенерация «Вся пьеса · новый риф»")
        st = web_api.lock(st, "comp", "A", True)
        self.assertEqual(web_api.detail(st)["undo_label"], "закрепление «Куплет A · ритм-гитара»")
        st = web_api.lock(st, "comp", "A", False)
        self.assertEqual(web_api.detail(st)["undo_label"], "снятие закрепления «Куплет A · ритм-гитара»")
        st = web_api.reroll(st, "all", "all")
        self.assertEqual(web_api.detail(st)["undo_label"], "перегенерация всего незакреплённого")
        st = web_api.option(st, "trailing_rest", False)
        self.assertEqual(web_api.detail(st)["undo_label"], "без паузы в конце такта")
        st = web_api.option(st, "b_lead", "rest")
        self.assertEqual(web_api.detail(st)["undo_label"], "соло в середине B: без соло")
        st = web_api.option(st, "octave_share", 0.4)
        st = web_api.option(st, "octave_share", None)
        self.assertEqual(web_api.detail(st)["undo_label"], "доля октав: авто")

        undone = web_api.undo(st)
        self.assertEqual(web_api.detail(undone)["redo_label"], "доля октав: авто")
        self.assertEqual(web_api.detail(undone)["undo_label"], "доля октав 0.4")
        self.assertIsNone(web_api.detail(web_api.redo(undone))["redo_label"])


class TestWebApiMarks(unittest.TestCase):
    def setUp(self):
        if web_api is None:
            self.fail("studio.web_api is not implemented yet")

    @staticmethod
    def _rt(obj):
        return json.loads(json.dumps(obj))

    def test_new_piece_and_legacy_state_have_no_marks(self):
        st = web_api.new_piece(801000)
        self.assertEqual(st["marks"], [])
        self.assertEqual(web_api.detail(st)["marks"], [])
        legacy = {k: v for k, v in st.items() if k != "marks"}
        snap = web_api.snapshot(legacy)
        self.assertEqual(snap["state"]["marks"], [])
        self.assertEqual(snap["detail"]["marks"], [])
        self.assertEqual(web_api.import_state(legacy)["state"]["marks"], [])

    def test_marks_leave_the_music_and_both_stacks_alone(self):
        st = web_api.new_piece(801000)
        st = web_api.reroll(st, "comp", "A")
        st = web_api.reroll(st, "texture", "A")
        st = web_api.undo(st)
        midi = web_api.export_midi(st)
        before = {k: self._rt(st[k]) for k in ("id", "recipe", "undo", "redo")}
        self.assertEqual(len(before["redo"]), 1)

        res = web_api.mark_add(st, 4.0, "rhythm", "кривая нота")
        mark = res["mark"]
        self.assertEqual([m["id"] for m in res["state"]["marks"]], [mark["id"]])
        self.assertEqual([m["id"] for m in res["detail"]["marks"]], [mark["id"]])
        res = web_api.mark_edit(res["state"], mark["id"], guitar="lead", text="лишняя пауза")
        self.assertEqual((res["mark"]["guitar"], res["mark"]["text"]), ("lead", "лишняя пауза"))
        res = web_api.mark_add(res["state"], 9.0)
        res = web_api.mark_delete(res["state"], mark["id"])
        self.assertEqual(len(res["state"]["marks"]), 1)
        after = res["state"]
        self.assertEqual({k: after[k] for k in ("id", "recipe", "undo", "redo")}, before)
        self.assertEqual(web_api.export_midi(after), midi)
        self.assertTrue(res["detail"]["can_redo"])
        redone = web_api.redo(after)
        self.assertEqual(redone["marks"], after["marks"])

    def test_marks_survive_every_music_change_and_stay_in_the_state(self):
        st = web_api.mark_add(web_api.new_piece(801000), 4.0)["state"]
        ids = [m["id"] for m in st["marks"]]
        for step in (
            lambda s: web_api.reroll(s, "comp", "A"),
            lambda s: web_api.lock(s, "b", "B", True),
            lambda s: web_api.option(s, "mute_share", 0.3),
            lambda s: web_api.undo(s),
            lambda s: web_api.redo(s),
            lambda s: web_api.snapshot(s)["state"],
        ):
            st = step(st)
            self.assertEqual([m["id"] for m in st["marks"]], ids)

    def test_state_json_and_import_round_trip_marks(self):
        st = web_api.mark_add(web_api.new_piece(801022), 21.4, "lead", "пауза")["state"]
        st = web_api.reroll(st, "comp", "A")
        wire = self._rt(st)
        snap = web_api.import_state(wire)
        self.assertEqual(snap["state"]["marks"], st["marks"])
        self.assertEqual(snap["detail"]["marks"][0]["stale"], True)
        again = json.loads(web_api.handle_json(json.dumps({"cmd": "snapshot", "payload": {"state": wire}})))
        self.assertEqual(again["state"]["marks"], st["marks"])

    def test_stale_after_reroll_and_fresh_after_undo(self):
        st = web_api.mark_add(web_api.new_piece(801000), 4.0)["state"]
        self.assertFalse(web_api.detail(st)["marks"][0]["stale"])
        st = web_api.reroll(st, "comp", "A")
        self.assertTrue(web_api.detail(st)["marks"][0]["stale"])
        st = web_api.undo(st)
        self.assertFalse(web_api.detail(st)["marks"][0]["stale"])

    def test_invalid_marks_and_requests_are_rejected(self):
        st = web_api.new_piece(801000)
        for args in (("x",), (4.0, "bass"), (4.0, "both", "я" * 81), (-1.0,), (9999.0,)):
            with self.assertRaises(ValueError):
                web_api.mark_add(st, *args)
        mark = web_api.mark_add(st, 4.0)["mark"]
        with_mark = web_api.mark_add(st, 4.0)["state"]
        with self.assertRaises(KeyError):
            web_api.mark_edit(with_mark, "nope", text="x")
        with self.assertRaises(KeyError):
            web_api.mark_delete(with_mark, "nope")
        bad = self._rt(with_mark)
        bad["marks"][0]["guitar"] = "bass"
        with self.assertRaises(ValueError):
            web_api.snapshot(bad)
        bad = self._rt(with_mark)
        bad["marks"] = {"not": "a list"}
        with self.assertRaises(ValueError):
            web_api.snapshot(bad)
        self.assertTrue(mark["id"])

    def test_handle_json_commands_and_export(self):
        st = web_api.new_piece(801000)

        def call(cmd, **payload):
            return json.loads(web_api.handle_json(json.dumps({"cmd": cmd, "payload": payload})))

        res = call("mark_add", state=st, t=4.0, guitar="lead", text="а")
        mid = res["mark"]["id"]
        res = call("mark_edit", state=res["state"], id=mid, text="б")
        self.assertEqual(res["mark"]["text"], "б")
        exported = call("export_marks", state=res["state"])
        self.assertEqual(exported["seed"], 801000)
        self.assertEqual(exported["id"], st["id"])
        self.assertEqual(exported["recipe"], st["recipe"])
        self.assertEqual([m["text"] for m in exported["marks"]], ["б"])
        res = call("mark_delete", state=res["state"], id=mid)
        self.assertEqual(res["mark"]["id"], mid)
        self.assertEqual(res["state"]["marks"], [])


class TestWebApiMarksReview(unittest.TestCase):
    def setUp(self):
        if web_api is None:
            self.fail("studio.web_api is not implemented yet")

    @staticmethod
    def _rt(obj):
        return json.loads(json.dumps(obj))

    def _with_mark(self, seed=801000):
        st = web_api.mark_add(web_api.new_piece(seed), 4.0, "lead", "т")["state"]
        return self._rt(st)

    def test_malformed_imported_marks_are_rejected_with_a_russian_message(self):
        def broken(change):
            st = self._with_mark()
            change(st["marks"])
            return st

        cases = {
            "notes": lambda m: m[0].update(notes=[{}]),
            "note type": lambda m: m[0].update(notes=[{"guitar": "1", "pitch": 60, "start": 0, "dur": 1, "string": 1, "fret": 0, "tech": []}]),
            "time": lambda m: m[0].update(t=1000),
            "negative time": lambda m: m[0].update(t=-1),
            "bar": lambda m: m[0].update(bar=m[0]["bar"] + 1),
            "sixteenth": lambda m: m[0].update(sixteenth=m[0]["sixteenth"] % 16 + 1),
            "guitar": lambda m: m[0].update(guitar="bass"),
            "text": lambda m: m[0].update(text="я" * 81),
            "too many notes": lambda m: m[0].update(notes=[
                {"guitar": 1, "pitch": 60, "start": 0, "dur": 1, "string": 1, "fret": 0, "tech": []}] * 65),
            "too many marks": lambda m: m.extend(
                [dict(m[0], id=f"x{i}") for i in range(200)]),
            "duplicate id": lambda m: m.append(dict(m[0])),
        }
        for name, change in cases.items():
            with self.subTest(case=name):
                with self.assertRaisesRegex(ValueError, "[А-Яа-я]"):
                    web_api.import_state(broken(change))
        good = self._with_mark()
        good["marks"][0]["notes"] = [
            {"guitar": 2, "pitch": 60, "start": 0, "dur": 2, "string": None, "fret": None, "tech": ["pm"]}
        ] * 64
        self.assertEqual(len(web_api.import_state(good)["state"]["marks"][0]["notes"]), 64)

    def test_stale_imported_marks_keep_their_time_but_still_get_a_bounded_audition(self):
        st = self._with_mark()
        st["marks"][0]["rev"] = "0" * 12
        st["marks"][0]["t"] = 1000
        marks = web_api.import_state(st)["detail"]["marks"]
        self.assertTrue(marks[0]["stale"])
        self.assertFalse(marks[0]["can_audition"])
        self.assertLessEqual(marks[0]["from_sec"], marks[0]["to_sec"])

    def test_added_mark_keeps_a_client_id_and_the_audible_revision(self):
        st = web_api.new_piece(801000)
        rev0 = web_api.detail(st)["rev"]
        st = web_api.reroll(st, "tempo", "all")
        res = web_api.mark_add(st, 10.0, "both", "", "mclient1", rev0)
        self.assertEqual(res["mark"]["id"], "mclient1")
        self.assertEqual(res["mark"]["rev"], rev0)
        self.assertTrue(res["detail"]["marks"][0]["stale"])
        old_piece = composer.compose(st["undo"][-1])
        self.assertEqual(res["mark"]["notes"], studio_engine.mark_notes(old_piece, 10.0))
        again = web_api.mark_add(res["state"], 10.0, "both", "", "mclient1", rev0)
        self.assertEqual(again["state"]["marks"], res["state"]["marks"])
        via_json = json.loads(web_api.handle_json(json.dumps(
            {"cmd": "mark_add", "payload": {"state": res["state"], "t": 3.0, "id": "mjson1", "rev": rev0}})))
        self.assertEqual(via_json["mark"]["id"], "mjson1")

    def test_zero_and_zero_point_zero_give_the_same_revision_in_the_web_state(self):
        st = self._with_mark()
        before = web_api.detail(st)["marks"][0]["stale"]
        changed = web_api.option(st, "mute_share", 0.5)
        back = web_api.option(self._rt(changed), "mute_share", 0)
        self.assertFalse(before)
        self.assertFalse(web_api.detail(self._rt(back))["marks"][0]["stale"])


if __name__ == "__main__":
    unittest.main()
