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


if __name__ == "__main__":
    unittest.main()
