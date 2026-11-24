import argparse
import collections
import copy
import hashlib
import json
import random
import shutil
import sys
import tempfile
import threading
import unittest
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path

import composer
import fretboard
from studio import engine as studio_engine
from studio import render as studio_render
from studio import server as studio_server
from studio import summary as studio_summary


GOLDEN_V8_MID_SHA256 = {
    801: "35bfc27c463990048b6286f4d5c54fd33bb5810ba337e16d132b955965981fd7",
    802: "bd23545662eea92ba1bee302a6b17d9a0b45e71ad8dc9214a7432712a6a0ffac",
    805: "f282b2b7928be26d8724d080f4ace15a59fe7b13a180556e26b4083504c503ef",
    807: "bf33165ac246f61fd9a2424ae933d62239887be3c3a8b2236d60e355aaf4ddb1",
    809: "de2977c3a95c0932161c93541d3709f673b8cdef59967a85f762cfcaaf12cc89",
    810: "f11bfbfac1e772e5842bf6503ece98880f531d6ab3f9f388190cc5b40807fc1a",
}

GOLDEN_RANDOM_V8_MID_SHA256 = {
    792514: "68568ea2f532c2377145638a5098f8565dd59762ad848ea9310d0337c65a6f02",
    604693: "5ba6f42307c238e562b1fc9d37a310efedaba20981e936dc18ff554423fc9065",
    942077: "8b9efc818c39024fed3aec7ea95767a0a1dde06ae0eeec74686ea531dde63fe7",
    687798: "f04058211c9bdbf9d0e184af0028d2e1136a5b8ce8988e17485aab9c829fee1a",
    518853: "b408b453c90b152fc8d20e0e2422426df917817695b3a73babb8bf0b2382d563",
    168680: "2b598c5de891d22d39ff367468b550f9f79792127c18882aef551af4f24643a7",
    182816: "5d7da49efbc42f3dc981a3ec4807fa980395d043d166412e82d8cf9a7c0024f2",
    853315: "cc02ebfcfb6bf43e116aecedc2e03d48ed2e0fbdd2cbdf595438bc67e0c27948",
    588703: "a7306ea0e532eecefbf5984470407d8bf32ee665ee1c97fd7f39dcfc6a2ab7cc",
    94121: "5eaefd8f0915afa7f24d0b2835f56d7901e2589d942b266b734516f8ce1d7d58",
    566529: "1691ca14087454c4a4f064b450ed2dccf072623ef797474c482ce4d60cfaf5be",
    895585: "c2bec3c67c4145aaab02077ae9dda096f5bcfd65b999f71ff7bcb1db781c7b6a",
    396748: "05e8f92535d010eb96bc182b560f8becb773a5f49c59d144c650853e8305ed45",
    408510: "5470d575caa3378c13645e0f9a2851a34102c86ed8990dd4f6a93c27bf4fe180",
    772707: "76348c206a38d407591207757e6e7ac6382cd06a36e1ff7ee86ae08a28c34dc4",
    195912: "bebd09762fed5d4d538747d3d81179b3e797faf503f1cfce4858a91dbe5199a9",
    31844: "246f5ba3694b9801165bea57cb2f3390936b24ebcf5f2a6674d7029462714f1c",
    687270: "3721223487a74c4b984404c51f5af83f0f2a784f14262941bc1f1a8a53138a6e",
    78428: "93db201d4a85b73fecadaedb777f6ce35427db5b944e6e4f5b9e67667931b482",
    106810: "75578be9fe917d342b8ee19d10678b6d0aea8e3873b54f14321fb02c7bc7e77e",
}


def midi_sha256(piece):
    with tempfile.TemporaryDirectory() as d:
        base = Path(d) / "p"
        composer.export(piece, base)
        return hashlib.sha256(base.with_suffix(".mid").read_bytes()).hexdigest()


def section_spans(piece):
    ch = piece["chords"]
    bars = [
        (c["sec"], c["start"], ch[i + 1]["start"] if i + 1 < len(ch) else piece["total16"])
        for i, c in enumerate(ch)
    ]
    spans = []
    for sec, s, e in bars:
        if spans and spans[-1][0] == sec:
            spans[-1][2] = e
        else:
            spans.append([sec, s, e])
    return [(sec, s, e) for sec, s, e in spans]


def section_notes(piece, g, start16, end16):
    out = []
    for n in piece["parts"][g]:
        if start16 <= n["start"] < end16:
            out.append((
                n["start"] - start16,
                n["dur"],
                n["pitch"],
                n["vel"],
                n["string"],
                n["fret"],
                tuple(n.get("tech", [])),
            ))
    return sorted(out)


def boundary_equal(a, b, end16):
    """Section notes equal, except what the neighbouring section can legitimately change:
    the duration of a note ringing past the section end and the legato mark of its first note."""
    if len(a) != len(b):
        return False
    key = lambda n: (n[0], n[2], n[3], n[4], n[5])
    a, b = sorted(a, key=key), sorted(b, key=key)
    first = min((n[0] for n in a), default=0)
    for x, y in zip(a, b):
        if key(x) != key(y):
            return False
        if x[1] != y[1] and x[0] + max(x[1], y[1]) <= end16:
            return False
        if x[6] != y[6] and x[0] != first:
            return False
    return True


def has_active_chatter(rec, piece, sec):
    lead = rec["intent"]["lead"]
    L = composer.METERS[rec["intent"]["meter"]][0]
    ch = piece["chords"]
    for i, b in enumerate(rec["form"]):
        if b["sec"] != sec:
            continue
        s = ch[i]["start"]
        e = ch[i + 1]["start"] if i + 1 < len(ch) else piece["total16"]
        mask = {int(n["start"] - s) for n in piece["parts"][lead] if s <= n["start"] < e}
        if b.get("role") == "chatter" and len(mask) < L / 2:
            return True
    return False


def pick_seeds(predicate, count=20, start=801):
    out = []
    s = start
    while len(out) < count:
        rec = studio_engine.new_recipe(s * 1000)
        if predicate(rec):
            out.append(s * 1000)
        s += 1
    return out


class TestBaselineIdentity(unittest.TestCase):
    def test_no_studio_byte_identical_v8(self):
        for seed, expected_hash in GOLDEN_V8_MID_SHA256.items():
            args = argparse.Namespace(meter=None, mode=None, key=None, tempo=None, v8=True)
            rec = composer.make_recipe(seed * 1000, args)
            self.assertNotIn("studio", rec)
            actual = midi_sha256(composer.compose(rec))
            self.assertEqual(actual, expected_hash, f"seed {seed} v8 MIDI drifted")


class TestLayerIsolation(unittest.TestCase):
    def _assert_section_layer(self, layer, sec, seeds, check_other_changed=True):
        for seed in seeds:
            rec0 = studio_engine.new_recipe(seed)
            p0 = composer.compose(rec0)
            rec1 = studio_engine.reroll(rec0, layer, sec)
            p1 = composer.compose(rec1)

            lead = rec0["intent"]["lead"]
            other = "g2" if lead == "g1" else "g1"

            spans0 = section_spans(p0)
            spans1 = section_spans(p1)
            self.assertEqual(spans0, spans1, (layer, sec, seed))
            self.assertEqual(p0["tempo"], p1["tempo"], (layer, sec, seed))
            self.assertEqual(rec0["harmony"], rec1["harmony"], (layer, sec, seed))

            # Lead guitar is never touched by accompaniment / B / ending layers
            for sname, s0, e0 in spans0:
                self.assertEqual(
                    section_notes(p0, lead, s0, e0),
                    section_notes(p1, lead, s0, e0),
                    (layer, sec, seed, sname, "lead changed"),
                )
                if sname != sec:
                    self.assertTrue(
                        boundary_equal(section_notes(p0, other, s0, e0), section_notes(p1, other, s0, e0), e0 - s0),
                        (layer, sec, seed, sname, "other section changed"),
                    )

            if check_other_changed:
                target0 = [section_notes(p0, other, s0, e0) for sn, s0, e0 in spans0 if sn == sec]
                target1 = [section_notes(p1, other, s0, e0) for sn, s0, e0 in spans1 if sn == sec]
                self.assertNotEqual(target0, target1, (layer, sec, seed, "target did not change"))

    def test_layer_isolation_and_target_change(self):
        seeds_20 = [s * 1000 for s in range(801, 821)]
        seeds_a2 = pick_seeds(lambda r: any(b["sec"] == "A2" for b in r["form"]), 20)

        # comp@A and comp@A2
        self._assert_section_layer("comp", "A", seeds_20, check_other_changed=True)
        self._assert_section_layer("comp", "A2", seeds_a2, check_other_changed=True)

        # texture@A and texture@A2: isolation on 20 seeds + target change on 20 active chatter seeds
        self._assert_section_layer("texture", "A", seeds_20, check_other_changed=False)
        self._assert_section_layer("texture", "A2", seeds_a2, check_other_changed=False)
        chatter_a = pick_seeds(lambda r: has_active_chatter(r, composer.compose(r), "A"), 20)
        chatter_a2 = pick_seeds(
            lambda r: any(b["sec"] == "A2" for b in r["form"]) and has_active_chatter(r, composer.compose(r), "A2"),
            20,
        )
        self._assert_section_layer("texture", "A", chatter_a, check_other_changed=True)
        self._assert_section_layer("texture", "A2", chatter_a2, check_other_changed=True)

        # Verify texture never changes the realized roles of the second guitar
        for seed in seeds_20:
            rec0 = studio_engine.new_recipe(seed)
            p0, t0 = traced(rec0)
            p1, t1 = traced(studio_engine.reroll(rec0, "texture", "A"))
            og = "g2" if rec0["intent"]["lead"] == "g1" else "g1"
            for sec in ("A", "A2", "A3"):
                for (sn, s0, e0), (_, s1, e1) in zip(section_spans(p0), section_spans(p1)):
                    if sn == sec:
                        self.assertEqual(strum_onsets(t0, og, s0, e0), strum_onsets(t1, og, s1, e1), (seed, sec))

        # b@B
        self._assert_section_layer("b", "B", seeds_20, check_other_changed=True)

        # lead_rhythm@all
        for seed in seeds_20:
            rec0 = studio_engine.new_recipe(seed)
            p0 = composer.compose(rec0)
            rec1 = studio_engine.reroll(rec0, "lead_rhythm", "all")
            p1 = composer.compose(rec1)
            lead = rec0["intent"]["lead"]
            other = "g2" if lead == "g1" else "g1"

            self.assertEqual(rec0["harmony"], rec1["harmony"], seed)
            self.assertEqual(rec0["form"], rec1["form"], seed)
            self.assertEqual(p0["tempo"], p1["tempo"], seed)
            self.assertEqual(p0["meters"], p1["meters"], seed)
            self.assertEqual(p0["total16"], p1["total16"], seed)
            self.assertEqual(p0["chords"], p1["chords"], seed)

            self.assertNotEqual(
                [(n["start"], n["dur"], n["pitch"]) for n in p0["parts"][lead]],
                [(n["start"], n["dur"], n["pitch"]) for n in p1["parts"][lead]],
                ("lead_rhythm", seed),
            )
            spans0 = section_spans(p0)
            for sname, s0, e0 in spans0:
                if sname in ("intro", "end"):
                    for g in ("g1", "g2"):
                        self.assertTrue(
                            boundary_equal(section_notes(p0, g, s0, e0), section_notes(p1, g, s0, e0), e0 - s0),
                            ("lead_rhythm", seed, sname, g),
                        )
                elif sname == "B":
                    # Second-guitar plan in B is unchanged; only minor-9th resolution against the new lead may cut/drop a note.
                    b0 = section_notes(p0, other, s0, e0)
                    b1 = section_notes(p1, other, s0, e0)
                    if b0 != b1:
                        lead_b1 = [(n["start"] - s0, n["start"] + n["dur"] - s0, n["pitch"]) for n in p1["parts"][lead] if s0 <= n["start"] < e0]
                        b1_map = {(n[0], n[2]): n for n in b1}
                        for n0 in b0:
                            n1 = b1_map.get((n0[0], n0[2]))
                            if n1 is None or n1 != n0:
                                clash = any(
                                    composer.minor_ninth(lp, n0[2]) and not (ls >= n0[0] + n0[1] or le <= n0[0])
                                    for ls, le, lp in lead_b1
                                )
                                if not clash:
                                    # Check whether p0 had a minor-9th cut/drop against p0's lead that no longer applies in p1
                                    lead_b0 = [(n["start"] - s0, n["start"] + n["dur"] - s0, n["pitch"]) for n in p0["parts"][lead] if s0 <= n["start"] < e0]
                                    clash0 = any(
                                        composer.minor_ninth(lp, n0[2]) and not (ls >= n0[0] + n0[1] or le <= n0[0])
                                        for ls, le, lp in lead_b0
                                    )
                                    self.assertTrue(clash0 or n1 is not None and (n1[0], n1[2], n1[3]) == (n0[0], n0[2], n0[3]), ("lead_rhythm leaked to B", seed, n0, n1))

        # intro@intro
        for seed in seeds_20:
            rec0 = studio_engine.new_recipe(seed)
            p0 = composer.compose(rec0)
            rec1 = studio_engine.reroll(rec0, "intro", "intro")
            p1 = composer.compose(rec1)

            spans0 = section_spans(p0)
            spans1 = section_spans(p1)
            self.assertEqual([x[0] for x in spans0], [x[0] for x in spans1], ("intro", seed))

            in0 = [section_notes(p0, g, spans0[0][1], spans0[0][2]) for g in ("g1", "g2")]
            in1 = [section_notes(p1, g, spans1[0][1], spans1[0][2]) for g in ("g1", "g2")]
            self.assertTrue(
                spans0[0][2] != spans1[0][2] or in0 != in1,
                ("intro did not change", seed),
            )
            for (sn0, s0, e0), (sn1, s1, e1) in zip(spans0[1:], spans1[1:]):
                self.assertEqual(sn0, sn1)
                self.assertEqual(e0 - s0, e1 - s1)
                for g in ("g1", "g2"):
                    self.assertTrue(
                        boundary_equal(section_notes(p0, g, s0, e0), section_notes(p1, g, s1, e1), e0 - s0),
                        ("intro leaked", seed, sn0, g),
                    )

        # ending@end
        for seed in seeds_20:
            rec0 = studio_engine.new_recipe(seed)
            p0 = composer.compose(rec0)
            rec1 = studio_engine.reroll(rec0, "ending", "end")
            p1 = composer.compose(rec1)
            spans0 = section_spans(p0)
            spans1 = section_spans(p1)
            self.assertEqual(spans0, spans1, ("ending", seed))
            for sn, s0, e0 in spans0:
                if sn == "end":
                    n0 = [section_notes(p0, g, s0, e0) for g in ("g1", "g2")]
                    n1 = [section_notes(p1, g, s0, e0) for g in ("g1", "g2")]
                    self.assertNotEqual(n0, n1, ("ending did not change", seed))
                else:
                    for g in ("g1", "g2"):
                        self.assertTrue(
                            boundary_equal(section_notes(p0, g, s0, e0), section_notes(p1, g, s0, e0), e0 - s0),
                            ("ending leaked", seed, sn, g),
                        )

        # tempo@all
        for seed in seeds_20:
            rec0 = studio_engine.new_recipe(seed)
            p0 = composer.compose(rec0)
            rec1 = studio_engine.reroll(rec0, "tempo", "all")
            p1 = composer.compose(rec1)
            self.assertNotEqual(p0["tempo"], p1["tempo"], ("tempo did not change", seed))
            lo = max(60, int(round(p0["tempo"] * 0.8)))
            hi = min(220, int(round(p0["tempo"] * 1.1)))
            self.assertTrue(lo <= p1["tempo"] <= hi, (seed, p0["tempo"], p1["tempo"], lo, hi))
            for g in ("g1", "g2"):
                self.assertEqual(
                    section_notes(p0, g, 0, p0["total16"]),
                    section_notes(p1, g, 0, p1["total16"]),
                    ("tempo changed notes", seed, g),
                )


class TestDeterminismAndSalts(unittest.TestCase):
    def test_same_salt_deterministic_and_different_salts_vary(self):
        seeds_20 = [s * 1000 for s in range(801, 821)]
        seeds_a2 = pick_seeds(lambda r: any(b["sec"] == "A2" for b in r["form"]), 20)
        chatter_a = pick_seeds(lambda r: has_active_chatter(r, composer.compose(r), "A"), 20)

        cases = [
            ("comp", "A", seeds_20),
            ("comp", "A2", seeds_a2),
            ("texture", "A", chatter_a),
            ("lead_rhythm", "all", seeds_20),
            ("b", "B", seeds_20),
            ("intro", "intro", seeds_20),
            ("ending", "end", seeds_20),
            ("tempo", "all", seeds_20),
        ]
        for layer, sec, seeds in cases:
            diff = 0
            for seed in seeds:
                a1 = studio_engine.reroll(studio_engine.new_recipe(seed), layer, sec)
                a2 = studio_engine.reroll(studio_engine.new_recipe(seed), layer, sec)
                self.assertEqual(a1, a2, (layer, sec, seed))
                h1 = midi_sha256(composer.compose(a1))
                h2 = midi_sha256(composer.compose(a2))
                self.assertEqual(h1, h2, (layer, sec, seed))

                b = studio_engine.reroll(copy.deepcopy(a1), layer, sec)
                hb = midi_sha256(composer.compose(b))
                if hb != h1:
                    diff += 1
            self.assertGreaterEqual(diff, 15, (layer, sec, diff))


class TestLocks(unittest.TestCase):
    def test_locked_layer_not_changed_by_reroll_all(self):
        for seed in [s * 1000 for s in range(801, 811)]:
            rec = studio_engine.new_recipe(seed)
            rec = studio_engine.set_lock(rec, "intro", "intro", True)
            rec = studio_engine.set_lock(rec, "tempo", "all", True)
            rec = studio_engine.set_lock(rec, "ending", "end", True)
            rec = studio_engine.set_lock(rec, "comp", "A", True)
            rec = studio_engine.set_lock(rec, "lead_rhythm", "all", True)

            # Direct reroll of locked layer is a no-op
            direct = studio_engine.reroll(rec, "comp", "A")
            self.assertEqual(direct, rec)

            p0 = composer.compose(rec)
            rec_all = studio_engine.reroll(rec, "all", "all")
            p1 = composer.compose(rec_all)

            for key in ("intro@intro", "tempo@all", "ending@end", "comp@A", "lead_rhythm@all"):
                self.assertEqual(rec_all["studio"]["salts"].get(key, 0), 0, (seed, key))

            self.assertEqual(rec_all["studio"]["salts"].get("b@B", 0), 1, seed)
            self.assertEqual(p0["tempo"], p1["tempo"], seed)

            spans0 = section_spans(p0)
            spans1 = section_spans(p1)
            self.assertEqual(spans0, spans1, seed)
            for sn, s0, e0 in spans0:
                if sn in ("intro", "end"):
                    for g in ("g1", "g2"):
                        self.assertEqual(section_notes(p0, g, s0, e0), section_notes(p1, g, s0, e0), (seed, sn, g))
            lead = rec["intent"]["lead"]
            self.assertEqual(p0["parts"][lead], p1["parts"][lead], seed)


class TestBLeadOption(unittest.TestCase):
    def test_b_lead_modes(self):
        seeds_20 = [s * 1000 for s in range(801, 821)]
        for seed in seeds_20:
            rec_plain = composer.make_recipe(
                seed, argparse.Namespace(meter=None, mode=None, key=None, tempo=None, v8=True)
            )
            rec_def = studio_engine.new_recipe(seed)
            rec_aug = studio_engine.set_option(rec_def, "b_lead", "augmented")
            self.assertEqual(midi_sha256(composer.compose(rec_def)), midi_sha256(composer.compose(rec_aug)), seed)

            lead = rec_def["intent"]["lead"]
            p_plain = composer.compose(rec_plain)
            p_aug = composer.compose(rec_aug)
            for sn, s0, e0 in section_spans(p_plain):
                if sn == "B":
                    b_plain = section_notes(p_plain, lead, s0, e0)
                    b_aug = section_notes(p_aug, lead, s0, e0)
                    self.assertEqual(
                        [(n[0], n[2], n[3]) for n in b_plain],
                        [(n[0], n[2], n[3]) for n in b_aug],
                        seed,
                    )

            # rest: no lead notes in B
            rec_rest = studio_engine.set_option(rec_def, "b_lead", "rest")
            p_rest = composer.compose(rec_rest)
            for sn, s0, e0 in section_spans(p_rest):
                if sn == "B":
                    self.assertEqual(section_notes(p_rest, lead, s0, e0), [], seed)

            # same_speed: B lead notes use hook's shaped durations
            rec_same = studio_engine.set_option(rec_def, "b_lead", "same_speed")
            p_same = composer.compose(rec_same)
            hook_L = composer.METERS[rec_same["intent"]["meter"]][0]
            b_L = composer.METERS[rec_same["intent"]["b_meter"]][0]
            expected_ev = composer.shape_lead(composer.motif_events(rec_same["motifs"]["hook"], hook_L), rec_same["lead"])
            expected_durs = [min(e["dur"], b_L - e["on"]) for e in expected_ev if e["on"] < b_L]
            b_bars = [
                (c["start"], p_same["chords"][i + 1]["start"] if i + 1 < len(p_same["chords"]) else p_same["total16"])
                for i, c in enumerate(p_same["chords"])
                if c["sec"] == "B"
            ]
            for bs, be in b_bars:
                notes = sorted((n for n in p_same["parts"][lead] if bs <= n["start"] < be), key=lambda n: n["start"])
                self.assertEqual([n["dur"] for n in notes], expected_durs, (seed, bs))

    def test_trailing_rest_and_v8_doses(self):
        # 809 has a trailing rest in v8 that --notail removes
        rec_nt_base = studio_engine.new_recipe(809000)
        rec_nt = studio_engine.set_option(rec_nt_base, "trailing_rest", False)
        p0 = composer.compose(rec_nt_base)
        p1 = composer.compose(rec_nt)
        self.assertNotEqual(midi_sha256(p0), midi_sha256(p1))

        # V8 doses apply on the next comp re-roll, without retroactively changing already-rolled sections
        seed_a2 = pick_seeds(lambda r: any(b["sec"] == "A2" for b in r["form"]), 1)[0]
        rec = studio_engine.new_recipe(seed_a2)
        rec_a = studio_engine.reroll(rec, "comp", "A")
        p_before_dose = composer.compose(rec_a)
        rec_dose = studio_engine.set_option(rec_a, "octave_piece", 1.0)
        rec_dose = studio_engine.set_option(rec_dose, "octave_sixteenths", 1.0)
        rec_dose = studio_engine.set_option(rec_dose, "dead_notes", 0.0)
        rec_dose = studio_engine.set_option(rec_dose, "slide_max", 7)
        p_after_dose = composer.compose(rec_dose)
        self.assertEqual(midi_sha256(p_before_dose), midi_sha256(p_after_dose))

        rec_a2 = studio_engine.reroll(rec_dose, "comp", "A2")
        p_after_reroll = composer.compose(rec_a2)
        other = "g2" if rec["intent"]["lead"] == "g1" else "g1"
        for sn, s0, e0 in section_spans(p_after_reroll):
            if sn == "A2":
                dyads = [n for n in p_after_reroll["parts"][other] if s0 <= n["start"] < e0 and n.get("dyad")]
                self.assertTrue(dyads, "octave_piece=1.0 should force octave dyads on next comp reroll")


class TestStudioInvariants(unittest.TestCase):
    def test_composer_invariants_after_random_rerolls(self):
        bad_clash = shared_clash = 0
        for s in range(801, 821):
            rng = random.Random(s + 4242)
            rec = studio_engine.new_recipe(s * 1000)
            choices = [(item["layer"], item["section"]) for item in studio_engine.available_layers(rec)]
            for _ in range(5):
                layer, sec = rng.choice(choices)
                rec = studio_engine.reroll(rec, layer, sec)
                if rng.random() < 0.3:
                    rec = studio_engine.set_option(rec, "b_lead", rng.choice(["augmented", "same_speed", "rest"]))
                if rng.random() < 0.2:
                    rec = studio_engine.set_option(rec, "trailing_rest", rng.choice([True, False]))

            p = composer.compose(rec)

            # 1. String and fret consistency
            for g, notes in p["parts"].items():
                for n in notes:
                    self.assertTrue(0 <= n["fret"] <= fretboard.MAX_FRET, (s, g, n))
                    self.assertEqual(fretboard.OPEN[n["string"]] + n["fret"], n["pitch"], (s, g, n))

            # 2. Legato only on same string + hammer ascending
            for g, notes in p["parts"].items():
                line = [
                    n for n in notes
                    if not n.get("chordtone") and not n.get("dyad") and not (n.get("fixed") and "preset_tech" not in n)
                ]
                for prev, n in zip(line, line[1:]):
                    if {"hammer", "pull", "slide"} & set(n["tech"]):
                        self.assertEqual(prev["string"], n["string"], (s, g, n))
                    if "hammer" in n["tech"]:
                        self.assertGreater(n["fret"], prev["fret"], (s, g, n))

            # 3. Octave slides from same string
            for g, notes in p["parts"].items():
                last = {}
                for n in sorted(notes, key=lambda x: (x["start"], x["string"], x["pitch"])):
                    if n.get("dyad") and "slide" in n["tech"]:
                        prev = last.get(n["string"])
                        self.assertTrue(
                            prev is not None and prev["fret"] != n["fret"] and "dead" not in prev.get("tech", []),
                            (s, g, prev, n),
                        )
                    last[n["string"]] = n

            # 4. Taps above fretting hand
            for g, notes in p["parts"].items():
                for _, n in zip(notes, notes[1:]):
                    if "tap" in n.get("tech", []) and n.get("fixed"):
                        nxt = [
                            m for m in notes
                            if n["start"] < m["start"] <= n["start"] + 2
                            and m["string"] == n["string"]
                            and "pull" in m.get("tech", [])
                        ][:1]
                        for m in nxt:
                            self.assertGreater(n["fret"], m["fret"], (s, g, n))

            # 5. Semitone clash count
            g2 = {}
            for n in p["parts"]["g2"]:
                g2.setdefault(n["start"], []).append(n["pitch"])
            for n in p["parts"]["g1"]:
                for q in g2.get(n["start"], []):
                    shared_clash += 1
                    bad_clash += abs(n["pitch"] - q) % 12 in (1, 11)

            # 6. No minor ninths between guitars (v7+)
            a = [(n["start"], n["start"] + n["dur"], n["pitch"]) for n in p["parts"]["g1"]]
            b = [(n["start"], n["start"] + n["dur"], n["pitch"]) for n in p["parts"]["g2"]]
            for x, y in ((a, b), (b, a)):
                for s0, _, pa in x:
                    for s2, e2, q in y:
                        if s2 <= s0 < e2 and pa != q:
                            self.assertNotEqual(abs(pa - q) % 12, 1, (s, s0, pa, q))

            # 7. MusicXML measures fill meter
            with tempfile.TemporaryDirectory() as d:
                composer.export(p, Path(d) / "x")
                root = ET.parse(Path(d) / "x.musicxml").getroot()
            for part in root.iter("part"):
                num = den = None
                for m in part.iter("measure"):
                    t = m.find("attributes/time")
                    if t is not None:
                        num, den = int(t.findtext("beats")), int(t.findtext("beat-type"))
                    total = sum(int(n.findtext("duration")) for n in m.iter("note") if n.find("chord") is None)
                    self.assertEqual(total, num * 16 // den, (s, part.get("id"), m.get("number")))

        self.assertLess(bad_clash / max(1, shared_clash), 0.03, (bad_clash, shared_clash))


class TestUndo(unittest.TestCase):
    def test_undo_restores_previous_recipe(self):
        with tempfile.TemporaryDirectory() as d:
            store = studio_engine.StudioStore(d, render_fn=studio_render.fake_render)
            det0 = store.create_piece(801000)
            pid = det0["id"]
            pdir = Path(d) / pid
            r0 = json.loads((pdir / "recipe.json").read_text(encoding="utf-8"))

            store.reroll(pid, "comp", "A")
            r1 = json.loads((pdir / "recipe.json").read_text(encoding="utf-8"))
            self.assertNotEqual(r1, r0)

            store.set_option(pid, "b_lead", "rest")
            r2 = json.loads((pdir / "recipe.json").read_text(encoding="utf-8"))
            self.assertNotEqual(r2, r1)

            store.set_lock(pid, "comp", "A", True)
            r3 = json.loads((pdir / "recipe.json").read_text(encoding="utf-8"))
            self.assertNotEqual(r3, r2)

            store.undo(pid)
            self.assertEqual(json.loads((pdir / "recipe.json").read_text(encoding="utf-8")), r2)
            store.undo(pid)
            self.assertEqual(json.loads((pdir / "recipe.json").read_text(encoding="utf-8")), r1)
            store.undo(pid)
            self.assertEqual(json.loads((pdir / "recipe.json").read_text(encoding="utf-8")), r0)


class TestServerAndSummary(unittest.TestCase):
    def _req(self, url, method="GET", body=None, headers=None):
        hdrs = dict(headers or {})
        data = None
        if body is not None:
            data = json.dumps(body).encode("utf-8")
            hdrs["Content-Type"] = "application/json"
        req = urllib.request.Request(url, data=data, method=method, headers=hdrs)
        with urllib.request.urlopen(req, timeout=10) as resp:
            return resp.status, dict(resp.headers), resp.read()

    def test_server_end_to_end(self):
        with tempfile.TemporaryDirectory() as d:
            srv = studio_server.create_server(d, host="127.0.0.1", port=0, render_fn=studio_render.fake_render)
            port = srv.server_address[1]
            t = threading.Thread(target=srv.serve_forever, daemon=True)
            t.start()
            base = f"http://127.0.0.1:{port}"
            try:
                st, _, html_bytes = self._req(f"{base}/")
                self.assertEqual(st, 200)
                html = html_bytes.decode("utf-8")
                self.assertIn("Новая пьеса", html)
                self.assertIn("Отменить", html)
                self.assertIn("Скачать MIDI", html)

                st, _, raw = self._req(f"{base}/api/new", method="POST", body={"seed": 801000})
                self.assertEqual(st, 200)
                created = json.loads(raw)
                pid = created["id"]
                for key in ("id", "seed", "meter", "mode", "key", "tempo", "length", "sections", "whole_layers", "options"):
                    self.assertIn(key, created)
                self.assertTrue(created["sections"])
                for sec in created["sections"]:
                    for sk in ("section", "start_sec", "end_sec", "layers"):
                        self.assertIn(sk, sec)

                st, _, raw = self._req(f"{base}/api/pieces")
                self.assertEqual(st, 200)
                pieces = json.loads(raw)["pieces"]
                self.assertEqual(len(pieces), 1)
                for key in ("id", "seed", "meter", "mode", "key", "tempo", "length", "sections"):
                    self.assertIn(key, pieces[0])

                st, _, raw = self._req(f"{base}/api/piece/{pid}")
                self.assertEqual(st, 200)
                self.assertEqual(json.loads(raw)["id"], pid)

                st, _, raw = self._req(f"{base}/api/piece/{pid}/reroll", method="POST", body={"layer": "comp", "section": "A"})
                self.assertEqual(st, 200)
                det = json.loads(raw)
                sec_a = next(s for s in det["sections"] if s["section"] == "A")
                comp_a = next(l for l in sec_a["layers"] if l["layer"] == "comp")
                self.assertEqual(comp_a["salt"], 1)

                st, _, raw = self._req(f"{base}/api/piece/{pid}/lock", method="POST", body={"layer": "comp", "section": "A", "locked": True})
                self.assertEqual(st, 200)
                det = json.loads(raw)
                sec_a = next(s for s in det["sections"] if s["section"] == "A")
                comp_a = next(l for l in sec_a["layers"] if l["layer"] == "comp")
                self.assertTrue(comp_a["locked"])

                st, _, raw = self._req(f"{base}/api/piece/{pid}/option", method="POST", body={"name": "b_lead", "value": "rest"})
                self.assertEqual(st, 200)
                self.assertEqual(json.loads(raw)["options"]["b_lead"], "rest")

                st, _, raw = self._req(f"{base}/api/piece/{pid}/undo", method="POST", body={})
                self.assertEqual(st, 200)
                self.assertEqual(json.loads(raw)["options"]["b_lead"], "augmented")

                st, _, mid = self._req(f"{base}/api/piece/{pid}/piece.mid")
                self.assertEqual(st, 200)
                self.assertTrue(mid.startswith(b"MThd"))

                st, _, mxml = self._req(f"{base}/api/piece/{pid}/piece.musicxml")
                self.assertEqual(st, 200)
                ET.fromstring(mxml)

                st, _, tab = self._req(f"{base}/api/piece/{pid}/piece.tab.txt")
                self.assertEqual(st, 200)
                self.assertIn("== g1 ==", tab.decode("utf-8"))

                st, _, mp3 = self._req(f"{base}/api/piece/{pid}/audio.mp3")
                self.assertEqual(st, 200)
                self.assertTrue(len(mp3) > 0)

                st, hdrs, part = self._req(f"{base}/api/piece/{pid}/audio.mp3", headers={"Range": "bytes=0-3"})
                self.assertEqual(st, 206)
                self.assertEqual(len(part), 4)
                self.assertIn("Content-Range", {k.title(): v for k, v in hdrs.items()})

                pdir = Path(d) / pid
                for fname in ("recipe.json", "piece.mid", "piece.musicxml", "piece.tab.txt", "piece.mp3", "history.jsonl"):
                    self.assertTrue((pdir / fname).exists(), fname)

                lines = [json.loads(line) for line in (pdir / "history.jsonl").read_text(encoding="utf-8").splitlines() if line]
                self.assertEqual(len(lines), 8)
                self.assertEqual(
                    [x["action"] for x in lines],
                    ["new", "reroll", "lock", "option", "undo", "export", "export", "export"],
                )
                for row in lines:
                    for k in ("t", "action", "layer", "section", "salt", "value"):
                        self.assertIn(k, row)

                stats = studio_summary.summarize(d)
                self.assertEqual(stats["pieces_total"], 1)
                self.assertEqual(stats["pieces_exported"], 1)
                self.assertEqual(stats["rerolls_per_layer"].get("comp"), 1)
                self.assertEqual(stats["rerolls_before_first_export"], [1])
                self.assertIn("b_lead", stats["options_used"])
            finally:
                srv.shutdown()
                srv.server_close()


def plain_recipe(seed, **flags):
    args = argparse.Namespace(
        meter=flags.pop("meter", None), mode=flags.pop("mode", None),
        key=flags.pop("key", None), tempo=flags.pop("tempo", None), **flags,
    )
    return composer.make_recipe(seed, args)


def piece_sha256(piece):
    with tempfile.TemporaryDirectory() as d:
        base = Path(d) / "p"
        composer.export(piece, base)
        h = hashlib.sha256()
        for suffix in (".mid", ".musicxml", ".tab.txt"):
            h.update(Path(str(base) + suffix).read_bytes())
        return h.hexdigest()


def other_of(rec):
    return "g2" if rec["intent"]["lead"] == "g1" else "g1"


def strum_onsets(trace, g, s0, e0):
    count = {}
    for n in trace["pre"][g]:
        if s0 <= n["start"] < e0:
            count[n["start"]] = count.get(n["start"], 0) + 1
    return sorted(k - s0 for k, v in count.items() if v >= 3)


def traced(rec):
    trace = {}
    return composer.compose(rec, _trace=trace), trace


def span_of(piece, sec):
    return next((s0, e0) for sn, s0, e0 in section_spans(piece) if sn == sec)


class TestAccompanimentFollowsItsOwnPlan(unittest.TestCase):
    def test_locked_comp_and_texture_survive_reroll_all(self):
        for seed in [6] + list(range(0, 14)):
            rec = studio_engine.new_recipe(seed)
            rec = studio_engine.set_lock(rec, "comp", "A", True)
            rec = studio_engine.set_lock(rec, "texture", "A", True)
            p0, t0 = traced(rec)
            p1, t1 = traced(studio_engine.reroll(rec, "all", "all"))
            other = other_of(rec)
            self.assertEqual(
                strum_onsets(t0, other, *span_of(p0, "A")),
                strum_onsets(t1, other, *span_of(p1, "A")),
                ("locked accompaniment changed", seed),
            )

    def test_salted_locked_comp_survives_lead_reroll(self):
        for seed in range(0, 14):
            rec = studio_engine.reroll(studio_engine.new_recipe(seed), "comp", "A")
            rec = studio_engine.set_lock(rec, "comp", "A", True)
            p0, t0 = traced(rec)
            p1, t1 = traced(studio_engine.reroll(rec, "lead_rhythm", "all"))
            other = other_of(rec)
            self.assertEqual(
                strum_onsets(t0, other, *span_of(p0, "A")), strum_onsets(t1, other, *span_of(p1, "A")), seed
            )

    def test_texture_reroll_keeps_realized_roles(self):
        for seed in range(0, 14):
            rec = studio_engine.new_recipe(seed)
            p0, t0 = traced(rec)
            p1, t1 = traced(studio_engine.reroll(rec, "texture", "A"))
            other = other_of(rec)
            self.assertEqual(
                strum_onsets(t0, other, *span_of(p0, "A")), strum_onsets(t1, other, *span_of(p1, "A")), seed
            )

    def test_lead_reroll_keeps_accompaniment_roles(self):
        for seed in range(0, 14):
            rec = studio_engine.new_recipe(seed)
            p0, t0 = traced(rec)
            p1, t1 = traced(studio_engine.reroll(rec, "lead_rhythm", "all"))
            other = other_of(rec)
            for sec in ("A", "A2", "A3"):
                if not any(sn == sec for sn, _, _ in section_spans(p0)):
                    continue
                self.assertEqual(
                    strum_onsets(t0, other, *span_of(p0, sec)),
                    strum_onsets(t1, other, *span_of(p1, sec)),
                    (seed, sec),
                )

    def test_call_response_does_not_follow_a_new_lead(self):
        checked = 0
        for seed in range(0, 20):
            rec = studio_engine.new_recipe(seed)
            call = [i for i, b in enumerate(rec["form"]) if b.get("prog") == "call" and b["sec"] == "A2"]
            if not call:
                continue
            L = composer.METERS[rec["intent"]["meter"]][0]
            p0, t0 = traced(rec)
            p1, t1 = traced(studio_engine.reroll(rec, "lead_rhythm", "all"))
            other = other_of(rec)
            start = p0["chords"][call[0]]["start"]

            def window(tr):
                return sorted(
                    (n["start"] - start, n["dur"], n["pitch"])
                    for n in tr["pre"][other]
                    if start + L // 2 <= n["start"] < start + L
                )

            self.assertTrue(window(t0))
            self.assertEqual(window(t0), window(t1), seed)
            checked += 1
        self.assertGreater(checked, 3)

    def test_locked_ending_survives_reroll_all(self):
        checked = 0
        for seed in range(0, 24):
            rec = studio_engine.new_recipe(seed)
            base_end = next(b.get("end") for b in rec["form"] if b["sec"] == "end")
            salt = next(
                (k for k in (1, 2, 3) if composer._studio_ending(rec["seed"], k, base_end) == "busy"), None
            )
            if salt is None:
                continue
            rec["studio"]["salts"]["ending@end"] = salt
            rec = studio_engine.set_lock(rec, "ending", "end", True)
            p0 = composer.compose(rec)
            p1 = composer.compose(studio_engine.reroll(rec, "all", "all"))
            e0, e1 = span_of(p0, "end"), span_of(p1, "end")
            for g in ("g1", "g2"):
                self.assertEqual(section_notes(p0, g, *e0), section_notes(p1, g, *e1), (seed, g))
            checked += 1
        self.assertGreater(checked, 3)


class TestOlderRecipesAreNotRewritten(unittest.TestCase):
    FLAGS = ({"v6": True}, {"v7": True}, {"v8": True}, {"v8": True, "notail": True})

    def test_metadata_only_edits_keep_the_music(self):
        for flags in self.FLAGS:
            for seed in range(0, 8):
                rec = plain_recipe(seed, **flags)
                expected = piece_sha256(composer.compose(rec))
                locked = studio_engine.set_lock(rec, "comp", "A", True)
                self.assertEqual(piece_sha256(composer.compose(locked)), expected, (flags, seed, "lock"))
                again = studio_engine.set_option(locked, "octave_piece", 0.5)
                self.assertEqual(piece_sha256(composer.compose(again)), expected, (flags, seed, "dose"))

    def test_reroll_leaves_untouched_sections_as_generated(self):
        for flags in self.FLAGS:
            for seed in range(0, 8):
                rec = plain_recipe(seed, **flags)
                p0 = composer.compose(rec)
                spans = section_spans(p0)
                for layer, sec in (("comp", "A"), ("b", "B"), ("intro", "intro"), ("ending", "end")):
                    if sec not in {sn for sn, _, _ in spans}:
                        continue
                    p1 = composer.compose(studio_engine.reroll(rec, layer, sec))
                    for (sn, s0, e0), (_, s1, e1) in zip(spans, section_spans(p1)):
                        if sn == sec:
                            continue
                        for g in ("g1", "g2"):
                            self.assertTrue(
                                boundary_equal(section_notes(p0, g, s0, e0), section_notes(p1, g, s1, e1), e0 - s0),
                                (flags, seed, layer, sn, g),
                            )

    def test_intro_reroll_does_not_move_saved_pushes(self):
        moved = 0
        for seed in range(0, 40):
            rec = plain_recipe(seed, v6=True)
            if not rec["v6"]["pushes"]:
                continue
            p0 = composer.compose(rec)
            p1 = composer.compose(studio_engine.reroll(rec, "intro", "intro"))
            s0, s1 = section_spans(p0), section_spans(p1)
            if s0[0][2] - s0[0][1] == s1[0][2] - s1[0][1]:
                continue
            moved += 1
            for (sn, a0, b0), (_, a1, b1) in zip(s0[1:], s1[1:]):
                for g in ("g1", "g2"):
                    self.assertTrue(
                        boundary_equal(section_notes(p0, g, a0, b0), section_notes(p1, g, a1, b1), b0 - a0),
                        (seed, sn, g),
                    )
        self.assertGreater(moved, 2)


class TestOptionsKeepLockedMaterial(unittest.TestCase):
    def test_trailing_rest_off_keeps_a_valid_hook(self):
        kw = dict(v8=True, meter="6/8", key="E", mode="lydian", tempo=150)
        rec = plain_recipe(1, **kw)
        expected = plain_recipe(1, notail=True, **kw)
        off = studio_engine.set_option(rec, "trailing_rest", False)
        piece, trace = traced(off)
        L = composer.METERS["6/8"][0]
        self.assertTrue(all(o < L for o in trace["hook"]["rhythm"]), trace["hook"]["rhythm"])
        self.assertEqual(trace["hook"]["rhythm"], expected["motifs"]["hook"]["rhythm"])
        for g in ("g1", "g2"):
            self.assertTrue(all(n["dur"] > 0 for n in piece["parts"][g]), g)

    def test_trailing_rest_follows_the_recipe_version(self):
        checked = 0
        for seed in range(0, 12):
            with_tail = plain_recipe(seed, v8=True)
            no_tail = plain_recipe(seed, v8=True, notail=True)
            if with_tail["motifs"]["hook"] == no_tail["motifs"]["hook"]:
                continue
            st = studio_engine.set_lock(no_tail, "comp", "A", True)
            self.assertFalse(st["studio"]["options"]["trailing_rest"], seed)
            on = studio_engine.set_option(st, "trailing_rest", True)
            self.assertEqual(traced(on)[1]["hook"]["rhythm"], with_tail["motifs"]["hook"]["rhythm"], seed)
            self.assertEqual(piece_sha256(composer.compose(st)), piece_sha256(composer.compose(no_tail)), seed)
            self.assertNotEqual(piece_sha256(composer.compose(on)), piece_sha256(composer.compose(st)), seed)
            checked += 1
        self.assertGreater(checked, 3)

    def test_dose_edit_keeps_locked_comp_notes(self):
        rec = studio_engine.new_recipe(0)
        st = rec["studio"]
        st["salts"]["comp@A"] = 1
        st["locks"] = ["comp@A"]
        st["options"]["octave_piece"] = 1.0
        st.pop("comp_doses", None)
        p0 = composer.compose(rec)
        other = other_of(rec)
        s0, e0 = span_of(p0, "A")
        self.assertTrue([n for n in p0["parts"][other] if s0 <= n["start"] < e0 and n.get("dyad")])
        edited = studio_engine.set_option(rec, "octave_piece", 0.0)
        p1 = composer.compose(edited)
        self.assertEqual(section_notes(p0, other, s0, e0), section_notes(p1, other, *span_of(p1, "A")))


class TestTempoAndVariants(unittest.TestCase):
    def test_tempo_rolls_stay_within_the_range_of_the_current_tempo(self):
        for seed in (2, 5, 11):
            rec = studio_engine.new_recipe(seed)
            prev = composer.compose(rec)["tempo"]
            for _ in range(12):
                rec = studio_engine.reroll(rec, "tempo", "all")
                cur = composer.compose(rec)["tempo"]
                lo = max(60, int(round(prev * 0.8)))
                hi = min(220, int(round(prev * 1.1)))
                self.assertTrue(lo <= cur <= hi, (seed, prev, cur))
                self.assertEqual(composer.compose(copy.deepcopy(rec))["tempo"], cur)
                prev = cur

    def test_single_b_keeps_one_variant(self):
        split_seen = False
        for seed in range(0, 40):
            rec = studio_engine.new_recipe(seed)
            if "B" not in {b["sec"] for b in rec["form"]}:
                continue
            for k in range(1, 4):
                rec["studio"]["salts"]["b@B"] = k
                _, tr = traced(rec)
                variants = {b["var"] for b in tr["form"] if b["sec"] == "B"}
                if rec["arrange"]["template"] == "double_b":
                    split_seen = split_seen or len(variants) == 2
                else:
                    self.assertEqual(len(variants), 1, (seed, k, variants))
        self.assertTrue(split_seen)


class RecordingRender:
    def __init__(self):
        self.calls = 0
        self.fail = False
        self.gate = None
        self.entered = threading.Event()

    def __call__(self, mid_path, mp3_path):
        self.calls += 1
        self.entered.set()
        if self.gate is not None:
            self.gate.wait(20)
        if self.fail:
            raise RuntimeError("render failed")
        digest = hashlib.sha256(Path(mid_path).read_bytes()).hexdigest().encode()
        Path(mp3_path).write_bytes(b"MP3:" + digest)


def mp3_matches_mid(testcase, pdir):
    digest = hashlib.sha256((pdir / "piece.mid").read_bytes()).hexdigest().encode()
    testcase.assertEqual((pdir / "piece.mp3").read_bytes(), b"MP3:" + digest)


def snapshot_files(pdir):
    return {p.name: p.read_bytes() for p in sorted(pdir.iterdir()) if p.is_file()}


class TestStorePublishing(unittest.TestCase):
    def test_failed_render_publishes_nothing(self):
        with tempfile.TemporaryDirectory() as d:
            render = RecordingRender()
            store = studio_engine.StudioStore(d, render_fn=render)
            pid = store.create_piece(801000)["id"]
            pdir = Path(d) / pid
            before = snapshot_files(pdir)
            render.fail = True
            with self.assertRaises(RuntimeError):
                store.reroll(pid, "comp", "A")
            self.assertEqual(snapshot_files(pdir), before)
            self.assertEqual([p.name for p in pdir.iterdir() if p.is_dir()], [])
            self.assertFalse(store.get_piece(pid)["can_undo"])
            render.fail = False
            store.set_lock(pid, "comp", "A", True)
            mp3_matches_mid(self, pdir)
            calls = render.calls
            store.reroll(pid, "texture", "A")
            self.assertGreater(render.calls, calls)
            mp3_matches_mid(self, pdir)
            lines = (pdir / "history.jsonl").read_text(encoding="utf-8").splitlines()
            self.assertEqual([json.loads(x)["action"] for x in lines], ["new", "lock", "reroll"])

    def test_failed_undo_keeps_the_stack(self):
        with tempfile.TemporaryDirectory() as d:
            render = RecordingRender()
            store = studio_engine.StudioStore(d, render_fn=render)
            pid = store.create_piece(801000)["id"]
            store.reroll(pid, "comp", "A")
            store.reroll(pid, "lead_rhythm", "all")
            pdir = Path(d) / pid
            before = snapshot_files(pdir)
            render.fail = True
            with self.assertRaises(RuntimeError):
                store.undo(pid)
            self.assertEqual(snapshot_files(pdir), before)
            render.fail = False
            store.undo(pid)
            store.undo(pid)
            self.assertFalse(store.get_piece(pid)["can_undo"])
            mp3_matches_mid(self, pdir)

    def test_failed_create_leaves_no_piece(self):
        with tempfile.TemporaryDirectory() as d:
            render = RecordingRender()
            render.fail = True
            store = studio_engine.StudioStore(d, render_fn=render)
            with self.assertRaises(RuntimeError):
                store.create_piece(801000)
            self.assertEqual(list(Path(d).iterdir()), [])
            self.assertEqual(store.list_pieces(), [])

    def test_render_of_one_piece_does_not_block_reads_of_another(self):
        with tempfile.TemporaryDirectory() as d:
            render = RecordingRender()
            store = studio_engine.StudioStore(d, render_fn=render)
            a = store.create_piece(801000)["id"]
            b = store.create_piece(802000)["id"]
            render.gate = threading.Event()
            render.entered.clear()
            done = {}

            def mutate(key, fn):
                done[key] = fn()

            t1 = threading.Thread(target=mutate, args=("first", lambda: store.reroll(a, "comp", "A")))
            t1.start()
            self.assertTrue(render.entered.wait(10))
            results = {}

            def reads():
                results["b"] = store.get_piece(b)
                results["b_audio"] = store.file_path(b, "piece.mp3")
                results["a"] = store.get_piece(a)
                results["a_audio"] = store.file_path(a, "piece.mp3")
                results["list"] = store.list_pieces()

            t2 = threading.Thread(target=reads)
            t2.start()
            t2.join(5)
            self.assertFalse(t2.is_alive(), "reads were blocked by a running render")
            self.assertFalse(results["a"]["can_undo"])
            self.assertEqual(len(results["list"]), 2)

            t3 = threading.Thread(target=mutate, args=("second", lambda: store.reroll(a, "texture", "A")))
            t3.start()
            t3.join(0.5)
            self.assertTrue(t3.is_alive(), "writes to one piece must be serialized")
            render.gate.set()
            t1.join(20)
            t3.join(20)
            self.assertTrue(done["second"]["can_undo"])
            self.assertEqual(len(store._read_undo(store._piece_dir(a))), 2)
            mp3_matches_mid(self, Path(d) / a)


def make_dir_link(link, target):
    try:
        link.symlink_to(target, target_is_directory=True)
        return True
    except (OSError, NotImplementedError):
        pass
    if sys.platform == "win32":
        import subprocess

        r = subprocess.run(["cmd", "/c", "mklink", "/J", str(link), str(target)], capture_output=True)
        return r.returncode == 0
    return False


class TestStoreContainment(unittest.TestCase):
    def test_piece_directory_link_outside_out_is_refused(self):
        with tempfile.TemporaryDirectory() as d, tempfile.TemporaryDirectory() as outside:
            root = Path(d) / "out"
            store = studio_engine.StudioStore(root, render_fn=studio_render.fake_render)
            pid = store.create_piece(801000)["id"]
            foreign = Path(outside) / "piece_x"
            shutil.copytree(root / pid, foreign)
            if not make_dir_link(root / "piece_link", foreign):
                self.skipTest("cannot create directory links here")
            for call in (
                lambda: store.get_piece("piece_link"),
                lambda: store.file_path("piece_link", "piece.mid"),
                lambda: store.reroll("piece_link", "comp", "A"),
                lambda: store.undo("piece_link"),
            ):
                with self.assertRaises(KeyError):
                    call()
            self.assertEqual([p["id"] for p in store.list_pieces()], [pid])
            self.assertEqual(
                (foreign / "recipe.json").read_bytes(), (root / pid / "recipe.json").read_bytes()
            )

    def test_file_link_outside_piece_is_refused(self):
        with tempfile.TemporaryDirectory() as d, tempfile.TemporaryDirectory() as outside:
            store = studio_engine.StudioStore(d, render_fn=studio_render.fake_render)
            pid = store.create_piece(801000)["id"]
            secret = Path(outside) / "secret.mid"
            secret.write_bytes(b"MThd-secret")
            target = Path(d) / pid / "piece.mid"
            target.unlink()
            try:
                target.symlink_to(secret)
            except (OSError, NotImplementedError):
                self.skipTest("cannot create file symlinks here")
            with self.assertRaises((KeyError, FileNotFoundError)):
                store.file_path(pid, "piece.mid")


class TestRequestValidation(unittest.TestCase):
    def _call(self, base, path, body=None, method="POST", raw=None):
        data = raw if raw is not None else (None if body is None else json.dumps(body).encode("utf-8"))
        req = urllib.request.Request(
            base + path, data=data, method=method, headers={"Content-Type": "application/json"}
        )
        try:
            with urllib.request.urlopen(req, timeout=10) as resp:
                return resp.status, json.loads(resp.read())
        except urllib.error.HTTPError as exc:
            return exc.code, json.loads(exc.read())

    def test_bad_requests_get_precise_codes(self):
        with tempfile.TemporaryDirectory() as d:
            srv = studio_server.create_server(d, host="127.0.0.1", port=0, render_fn=studio_render.fake_render)
            threading.Thread(target=srv.serve_forever, daemon=True).start()
            base = f"http://127.0.0.1:{srv.server_address[1]}"
            try:
                st, det = self._call(base, "/api/new", {"seed": 801000})
                self.assertEqual(st, 200)
                pid = det["id"]
                p = f"/api/piece/{pid}"
                pdir = Path(d) / pid
                before = snapshot_files(pdir)
                cases = [
                    ("/api/new", [], 400),
                    ("/api/new", "text", 400),
                    ("/api/new", {"seed": "7"}, 400),
                    ("/api/new", {"seed": True}, 400),
                    ("/api/new", {"seed": 1.5}, 400),
                    ("/api/new", {"seed": -3}, 400),
                    (p + "/reroll", {}, 400),
                    (p + "/reroll", {"layer": 3, "section": "A"}, 400),
                    (p + "/reroll", {"layer": "comp", "section": "B"}, 400),
                    (p + "/reroll", {"layer": "all", "section": "A"}, 400),
                    (p + "/lock", {"layer": "comp", "section": "A"}, 400),
                    (p + "/lock", {"layer": "comp", "section": "A", "locked": "false"}, 400),
                    (p + "/lock", {"layer": "comp", "section": "B", "locked": True}, 400),
                    (p + "/lock", {"layer": "all", "section": "all", "locked": True}, 400),
                    (p + "/option", {"name": "b_lead"}, 400),
                    (p + "/option", {"name": "nope", "value": 1}, 400),
                    (p + "/option", {"name": "b_lead", "value": "x"}, 400),
                    (p + "/option", {"name": "trailing_rest", "value": "false"}, 400),
                    (p + "/option", {"name": "octave_piece", "value": "0.5"}, 400),
                    (p + "/option", {"name": "octave_piece", "value": True}, 400),
                    (p + "/option", {"name": "slide_max", "value": 2.5}, 400),
                    ("/api/piece/missing/reroll", {"layer": "comp", "section": "A"}, 404),
                    ("/api/piece/missing/undo", {}, 404),
                    ("/api/nothing", {}, 404),
                ]
                for path, body, code in cases:
                    st, out = self._call(base, path, body)
                    self.assertEqual(st, code, (path, body, out))
                    self.assertIn("error", out)
                st, out = self._call(base, p + "/undo", raw=b"{not json")
                self.assertEqual(st, 400)
                st, out = self._call(base, p + "/undo", raw=b"x" * (1 << 20))
                self.assertEqual(st, 413)
                self.assertEqual(snapshot_files(pdir), before)
                st, out = self._call(base, p + "/reroll", {"layer": "all", "section": "all"})
                self.assertEqual(st, 200)
                st, out = self._call(base, p + "/lock", {"layer": "comp", "section": "A", "locked": False})
                self.assertEqual(st, 200)
            finally:
                srv.shutdown()
                srv.server_close()


class TestWslPaths(unittest.TestCase):
    def test_conversion(self):
        conv = studio_render.win_to_wsl_path
        self.assertEqual(conv("C:\\Users\\me\\x.mid"), "/mnt/c/Users/me/x.mid")
        self.assertEqual(conv("\\\\?\\C:\\Users\\me"), "/mnt/c/Users/me")
        self.assertEqual(conv("\\\\wsl.localhost\\NixOS\\home\\me\\x"), "/home/me/x")
        self.assertEqual(conv("\\\\wsl$\\NixOS\\home\\me"), "/home/me")
        self.assertEqual(conv("\\\\wsl.localhost\\nixos"), "/")
        with self.assertRaises(ValueError):
            conv("\\\\wsl.localhost\\Ubuntu\\home\\me")
        self.assertEqual(conv("/home/me/x"), "/home/me/x")


UI_HARNESS = r"""
const fs = require("fs"), vm = require("vm");
const html = fs.readFileSync(process.argv[2], "utf8");
const script = html.split("<script>")[1].split("</script>")[0];
const pending = {};
function el() {
  return { classList: { toggle() {}, add() {}, remove() {} }, style: {}, dataset: {}, children: [],
    appendChild(c) { this.children.push(c); }, addEventListener() {}, set innerHTML(v) {},
    get innerHTML() { return this._html || ""; }, set onclick(v) {}, value: "", currentTime: 0,
    play() { return Promise.resolve(); } };
}
const ctx = {
  document: { getElementById: () => el(), createElement: () => el(), addEventListener() {} },
  fetch: (url, opts) => new Promise((resolve) => {
    const key = url + (opts && opts.method ? " " + opts.method : "");
    (pending[key] = pending[key] || []).push((body) => resolve({ ok: true, json: async () => body }));
  }),
  alert() {}, console, Date, Math, Number, String, Promise, encodeURIComponent, parseInt, parseFloat,
};
vm.createContext(ctx);
vm.runInContext(script, ctx);
vm.runInContext("renderWorkspace = () => {}; renderList = () => {};", ctx);
const tick = () => new Promise((r) => setTimeout(r, 5));
const piece = (id, seed) => ({ id, seed: seed || 1, sections: [], whole_layers: [], options: {}, length: 1 });
const flushList = () => (pending["/api/pieces"] || []).splice(0).forEach((f) => f({ pieces: [] }));
const cur = (expr) => vm.runInContext(expr, ctx);
(async () => {
  await tick();
  flushList();
  await tick();
  const out = {};
  cur("selectPiece('a'); selectPiece('b');");
  await tick();
  pending["/api/piece/b"].shift()(piece("b"));
  await tick();
  pending["/api/piece/a"].shift()(piece("a"));
  await tick();
  out.afterSelect = cur("state.current && state.current.id");
  cur("selectPiece('a')");
  await tick();
  pending["/api/piece/a"].shift()(piece("a"));
  await tick();
  cur("mutate('/api/piece/a/reroll', {layer:'comp'}, 'a'); selectPiece('b');");
  await tick();
  pending["/api/piece/b"].shift()(piece("b"));
  await tick();
  pending["/api/piece/a/reroll POST"].shift()(piece("a"));
  await tick();
  flushList();
  await tick();
  out.afterMutate = cur("state.current && state.current.id");
  cur("mutate('/api/piece/b/undo', {}, 'b'); mutate('/api/piece/b/reroll', {}, 'b');");
  await tick();
  pending["/api/piece/b/reroll POST"].shift()(piece("b", 2));
  await tick();
  pending["/api/piece/b/undo POST"].shift()(piece("b", 1));
  await tick();
  flushList();
  await tick();
  out.latestSeed = cur("state.current.seed");
  console.log(JSON.stringify(out));
  process.exit(0);
})();
"""


class TestStudioPage(unittest.TestCase):
    def test_grid_does_not_overflow_a_narrow_viewport(self):
        html = studio_server.INDEX_HTML.read_text(encoding="utf-8")
        self.assertIn("minmax(min(100%, 260px), 1fr)", html)
        self.assertNotIn("minmax(260px, 1fr)", html)

    def test_stale_responses_do_not_change_the_selection(self):
        import subprocess

        node = shutil.which("node")
        if node is None:
            self.skipTest("node is not available")
        with tempfile.TemporaryDirectory() as d:
            harness = Path(d) / "harness.js"
            harness.write_text(UI_HARNESS, encoding="utf-8")
            res = subprocess.run(
                [node, str(harness), str(studio_server.INDEX_HTML)], capture_output=True, text=True, timeout=60
            )
        self.assertEqual(res.returncode, 0, res.stderr)
        out = json.loads(res.stdout.strip().splitlines()[-1])
        self.assertEqual(out["afterSelect"], "b")
        self.assertEqual(out["afterMutate"], "b")
        self.assertEqual(out["latestSeed"], 2)


class TestRedoStore(unittest.TestCase):
    def _store(self, d):
        return studio_engine.StudioStore(d, render_fn=studio_render.fake_render)

    def _history(self, store, pid):
        path = store._piece_dir(pid) / "history.jsonl"
        return [json.loads(x)["action"] for x in path.read_text(encoding="utf-8").splitlines() if x]

    def test_undo_then_redo_restores_recipe_and_swaps_stacks(self):
        with tempfile.TemporaryDirectory() as d:
            store = self._store(d)
            pid = store.create_piece(801000)["id"]
            pdir = store._piece_dir(pid)
            store.reroll(pid, "comp", "A")
            r1 = json.loads((pdir / "recipe.json").read_text(encoding="utf-8"))
            store.set_option(pid, "mute_share", 0.3)
            r2 = json.loads((pdir / "recipe.json").read_text(encoding="utf-8"))

            det = store.undo(pid)
            self.assertTrue(det["can_redo"])
            self.assertEqual(det["redo_label"], "глухие ноты 0.3")
            self.assertEqual(det["undo_label"], "перегенерация «Куплет A · ритм-гитара»")
            self.assertEqual(store._read_redo(pdir), [r2])
            self.assertEqual(json.loads((pdir / "recipe.json").read_text(encoding="utf-8")), r1)

            det = store.redo(pid)
            self.assertEqual(json.loads((pdir / "recipe.json").read_text(encoding="utf-8")), r2)
            self.assertFalse(det["can_redo"])
            self.assertTrue(det["can_undo"])
            self.assertEqual(det["undo_label"], "глухие ноты 0.3")
            self.assertEqual(store._read_redo(pdir), [])
            self.assertEqual(len(store._read_undo(pdir)), 2)
            self.assertFalse(store.get_piece(pid)["can_redo"])
            self.assertEqual(self._history(store, pid)[-2:], ["undo", "redo"])

    def test_new_change_clears_redo_and_noop_keeps_it(self):
        with tempfile.TemporaryDirectory() as d:
            store = self._store(d)
            pid = store.create_piece(801000)["id"]
            pdir = store._piece_dir(pid)
            store.reroll(pid, "comp", "A")
            store.undo(pid)
            self.assertEqual(len(store._read_redo(pdir)), 1)

            det = store.set_option(pid, "intro_bars", "auto")
            self.assertTrue(det["can_redo"])
            self.assertEqual(len(store._read_redo(pdir)), 1)

            det = store.set_option(pid, "mute_share", 0.5)
            self.assertFalse(det["can_redo"])
            self.assertEqual(store._read_redo(pdir), [])
            self.assertFalse(store.get_piece(pid)["can_redo"])

            store.undo(pid)
            self.assertTrue(store.get_piece(pid)["can_redo"])
            store.set_lock(pid, "b", "B", True)
            self.assertFalse(store.get_piece(pid)["can_redo"])

    def test_redo_and_undo_on_empty_stacks_are_noops_and_not_logged(self):
        with tempfile.TemporaryDirectory() as d:
            store = self._store(d)
            pid = store.create_piece(801000)["id"]
            pdir = store._piece_dir(pid)
            before = (pdir / "recipe.json").read_bytes()
            det = store.redo(pid)
            self.assertFalse(det["can_redo"])
            self.assertFalse(det["can_undo"])
            store.undo(pid)
            self.assertEqual((pdir / "recipe.json").read_bytes(), before)
            self.assertEqual(self._history(store, pid), ["new"])

    def test_redo_chain_follows_every_undone_step(self):
        with tempfile.TemporaryDirectory() as d:
            store = self._store(d)
            pid = store.create_piece(801000)["id"]
            pdir = store._piece_dir(pid)
            recipes = [json.loads((pdir / "recipe.json").read_text(encoding="utf-8"))]
            for _ in range(3):
                store.reroll(pid, "comp", "A")
                recipes.append(json.loads((pdir / "recipe.json").read_text(encoding="utf-8")))
            for _ in range(3):
                store.undo(pid)
            self.assertEqual(len(store._read_redo(pdir)), 3)
            for expected in recipes[1:]:
                store.redo(pid)
                self.assertEqual(json.loads((pdir / "recipe.json").read_text(encoding="utf-8")), expected)
            self.assertFalse(store.get_piece(pid)["can_redo"])

    def test_failed_redo_keeps_both_stacks(self):
        with tempfile.TemporaryDirectory() as d:
            calls = {"n": 0}

            def flaky(mid, mp3):
                calls["n"] += 1
                if calls["n"] == 4:
                    raise RuntimeError("render failed")
                studio_render.fake_render(mid, mp3)

            store = studio_engine.StudioStore(d, render_fn=flaky)
            pid = store.create_piece(801000)["id"]
            pdir = store._piece_dir(pid)
            store.reroll(pid, "comp", "A")
            store.undo(pid)
            redo_before = store._read_redo(pdir)
            undo_before = store._read_undo(pdir)
            with self.assertRaises(RuntimeError):
                store.redo(pid)
            self.assertEqual(store._read_redo(pdir), redo_before)
            self.assertEqual(store._read_undo(pdir), undo_before)
            store.redo(pid)
            self.assertFalse(store.get_piece(pid)["can_redo"])

    def test_unchanged_lock_requests_keep_history_and_redo(self):
        with tempfile.TemporaryDirectory() as d:
            store = self._store(d)
            pid = store.create_piece(801000)["id"]
            pdir = store._piece_dir(pid)
            store.set_lock(pid, "comp", "A", True)
            store.set_lock(pid, "b", "B", True)
            store.set_option(pid, "mute_share", 0.3)
            store.undo(pid)
            before = snapshot_files(pdir)
            for layer, section, locked in (("comp", "A", True), ("ending", "end", False)):
                det = store.set_lock(pid, layer, section, locked)
                self.assertTrue(det["can_redo"])
                self.assertEqual(snapshot_files(pdir), before)
            self.assertNotIn("locked_riffs", json.loads(before["recipe.json"])["studio"])

    def test_failed_undo_and_redo_leave_the_revision_intact_and_retryable(self):
        for action in ("undo", "redo"):
            with tempfile.TemporaryDirectory() as d:
                store = self._store(d)
                pid = store.create_piece(801000)["id"]
                pdir = store._piece_dir(pid)
                store.reroll(pid, "comp", "A")
                store.reroll(pid, "texture", "A")
                if action == "redo":
                    store.undo(pid)
                before = snapshot_files(pdir)
                before.pop("history.jsonl")
                original = store._write_redo

                def broken(*args, **kwargs):
                    raise PermissionError("denied")

                store._write_redo = broken
                with self.assertRaises(PermissionError):
                    getattr(store, action)(pid)
                store._write_redo = original
                after = snapshot_files(pdir)
                after.pop("history.jsonl")
                self.assertEqual(after, before)
                self.assertEqual(self._history(store, pid).count(action), 0)
                getattr(store, action)(pid)
                self.assertEqual(self._history(store, pid).count(action), 1)

    def test_failed_backup_leaves_no_stage_folder_and_no_changes(self):
        from unittest import mock

        with tempfile.TemporaryDirectory() as d:
            store = self._store(d)
            pid = store.create_piece(801000)["id"]
            pdir = store._piece_dir(pid)
            before = snapshot_files(pdir)
            names = sorted(p.name for p in pdir.iterdir())
            with mock.patch.object(studio_engine.shutil, "copy2", side_effect=OSError("disk")):
                with self.assertRaises(OSError):
                    store.reroll(pid, "comp", "A")
            self.assertEqual(sorted(p.name for p in pdir.iterdir()), names)
            self.assertEqual(snapshot_files(pdir), before)
            store.reroll(pid, "comp", "A")

    def test_history_stacks_are_bounded_to_50(self):
        with tempfile.TemporaryDirectory() as d:
            store = self._store(d)
            pid = store.create_piece(801000)["id"]
            pdir = store._piece_dir(pid)
            for _ in range(51):
                store.reroll(pid, "comp", "A")
            self.assertEqual(len(store._read_undo(pdir)), 50)
            for _ in range(51):
                store.undo(pid)
            self.assertEqual(len(store._read_redo(pdir)), 50)
            self.assertEqual(len(store._read_undo(pdir)), 0)

    def test_redo_endpoint_and_validation(self):
        with tempfile.TemporaryDirectory() as d:
            srv = studio_server.create_server(d, host="127.0.0.1", port=0, render_fn=studio_render.fake_render)
            port = srv.server_address[1]
            t = threading.Thread(target=srv.serve_forever, daemon=True)
            t.start()
            base = f"http://127.0.0.1:{port}"

            def post(path, body=None):
                req = urllib.request.Request(
                    base + path, data=json.dumps(body or {}).encode("utf-8"), method="POST",
                    headers={"Content-Type": "application/json"},
                )
                try:
                    with urllib.request.urlopen(req, timeout=10) as resp:
                        return resp.status, json.loads(resp.read())
                except urllib.error.HTTPError as err:
                    return err.code, json.loads(err.read())

            try:
                st, det = post("/api/new", {"seed": 801000})
                pid = det["id"]
                self.assertFalse(det["can_redo"])
                post(f"/api/piece/{pid}/reroll", {"layer": "comp", "section": "A"})
                st, det = post(f"/api/piece/{pid}/undo")
                self.assertTrue(det["can_redo"])
                st, det = post(f"/api/piece/{pid}/redo")
                self.assertEqual(st, 200)
                self.assertFalse(det["can_redo"])
                comp = next(
                    layer for s in det["sections"] if s["section"] == "A"
                    for layer in s["layers"] if layer["layer"] == "comp"
                )
                self.assertEqual(comp["salt"], 1)
                st, det = post(f"/api/piece/{pid}/redo")
                self.assertEqual(st, 200)
                self.assertFalse(det["can_redo"])
                self.assertEqual(post("/api/piece/missing/redo")[0], 404)
                self.assertEqual(post("/api/piece/bad%20id/redo")[0], 404)
            finally:
                srv.shutdown()
                srv.server_close()
                t.join(timeout=5)


PAGE_PRELUDE = r"""
const fs = require("fs"), vm = require("vm");
const html = fs.readFileSync(process.argv[2], "utf8");
const script = html.split("<script>")[1].split("</script>")[0];
const calls = [];
const alerts = [];
const ids = new Map();
const docListeners = {};
const STATIC = ["workspace", "pieceList", "busy", "newBtn", "seedInput", "statusChip"];
function makeEl(tag) {
  const el = {
    tagName: tag, id: "", className: "", value: "", textContent: "", title: "", type: "",
    min: "", max: "", step: "", disabled: false, hidden: false, href: "", download: "",
    style: {}, dataset: {}, children: [], _attrs: {},
    classList: {
      set: new Set(),
      toggle(k, f) { if (f === undefined) { if (this.set.has(k)) this.set.delete(k); else this.set.add(k); } else if (f) this.set.add(k); else this.set.delete(k); },
      add(...ks) { ks.forEach((k) => this.set.add(k)); },
      remove(...ks) { ks.forEach((k) => this.set.delete(k)); },
      contains(k) { return this.set.has(k); },
    },
    appendChild(c) { this.children.push(c); return c; },
    setAttribute(k, v) { this._attrs[k] = String(v); },
    getAttribute(k) { return k in this._attrs ? this._attrs[k] : null; },
    removeAttribute(k) { delete this._attrs[k]; },
    _ls: {},
    addEventListener(t, fn) { (this._ls[t] = this._ls[t] || []).push(fn); },
    set innerHTML(v) { this._html = v; this.children = []; if (this.id === "workspace") { for (const k of Array.from(ids.keys())) { if (!STATIC.includes(k)) ids.delete(k); } } },
    get innerHTML() { return this._html || ""; },
  };
  return el;
}
for (const id of STATIC) {
  const e = makeEl("div");
  e.id = id;
  ids.set(id, e);
}
let respond = (url, body) => ({ pieces: [], id: "p1", seed: 1, meter: "4/4", mode: "lydian", key: "E", tempo: 140, length: 10, sections: [], whole_layers: [], options: { octave_share: 0.5 }, can_undo: true });
const ctx = {
  document: {
    getElementById: (id) => {
      if (!ids.has(id)) {
        if (!html.includes('id="' + id + '"')) throw new Error("no element in the page markup: " + id);
        const e = makeEl("div");
        e.id = id;
        ids.set(id, e);
        ids.get("workspace").appendChild(e);
      }
      return ids.get(id);
    },
    createElement: (tag) => makeEl(tag),
    addEventListener: (t, fn) => { (docListeners[t] = docListeners[t] || []).push(fn); },
  },
  fetch: async (url, opts) => {
    const body = opts && opts.body ? JSON.parse(opts.body) : null;
    calls.push({ url, body });
    const out = await respond(url, body);
    if (out && out.__error) return { ok: false, statusText: "err", json: async () => ({ error: out.__error }) };
    return { ok: true, json: async () => out };
  },
  alert(msg) { alerts.push(String(msg)); },
  console, Date, Math, Number, String, Promise, encodeURIComponent, parseInt, parseFloat, setTimeout, clearTimeout, Array, Object, JSON,
};
vm.createContext(ctx);
vm.runInContext(script, ctx);
const run = (code) => vm.runInContext(code, ctx);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
function walk(node, pred, out = []) {
  if (pred(node)) out.push(node);
  for (const c of node.children || []) walk(c, pred, out);
  return out;
}
const root = () => ids.get("workspace");
const findAll = (pred) => walk(root(), pred);
const find = (pred) => findAll(pred)[0];
const piece = (extra) => Object.assign({ id: "p1", seed: 1, meter: "4/4", mode: "lydian", key: "E", tempo: 140, length: 10, sections: [], whole_layers: [], options: { octave_share: null, mute_share: 0 }, can_undo: false, can_redo: false, undo_label: null, redo_label: null }, extra || {});
const show = (extra) => { ctx.__p = piece(extra); run("state.current = __p; state.currentId = __p.id; renderWorkspace();"); };
"""


def _run_page(body_js, timeout=60):
    import subprocess

    node = shutil.which("node")
    if node is None:
        raise unittest.SkipTest("node is not available")
    with tempfile.TemporaryDirectory() as d:
        harness = Path(d) / "page_harness.js"
        harness.write_text(PAGE_PRELUDE + "\n(async () => {\n" + body_js + "\n})().then(() => process.exit(0), (e) => { console.error(e); process.exit(1); });\n", encoding="utf-8")
        res = subprocess.run(
            [node, str(harness), str(studio_server.INDEX_HTML)],
            capture_output=True, text=True, encoding="utf-8", timeout=timeout,
        )
    if res.returncode != 0:
        raise AssertionError(res.stderr)
    return json.loads(res.stdout.strip().splitlines()[-1])


class TestStudioPageControls(unittest.TestCase):
    def setUp(self):
        self.html = studio_server.INDEX_HTML.read_text(encoding="utf-8")

    def test_piece_wide_note_and_rerolls_only_hint(self):
        self.assertIn("Параметры действуют на всю пьесу, в том числе на закреплённые секции.", self.html)
        self.assertIn(
            "Эти параметры действуют только при следующей перегенерации ритм-гитары "
            "(«Перегенерировать ритм-гитару» или «Перегенерировать всё незакреплённое»).",
            self.html,
        )
        self.assertRegex(
            self.html,
            r'<summary>Дополнительно</summary>\s*<div class="advanced-note">[^<]*</div>\s*<div class="options-grid" id="advancedGrid">',
        )
        self.assertRegex(
            self.html,
            r'<h2>Параметры пьесы</h2>\s*<div class="params-note">[^<]*</div>\s*<div class="options-grid" id="optionsGrid">',
        )

    def test_plain_labels_without_internal_names(self):
        for bad in ("(b_lead)", "(trailing_rest)", "--notail", "Вкл (есть пауза)", "Выкл (", "Удвоенно (как сейчас)"):
            self.assertNotIn(bad, self.html)
        for good in (
            "Соло в середине B",
            "Медленнее (удвоенные длительности)",
            "В темпе рифа",
            "Без соло",
            "Пауза в конце такта",
            "Без паузы",
            "Сколько раз перегенерировали",
            "Номер пьесы (seed), необязательно",
            "Глухие удары перед акцентами (только в октавных тактах, при перегенерации)",
        ):
            self.assertIn(good, self.html)
        self.assertNotIn("Глухие перед акцентами в октавах", self.html)
        self.assertRegex(self.html, r'<label[^>]*for="seedInput"')

    def test_lock_toggle_choice_groups_and_zero_badge(self):
        self.assertIn("🔒 Закреплено", self.html)
        self.assertIn("Слой закреплён", self.html)
        self.assertIn("aria-pressed", self.html)
        out = _run_page("""
const layer = (locked, salt) => ({ layer: "comp", section: "A", label: "Перегенерировать ритм-гитару", salt, locked });
const mk = (l) => run("makeLayerControl('p1', " + JSON.stringify(l) + ")");
const open = mk(layer(false, 0));
const closed = mk(layer(true, 2));
const info = (row) => ({
  lockText: row.children[1].textContent, lockTitle: row.children[1].title,
  pressed: row.children[1].getAttribute("aria-pressed"),
  rerollTitle: row.children[0].title, rerollDisabled: row.children[0].disabled,
  badge: walk(row, (e) => e.className === "salt-badge").map((b) => ({ hidden: b.hidden, title: b.title, text: b.textContent })),
});
show({ options: { octave_share: null, mute_share: 0, trailing_rest: true, b_lead: "augmented", intro_bars: "auto", square_repeat: 1 } });
const tail = findAll((e) => e.dataset && e.dataset.option === "trailing_rest").map((b) => [b.textContent, b.getAttribute("aria-pressed")]);
const bl = findAll((e) => e.dataset && e.dataset.option === "b_lead").map((b) => [b.textContent, b.getAttribute("aria-pressed")]);
console.log(JSON.stringify({ open: info(open), closed: info(closed), tail, bl }));
""")
        self.assertEqual(out["open"]["lockText"], "Закрепить")
        self.assertEqual(out["open"]["pressed"], "false")
        self.assertEqual(out["open"]["rerollTitle"], "")
        self.assertEqual(out["open"]["badge"], [{"hidden": True, "title": "Сколько раз перегенерировали", "text": "#0"}])
        self.assertEqual(out["closed"]["lockText"], "🔒 Закреплено")
        self.assertEqual(out["closed"]["lockTitle"], "Открепить")
        self.assertEqual(out["closed"]["pressed"], "true")
        self.assertEqual(out["closed"]["rerollTitle"], "Слой закреплён")
        self.assertTrue(out["closed"]["rerollDisabled"])
        self.assertFalse(out["closed"]["badge"][0]["hidden"])
        self.assertEqual(out["tail"], [["Пауза в конце такта", "true"], ["Без паузы", "false"]])
        self.assertEqual(
            out["bl"],
            [["Медленнее (удвоенные длительности)", "true"], ["В темпе рифа", "false"], ["Без соло", "false"]],
        )

    def test_octave_switch_selected_button_does_nothing(self):
        self.assertIn("в секциях A, A2, «Кульминация» и B; применяется сразу.", self.html)
        out = _run_page("""
show({ options: { octave_share: null, mute_share: 0 } });
const btn = (v) => find((e) => e.dataset && e.dataset.option === "octave_share" && e.dataset.value === v && e.tagName === "button");
const first = { auto: btn("auto").textContent, manual: btn("manual").textContent, aut: btn("auto").getAttribute("aria-pressed"), man: btn("manual").getAttribute("aria-pressed") };
btn("auto").onclick();
await sleep(10);
const afterAuto = calls.filter((c) => c.url.endsWith("/option")).length;
btn("manual").onclick();
await sleep(10);
show({ options: { octave_share: 0.5, mute_share: 0 } });
btn("manual").onclick();
await sleep(10);
btn("auto").onclick();
await sleep(10);
console.log(JSON.stringify({ first, afterAuto, sent: calls.filter((c) => c.url.endsWith("/option")).map((c) => c.body) }));
""")
        self.assertEqual(out["first"], {"auto": "Авто", "manual": "Вручную", "aut": "true", "man": "false"})
        self.assertEqual(out["afterAuto"], 0)
        self.assertEqual(out["sent"], [{"name": "octave_share", "value": 0.5}, {"name": "octave_share", "value": None}])

    def test_slider_sends_once_per_gesture(self):
        out = _run_page("""
show();
const slider = () => find((e) => e.dataset && e.dataset.option === "mute_share" && e.type === "range");
const count = () => calls.filter((c) => c.url.endsWith("/option")).length;
for (const v of ["0.05", "0.1", "0.15"]) { slider().value = v; slider().oninput(); await sleep(100); }
const early = count();
await sleep(650);
const afterKeys = calls.filter((c) => c.url.endsWith("/option")).map((c) => c.body);
slider().onpointerdown();
for (const v of ["0.2", "0.4", "0.6"]) { slider().value = v; slider().oninput(); }
await sleep(700);
const during = count();
slider().onpointerup();
await sleep(50);
await sleep(700);
console.log(JSON.stringify({ early, afterKeys, during, total: count(), last: calls.filter((c) => c.url.endsWith("/option")).pop().body }));
""")
        self.assertEqual(out["early"], 0)
        self.assertEqual(out["afterKeys"], [{"name": "mute_share", "value": 0.15}])
        self.assertEqual(out["during"], 1)
        self.assertEqual(out["total"], 2)
        self.assertEqual(out["last"], {"name": "mute_share", "value": 0.6})

    def test_sliders_are_named_and_valued_for_assistive_tech(self):
        out = _run_page("""
show();
const sliders = findAll((e) => e.type === "range");
const mute = sliders.find((e) => e.dataset.option === "mute_share");
mute.value = "0.35";
mute.oninput();
console.log(JSON.stringify({
  n: sliders.length,
  named: sliders.every((e) => (e.getAttribute("aria-label") || "").length > 0),
  valued: sliders.every((e) => (e.getAttribute("aria-valuetext") || "").length > 0),
  muteText: mute.getAttribute("aria-valuetext"),
  octaveText: sliders.find((e) => e.dataset.option === "octave_share").getAttribute("aria-valuetext"),
}));
""")
        self.assertEqual(out["n"], 6)
        self.assertTrue(out["named"])
        self.assertTrue(out["valued"])
        self.assertEqual(out["muteText"], "0.35")
        self.assertEqual(out["octaveText"], "авто")
        self.assertRegex(self.html, r'id="statusChip"[^>]*role="status"|role="status"[^>]*id="statusChip"')

    def test_status_chip_and_pressed_control_while_a_request_runs(self):
        out = _run_page("""
show();
let release;
respond = () => new Promise((resolve) => { release = () => resolve(piece()); });
const btn = (v) => find((e) => e.dataset && e.dataset.option === "intro_bars" && e.dataset.value === v);
const b4 = btn("4");
b4.onclick();
await sleep(10);
const during = { chip: ids.get("statusChip").textContent, disabled: b4.disabled };
release();
await sleep(20);
console.log(JSON.stringify({ during, after: ids.get("statusChip").textContent }));
""")
        self.assertEqual(out["during"], {"chip": "Пересчёт…", "disabled": True})
        self.assertEqual(out["after"], "")

    def test_errors_use_a_dismissible_banner_not_alert(self):
        self.assertNotRegex(self.html, r"\balert\s*\(")
        out = _run_page("""
show();
respond = () => ({ __error: "boom" });
find((e) => e.dataset && e.dataset.option === "intro_bars" && e.dataset.value === "4").onclick();
await sleep(20);
const shown = { hidden: ids.get("errorBanner").hidden, text: ids.get("errorText").textContent };
ids.get("errorCloseBtn").onclick();
console.log(JSON.stringify({ shown, alerts, hiddenAfter: ids.get("errorBanner").hidden, textAfter: ids.get("errorText").textContent }));
""")
        self.assertFalse(out["shown"]["hidden"])
        self.assertIn("boom", out["shown"]["text"])
        self.assertEqual(out["alerts"], [])
        self.assertTrue(out["hiddenAfter"])
        self.assertEqual(out["textAfter"], "")

    def test_undo_flushes_a_pending_slider_edit_first(self):
        out = _run_page("""
show({ can_undo: true, undo_label: "длина вступления 8" });
const slider = find((e) => e.dataset && e.dataset.option === "mute_share" && e.type === "range");
slider.value = "0.35";
slider.oninput();
ids.get("undoBtn").onclick();
await sleep(50);
await sleep(700);
console.log(JSON.stringify({ urls: calls.map((c) => c.url.replace("/api/piece/p1/", "")).filter((u) => !u.includes("pieces")) }));
""")
        self.assertEqual(out["urls"], ["option", "undo"])

    def test_switching_pieces_flushes_a_pending_slider_edit_whatever_the_latency(self):
        out = _run_page("""
show();
run("state.pieces = " + JSON.stringify([{ id: "p1", seed: 1 }, { id: "p2", seed: 2 }]));
respond = async (url, body) => {
  if (url.endsWith("/api/piece/p2")) { await sleep(700); return piece({ id: "p2", seed: 2 }); }
  if (url.endsWith("/api/pieces")) return { pieces: [] };
  return piece();
};
const slider = find((e) => e.dataset && e.dataset.option === "mute_share" && e.type === "range");
slider.value = "0.35";
slider.oninput();
run("selectPiece('p2')");
await sleep(900);
const sent = calls.filter((c) => c.url.endsWith("/option") || c.url.endsWith("/api/piece/p2")).map((c) => c.url);
console.log(JSON.stringify({ first: calls.find((c) => c.url.endsWith("/option")), sent, currentId: run("state.currentId"), shown: run("state.current.id") }));
""")
        self.assertEqual(out["first"], {"url": "/api/piece/p1/option", "body": {"name": "mute_share", "value": 0.35}})
        self.assertEqual(out["sent"], ["/api/piece/p1/option", "/api/piece/p2"])
        self.assertEqual(out["currentId"], "p2")
        self.assertEqual(out["shown"], "p2")

    def test_failed_follow_up_request_still_shows_the_saved_state(self):
        out = _run_page("""
show();
respond = (url) => {
  if (url.endsWith("/api/pieces")) return { __error: "list failed" };
  return piece({ options: { octave_share: null, mute_share: 0, intro_bars: 4, square_repeat: 1, b_lead: "augmented", trailing_rest: true } });
};
find((e) => e.dataset && e.dataset.option === "intro_bars" && e.dataset.value === "4").onclick();
await sleep(30);
const b4 = find((e) => e.dataset && e.dataset.option === "intro_bars" && e.dataset.value === "4");
const next = find((e) => e.dataset && e.dataset.option === "intro_bars" && e.dataset.value === "1");
console.log(JSON.stringify({ pressed: b4.getAttribute("aria-pressed"), disabled: [b4.disabled, next.disabled], hidden: ids.get("errorBanner").hidden, text: ids.get("errorText").textContent }));
""")
        self.assertEqual(out["pressed"], "true")
        self.assertEqual(out["disabled"], [False, False])
        self.assertFalse(out["hidden"])
        self.assertIn("list failed", out["text"])

    def test_failed_piece_switch_redisplays_the_current_piece(self):
        out = _run_page("""
show();
run("state.pieces = " + JSON.stringify([{ id: "p1", seed: 1 }, { id: "p2", seed: 2 }]));
respond = async (url) => {
  if (url.endsWith("/api/piece/p2")) return { __error: "no p2" };
  if (url.endsWith("/api/pieces")) return { pieces: [] };
  await sleep(40);
  return piece({ options: { octave_share: null, mute_share: 0, intro_bars: 4, square_repeat: 1, b_lead: "augmented", trailing_rest: true } });
};
find((e) => e.dataset && e.dataset.option === "intro_bars" && e.dataset.value === "4").onclick();
await sleep(10);
run("selectPiece('p2')");
await sleep(200);
const b4 = find((e) => e.dataset && e.dataset.option === "intro_bars" && e.dataset.value === "4");
console.log(JSON.stringify({ currentId: run("state.currentId"), pressed: b4.getAttribute("aria-pressed"), stuck: findAll((e) => e.tagName === "button" && e.disabled && !["undoBtn", "redoBtn"].includes(e.id) && e.className !== "reroll-btn").length, hidden: ids.get("errorBanner").hidden }));
""")
        self.assertEqual(out["currentId"], "p1")
        self.assertEqual(out["pressed"], "true")
        self.assertEqual(out["stuck"], 0)
        self.assertFalse(out["hidden"])

    def test_rejected_slider_edit_restores_saved_value_and_pressed_state(self):
        out = _run_page("""
show({ options: { octave_share: null, mute_share: 0 } });
respond = () => ({ __error: "rejected" });
const slider = () => find((e) => e.dataset && e.dataset.option === "octave_share" && e.type === "range");
const btn = (v) => find((e) => e.dataset && e.dataset.option === "octave_share" && e.dataset.value === v && e.tagName === "button");
slider().value = "0.75";
slider().onpointerdown();
slider().oninput();
slider().onpointerup();
await sleep(30);
const afterFail = { auto: btn("auto").getAttribute("aria-pressed"), manual: btn("manual").getAttribute("aria-pressed"), disabled: slider().disabled };
respond = () => piece({ options: { octave_share: 0.5, mute_share: 0 } });
calls.length = 0;
btn("manual").onclick();
await sleep(30);
console.log(JSON.stringify({ afterFail, retried: calls.filter((c) => c.url.endsWith("/option")).length }));
""")
        self.assertEqual(out["afterFail"], {"auto": "true", "manual": "false", "disabled": False})
        self.assertEqual(out["retried"], 1)

    def test_error_banner_does_not_follow_the_user_to_another_piece(self):
        out = _run_page("""
show();
run("state.pieces = " + JSON.stringify([{ id: "p1", seed: 1 }, { id: "p2", seed: 2 }]));
respond = () => ({ __error: "boom" });
find((e) => e.dataset && e.dataset.option === "intro_bars" && e.dataset.value === "4").onclick();
await sleep(30);
const shown = ids.get("errorBanner").hidden;
respond = (url) => url.endsWith("/api/pieces") ? { pieces: [] } : piece({ id: "p2", seed: 2 });
await run("selectPiece('p2')");
console.log(JSON.stringify({ shown, hiddenAfter: ids.get("errorBanner").hidden, text: ids.get("errorText").textContent }));
""")
        self.assertFalse(out["shown"])
        self.assertTrue(out["hiddenAfter"])
        self.assertEqual(out["text"], "")

    def test_selected_piece_is_current_and_labels_are_plain_russian(self):
        self.assertIn("в секциях A, A2, «Кульминация» и B", self.html)
        out = _run_page("""
run("state.pieces = " + JSON.stringify([{ id: "p1", seed: 1, meter: "4/4", key: "E", mode: "mixolydian", tempo: 120, length: 10 }, { id: "p2", seed: 2, meter: "4/4", key: "A", mode: "aeolian", tempo: 120, length: 10 }]));
show({ mode: "mixolydian" });
run("renderList()");
const items = ids.get("pieceList").children;
console.log(JSON.stringify({ cur: items.map((b) => b.getAttribute("aria-current")), text: items.map((b) => b.innerHTML) }));
""")
        self.assertEqual(out["cur"], ["true", None])
        self.assertIn("миксолидийский", out["text"][0])
        self.assertIn("минор (эолийский)", out["text"][1])

    def test_banner_markup_is_in_the_production_page(self):
        self.assertRegex(
            self.html,
            r'<main id="workspace">\s*<div id="errorBanner"[^>]*role="alert"[^>]*hidden>\s*<span id="errorText"></span>\s*<button id="errorCloseBtn"',
        )

    def test_every_pending_slider_is_flushed_in_the_order_it_was_moved(self):
        out = _run_page("""
show({ can_undo: true, undo_label: "длина вступления 8" });
const slider = (n) => find((e) => e.dataset && e.dataset.option === n && e.type === "range");
slider("mute_share").value = "0.35";
slider("mute_share").oninput();
await sleep(100);
slider("octave_piece").value = "0.5";
slider("octave_piece").oninput();
await sleep(50);
ids.get("undoBtn").onclick();
await sleep(100);
await sleep(700);
console.log(JSON.stringify({ sent: calls.filter((c) => !c.url.endsWith("/api/pieces")).map((c) => c.url.replace("/api/piece/p1/", "") + (c.body && c.body.name ? ":" + c.body.name : "")) }));
""")
        self.assertEqual(out["sent"], ["option:mute_share", "option:octave_piece", "undo"])

    def test_superseded_edit_that_saved_is_shown_after_a_later_edit_fails(self):
        out = _run_page("""
show();
const opts = (extra) => Object.assign({ octave_share: null, mute_share: 0, intro_bars: "auto", square_repeat: 1, b_lead: "augmented", trailing_rest: true }, extra);
let saved = opts({});
respond = async (url, body) => {
  if (url.endsWith("/option") && body.name === "intro_bars") { await sleep(40); saved = opts({ intro_bars: 4 }); return piece({ options: saved }); }
  if (url.endsWith("/option")) { await sleep(80); return { __error: "rejected" }; }
  if (url.endsWith("/api/pieces")) return { pieces: [] };
  return piece({ options: saved });
};
find((e) => e.dataset && e.dataset.option === "intro_bars" && e.dataset.value === "4").onclick();
await sleep(5);
find((e) => e.dataset && e.dataset.option === "square_repeat" && e.dataset.value === "2").onclick();
await sleep(250);
const b4 = find((e) => e.dataset && e.dataset.option === "intro_bars" && e.dataset.value === "4");
console.log(JSON.stringify({ pressed: b4.getAttribute("aria-pressed"), hidden: ids.get("errorBanner").hidden }));
""")
        self.assertEqual(out["pressed"], "true")
        self.assertFalse(out["hidden"])

    def test_undo_names_the_action_and_redo_posts_to_the_redo_endpoint(self):
        out = _run_page("""
show({ can_undo: true, undo_label: "глухие ноты 0.3", can_redo: true, redo_label: "перегенерация «Куплет A · ритм-гитара»" });
const undo = ids.get("undoBtn"), redo = ids.get("redoBtn");
const info = { undoTitle: undo.title, undoHint: walk(undo, (e) => e.className === "btn-hint").map((e) => e.textContent), redoTitle: redo.title, redoDisabled: redo.disabled, redoText: redo.textContent };
redo.onclick();
await sleep(10);
show();
const off = { undoTitle: ids.get("undoBtn").title, redoDisabled: ids.get("redoBtn").disabled };
console.log(JSON.stringify({ info, off, urls: calls.map((c) => c.url) }));
""")
        self.assertEqual(out["info"]["undoTitle"], "Отменить: глухие ноты 0.3")
        self.assertEqual(out["info"]["undoHint"], [": глухие ноты 0.3"])
        self.assertEqual(out["info"]["redoTitle"], "Вернуть: перегенерация «Куплет A · ритм-гитара»")
        self.assertFalse(out["info"]["redoDisabled"])
        self.assertTrue(out["info"]["redoText"].startswith("Вернуть"))
        self.assertIn("/api/piece/p1/redo", out["urls"])
        self.assertTrue(out["off"]["redoDisabled"])
        self.assertEqual(out["off"]["undoTitle"], "Отменить")

    def test_timeline_short_labels_for_narrow_screens(self):
        for short in ("Вст.", "Кульм.", "Конц."):
            self.assertIn(short, self.html)
        self.assertRegex(self.html, r"@media \(max-width: 520px\)[\s\S]*\.name-short")
        out = _run_page("""
show({ length: 10, sections: [
  { section: "intro", label: "Вступление", start_sec: 0, end_sec: 2, layers: [] },
  { section: "A3", label: "Кульминация", start_sec: 2, end_sec: 10, layers: [] },
] });
const blocks = findAll((e) => e.className && e.className.startsWith("sec-block"));
console.log(JSON.stringify(blocks.map((b) => ({
  title: b.title, aria: b.getAttribute("aria-label"),
  short: walk(b, (e) => e.className === "name-short").map((e) => e.textContent),
  full: walk(b, (e) => e.className === "name-full").map((e) => e.textContent),
}))));
""")
        self.assertEqual([b["title"] for b in out], ["Вступление", "Кульминация"])
        self.assertEqual([b["short"] for b in out], [["Вст."], ["Кульм."]])
        self.assertEqual([b["full"] for b in out], [["Вступление"], ["Кульминация"]])
        for b in out:
            self.assertTrue(b["aria"].startswith(b["title"]))


class TestStudioControls(unittest.TestCase):
    def _v8_plain(self, seed):
        args = argparse.Namespace(meter=None, mode=None, key=None, tempo=None, v8=True)
        return composer.make_recipe(seed, args)

    def test_defaults_byte_identical(self):
        rng = random.Random(20261123)
        rand_seeds = rng.sample(range(1000, 999999), 20)
        self.assertEqual(set(rand_seeds), set(GOLDEN_RANDOM_V8_MID_SHA256))
        seeds = [s * 1000 for s in (801, 802, 805, 807, 809, 810)] + rand_seeds
        for seed in seeds:
            plain = self._v8_plain(seed)
            rec = studio_engine.new_recipe(seed)
            rec_omitted = copy.deepcopy(rec)
            for k in ("mute_share", "octave_share", "intro_bars", "square_repeat"):
                rec_omitted["studio"]["options"].pop(k, None)
            h_plain = midi_sha256(composer.compose(plain))
            h_rec = midi_sha256(composer.compose(rec))
            h_omitted = midi_sha256(composer.compose(rec_omitted))
            self.assertEqual(h_rec, h_plain, seed)
            self.assertEqual(h_omitted, h_plain, seed)
            if seed in GOLDEN_RANDOM_V8_MID_SHA256:
                self.assertEqual(h_plain, GOLDEN_RANDOM_V8_MID_SHA256[seed], seed)
                self.assertEqual(h_rec, GOLDEN_RANDOM_V8_MID_SHA256[seed], seed)
                self.assertEqual(h_omitted, GOLDEN_RANDOM_V8_MID_SHA256[seed], seed)
        for s, expected in GOLDEN_V8_MID_SHA256.items():
            self.assertEqual(midi_sha256(composer.compose(studio_engine.new_recipe(s * 1000))), expected, s)

    def test_mute_share(self):
        seeds_20 = [s * 1000 for s in range(801, 821)]
        target_secs = {"A", "A2", "A3", "B"}

        for seed in seeds_20:
            rec0 = studio_engine.new_recipe(seed)
            p0 = composer.compose(rec0)
            rec1 = studio_engine.set_option(rec0, "mute_share", 1.0)
            p1 = composer.compose(rec1)
            lead = rec0["intent"]["lead"]
            other = other_of(rec0)

            self.assertEqual(p0["parts"][lead], p1["parts"][lead], ("lead changed", seed))

            spans = section_spans(p0)
            self.assertEqual(spans, section_spans(p1), seed)
            for sn, s0, e0 in spans:
                ns0 = sorted((n for n in p0["parts"][other] if s0 <= n["start"] < e0), key=lambda n: (n["start"], n["pitch"]))
                ns1 = sorted((n for n in p1["parts"][other] if s0 <= n["start"] < e0), key=lambda n: (n["start"], n["pitch"]))
                if sn in target_secs:
                    self.assertEqual(len(ns0), len(ns1), (seed, sn))
                    for a, b in zip(ns0, ns1):
                        self.assertEqual(
                            (a["start"], a["pitch"], a["string"], a["fret"]),
                            (b["start"], b["pitch"], b["string"], b["fret"]),
                            (seed, sn),
                        )
                        self.assertIn("dead", b.get("tech", []), (seed, sn, b))
                        self.assertEqual(b["dur"], 1, (seed, sn, b))
                        if "dead" in a.get("tech", []):
                            self.assertEqual(b["vel"], a["vel"], (seed, sn, b))
                        else:
                            self.assertEqual(b["vel"], 40, (seed, sn, b))
                else:
                    self.assertEqual(ns0, ns1, ("non-target section changed", seed, sn))

        # mute_share=0.3 share, onset grouping, determinism, and undo
        muted_onsets = 0
        total_onsets = 0
        for seed in seeds_20:
            rec0 = studio_engine.new_recipe(seed)
            p0 = composer.compose(rec0)
            rec_m1 = studio_engine.set_option(rec0, "mute_share", 0.3)
            rec_m2 = studio_engine.set_option(rec0, "mute_share", 0.3)
            p_m1 = composer.compose(rec_m1)
            self.assertEqual(midi_sha256(p_m1), midi_sha256(composer.compose(rec_m2)), seed)
            other = other_of(rec0)

            pre_dead_onsets = set()
            for n in p0["parts"][other]:
                if "dead" in n.get("tech", []):
                    pre_dead_onsets.add(n["start"])

            for sn, s0, e0 in section_spans(p_m1):
                if sn not in target_secs:
                    continue
                by_onset = {}
                for n in p_m1["parts"][other]:
                    if s0 <= n["start"] < e0:
                        by_onset.setdefault(n["start"], []).append(n)
                for on, grp in by_onset.items():
                    if on in pre_dead_onsets:
                        continue
                    total_onsets += 1
                    deads = ["dead" in n.get("tech", []) for n in grp]
                    self.assertTrue(all(deads) or not any(deads), ("partial onset mute", seed, on, grp))
                    if all(deads):
                        muted_onsets += 1

        share = muted_onsets / max(1, total_onsets)
        self.assertTrue(0.2 <= share <= 0.4, (muted_onsets, total_onsets, share))

        with tempfile.TemporaryDirectory() as d:
            store = studio_engine.StudioStore(d, render_fn=studio_render.fake_render)
            det0 = store.create_piece(801000)
            pid = det0["id"]
            pdir = Path(d) / pid
            mid0 = (pdir / "piece.mid").read_bytes()
            store.set_option(pid, "mute_share", 0.5)
            mid1 = (pdir / "piece.mid").read_bytes()
            self.assertNotEqual(mid0, mid1)
            store.undo(pid)
            self.assertEqual((pdir / "piece.mid").read_bytes(), mid0)

    def test_octave_share(self):
        seeds_20 = [s * 1000 for s in range(801, 821)]
        for seed in seeds_20:
            rec0 = studio_engine.new_recipe(seed)
            p0 = composer.compose(rec0)
            rec_auto = studio_engine.set_option(rec0, "octave_share", None)
            self.assertEqual(midi_sha256(p0), midi_sha256(composer.compose(rec_auto)), seed)

            other = other_of(rec0)
            rec1 = studio_engine.set_option(rec0, "octave_share", 1.0)
            p1, t1 = traced(rec1)
            for i, b in enumerate(t1["form"]):
                if b["sec"] in ("A", "A2") and (b.get("role") or b.get("prog") == "cad_open") and not b.get("tex"):
                    self.assertEqual(b["role"], "octave", (seed, i, b))
                    s0 = p1["chords"][i]["start"]
                    s1 = p1["chords"][i + 1]["start"] if i + 1 < len(p1["chords"]) else p1["total16"]
                    in_bar = [n for n in p1["parts"][other] if s0 <= n["start"] < s1]
                    dyads = [n for n in in_bar if n.get("dyad")]
                    self.assertTrue(dyads, (seed, i))
                    self.assertEqual(len(dyads), len(in_bar), (seed, i, in_bar))
                    by_on = {}
                    for n in dyads:
                        by_on.setdefault(n["start"], []).append(n)
                    for pair in by_on.values():
                        self.assertEqual(sorted(n["string"] for n in pair), [1, 3], (seed, pair))
                        lo, hi = sorted(n["pitch"] for n in pair)
                        self.assertEqual(hi - lo, 12, (seed, pair))

            rec_zero = studio_engine.set_option(rec0, "octave_share", 0.0)
            p_zero, t_zero = traced(rec_zero)
            for i, b in enumerate(t_zero["form"]):
                if b["sec"] in ("A", "A2"):
                    self.assertNotEqual(b.get("role"), "octave", (seed, i, b))
                    s0 = p_zero["chords"][i]["start"]
                    s1 = p_zero["chords"][i + 1]["start"] if i + 1 < len(p_zero["chords"]) else p_zero["total16"]
                    dyads = [n for n in p_zero["parts"][other] if s0 <= n["start"] < s1 and n.get("dyad")]
                    self.assertEqual(dyads, [], (seed, i))

        # Regression check on seed 801000: 4th bar of A (cad_open) and A2 cadence must play octaves, not [62, 65, 69, 74]
        rec_801 = studio_engine.set_option(studio_engine.new_recipe(801000), "octave_share", 1.0)
        p_801, t_801 = traced(rec_801)
        other_801 = other_of(rec_801)
        cad_bars = [i for i, b in enumerate(t_801["form"]) if b["sec"] in ("A", "A2") and b["prog"] == "cad_open"]
        self.assertTrue(cad_bars)
        for bi in cad_bars:
            s0 = p_801["chords"][bi]["start"]
            s1 = p_801["chords"][bi + 1]["start"] if bi + 1 < len(p_801["chords"]) else p_801["total16"]
            bar_notes = [n for n in p_801["parts"][other_801] if s0 <= n["start"] < s1]
            self.assertTrue(bar_notes and all(n.get("dyad") for n in bar_notes), (bi, bar_notes))
            self.assertNotIn([62, 65, 69, 74], [sorted(n["pitch"] for n in bar_notes if n["start"] == s0)])

    def test_mute_share_preserves_repeat_and_locks(self):
        rec0 = studio_engine.new_recipe(801000)
        rec_rep = studio_engine.set_option(rec0, "mute_share", 0.3)
        rec_rep = studio_engine.set_option(rec_rep, "square_repeat", 2)
        p_rep = composer.compose(rec_rep)
        for sec in ("A", "A2"):
            s0, e0 = span_of(p_rep, sec)
            half = (e0 - s0) // 2
            for g in ("g1", "g2"):
                self.assertEqual(
                    section_notes(p_rep, g, s0, s0 + half),
                    section_notes(p_rep, g, s0 + half, e0),
                    (sec, g),
                )

        rec_lock = studio_engine.set_option(rec0, "mute_share", 0.3)
        rec_lock = studio_engine.set_lock(rec_lock, "comp", "A", True)
        p_a0 = composer.compose(rec_lock)
        other = other_of(rec_lock)
        p_a1 = composer.compose(studio_engine.reroll(rec_lock, "intro", "intro"))
        self.assertEqual(
            section_notes(p_a0, other, *span_of(p_a0, "A")),
            section_notes(p_a1, other, *span_of(p_a1, "A")),
        )
        p_a2 = composer.compose(studio_engine.set_option(rec_lock, "intro_bars", 8))
        self.assertEqual(
            section_notes(p_a0, other, *span_of(p_a0, "A")),
            section_notes(p_a2, other, *span_of(p_a2, "A")),
        )

    def test_mute_share_clears_incoming_legato_from_dead_notes(self):
        stripped_cases = 0
        for seed in [s * 1000 for s in range(801, 821)]:
            rec0 = studio_engine.new_recipe(seed)
            p0 = composer.compose(rec0)
            other = other_of(rec0)
            orig_legato = {
                (n["start"], n["string"])
                for n in p0["parts"][other]
                if {"hammer", "pull", "slide"} & set(n.get("tech", []))
            }
            for share in (0.3, 1.0):
                rec = studio_engine.set_option(rec0, "mute_share", share)
                p = composer.compose(rec)
                last_str = {}
                prev_line = None
                for n in sorted(p["parts"][other], key=lambda x: (x["start"], x["string"], x["pitch"])):
                    p_str = last_str.get(n["string"])
                    is_line = (
                        not n.get("chordtone")
                        and not n.get("dyad")
                        and not (n.get("fixed") and "preset_tech" not in n)
                    )
                    if (n["start"], n["string"]) in orig_legato and not ({"hammer", "pull", "slide"} & set(n.get("tech", []))):
                        stripped_cases += 1
                    if {"hammer", "pull", "slide"} & set(n.get("tech", [])):
                        self.assertFalse(p_str is not None and "dead" in p_str.get("tech", []), (seed, share, p_str, n))
                        if is_line:
                            self.assertFalse(
                                prev_line is not None and "dead" in prev_line.get("tech", []),
                                (seed, share, prev_line, n),
                            )
                    last_str[n["string"]] = n
                    if is_line:
                        prev_line = n

                if seed <= 805000:
                    with tempfile.TemporaryDirectory() as d:
                        composer.export(p, Path(d) / "x")
                        root = ET.parse(Path(d) / "x.musicxml").getroot()
                    pid = "P2" if other == "g2" else "P1"
                    part = next(pt for pt in root.iter("part") if pt.get("id") == pid)
                    for note in part.iter("note"):
                        if note.find("notehead") is not None and note.findtext("notehead") == "x":
                            for tag in ("hammer-on", "pull-off", "slide"):
                                for el in note.iter(tag):
                                    self.assertNotEqual(el.get("type"), "start", (seed, share, tag))
        self.assertGreater(stripped_cases, 0)

    def test_musicxml_writes_muted_strokes_as_one_division(self):
        rec = studio_engine.set_option(studio_engine.new_recipe(801000), "mute_share", 1.0)
        p = composer.compose(rec)
        other = other_of(rec)
        pid = "P2" if other == "g2" else "P1"
        with tempfile.TemporaryDirectory() as d:
            composer.export(p, Path(d) / "x")
            root = ET.parse(Path(d) / "x.musicxml").getroot()
        part = next(pt for pt in root.iter("part") if pt.get("id") == pid)
        m3 = next(m for m in part.iter("measure") if m.get("number") == "3")
        m3_notes = list(m3.iter("note"))
        self.assertTrue(m3_notes)
        self.assertIsNotNone(m3_notes[0].find("notehead"))
        self.assertEqual(m3_notes[0].findtext("notehead"), "x")
        self.assertEqual(int(m3_notes[0].findtext("duration")), 1)
        rest_after = next((n for n in m3_notes[1:] if n.find("chord") is None), None)
        self.assertIsNotNone(rest_after)
        self.assertIsNotNone(rest_after.find("rest"))

        rec_03 = studio_engine.set_option(studio_engine.new_recipe(801000), "mute_share", 0.3)
        p_03 = composer.compose(rec_03)
        with tempfile.TemporaryDirectory() as d:
            composer.export(p_03, Path(d) / "x")
            root_03 = ET.parse(Path(d) / "x.musicxml").getroot()
        part_03 = next(pt for pt in root_03.iter("part") if pt.get("id") == pid)
        num = den = None
        for m in part_03.iter("measure"):
            t = m.find("attributes/time")
            if t is not None:
                num, den = int(t.findtext("beats")), int(t.findtext("beat-type"))
            total = sum(int(n.findtext("duration")) for n in m.iter("note") if n.find("chord") is None)
            self.assertEqual(total, num * 16 // den)
            for n in m.iter("note"):
                if n.find("notehead") is not None and n.findtext("notehead") == "x":
                    self.assertEqual(int(n.findtext("duration")), 1)

    def test_octave_slides_validated_against_all_preceding_notes_and_repeat_boundary(self):
        rec = studio_engine.set_option(studio_engine.new_recipe(4), "octave_share", 0.5)
        p = composer.compose(rec)
        other = other_of(rec)
        smax = rec["studio"]["options"]["slide_max"]
        last = {}
        for n in sorted(p["parts"][other], key=lambda x: (x["start"], x["string"], x["pitch"])):
            if n.get("dyad") and "slide" in n.get("tech", []):
                prev = last.get(n["string"])
                self.assertIsNotNone(prev, n)
                self.assertNotEqual(prev["fret"], n["fret"], (prev, n))
                self.assertNotIn("dead", prev.get("tech", []), (prev, n))
                self.assertLessEqual(abs(n["fret"] - prev["fret"]), smax, (prev, n))
            last[n["string"]] = n

        rec_rep = studio_engine.set_option(rec, "square_repeat", 2)
        p_rep = composer.compose(rec_rep)
        s0, e0 = span_of(p_rep, "A2")
        half = (e0 - s0) // 2
        self.assertEqual(section_notes(p_rep, other, s0, s0 + half), section_notes(p_rep, other, s0 + half, e0))
        slides_a2 = [
            n for n in p_rep["parts"][other]
            if s0 <= n["start"] < s0 + half and n.get("dyad") and n["string"] == 1 and "slide" in n.get("tech", [])
        ]
        self.assertTrue(slides_a2, "valid fret 3->5 slide on string 1 in A2 bar 2 must be kept with square_repeat=2")

    def test_legacy_dose_edit_preserves_saved_arrangement_with_octave_share(self):
        rec = studio_engine.new_recipe(801000)
        rec["studio"]["salts"]["comp@A"] = 1
        rec["studio"]["options"]["octave_piece"] = 0.0
        rec["studio"]["options"]["octave_sixteenths"] = 1.0
        rec["studio"].pop("comp_doses", None)
        rec = studio_engine.set_option(rec, "octave_share", 1.0)
        p0 = composer.compose(rec)
        rec2 = studio_engine.set_option(rec, "slide_max", 4)
        p1 = composer.compose(rec2)
        other = other_of(rec)
        self.assertEqual(
            section_notes(p0, other, *span_of(p0, "A")),
            section_notes(p1, other, *span_of(p1, "A")),
        )
        self.assertEqual(midi_sha256(p0), midi_sha256(p1))

    def test_rerolls_vary_under_fixed_octave_share_and_intro_bars(self):
        rec = studio_engine.set_option(studio_engine.new_recipe(801000), "octave_share", 1.0)
        rec = studio_engine.set_option(rec, "octave_piece", 0.0)
        other = other_of(rec)
        a_notes = []
        cur = rec
        a_notes.append(section_notes(composer.compose(cur), other, *span_of(composer.compose(cur), "A")))
        for _ in range(5):
            cur = studio_engine.reroll(cur, "comp", "A")
            p = composer.compose(cur)
            a_notes.append(section_notes(p, other, *span_of(p, "A")))
        self.assertNotEqual(a_notes[0], a_notes[1])
        self.assertGreater(len({tuple(x) for x in a_notes[1:]}), 1)

        rec_in = studio_engine.set_option(studio_engine.new_recipe(0), "intro_bars", 8)
        p_in0 = composer.compose(rec_in)
        p_in1 = composer.compose(studio_engine.reroll(rec_in, "intro", "intro"))
        other_0 = other_of(rec_in)
        self.assertNotEqual(
            section_notes(p_in0, other_0, *span_of(p_in0, "intro")),
            section_notes(p_in1, other_0, *span_of(p_in1, "intro")),
        )

    def test_comp_first_reroll_always_changes_forced_octaves_over_60_seeds(self):
        seeds = [s * 1000 for s in range(801, 821)] + list(range(40))
        for seed in seeds:
            for oct_piece in (0.0, 0.5, 1.0):
                rec0 = studio_engine.new_recipe(seed)
                rec0 = studio_engine.set_option(rec0, "octave_share", 1.0)
                rec0 = studio_engine.set_option(rec0, "octave_piece", oct_piece)
                other = other_of(rec0)
                p0 = composer.compose(rec0)
                rec1 = studio_engine.reroll(rec0, "comp", "A")
                p1 = composer.compose(rec1)
                self.assertNotEqual(
                    section_notes(p0, other, *span_of(p0, "A")),
                    section_notes(p1, other, *span_of(p1, "A")),
                    (seed, "A", oct_piece),
                )
            rec0 = studio_engine.new_recipe(seed)
            rec0 = studio_engine.set_option(rec0, "octave_share", 1.0)
            rec0 = studio_engine.set_option(rec0, "octave_piece", 0.0)
            other = other_of(rec0)
            p0 = composer.compose(rec0)
            if "A2" in [sn for sn, _, _ in section_spans(p0)]:
                rec1_a2 = studio_engine.reroll(rec0, "comp", "A2")
                p1_a2 = composer.compose(rec1_a2)
                self.assertNotEqual(
                    section_notes(p0, other, *span_of(p0, "A2")),
                    section_notes(p1_a2, other, *span_of(p1_a2, "A2")),
                    (seed, "A2"),
                )

    def test_noop_option_does_not_push_undo_or_log(self):
        with tempfile.TemporaryDirectory() as d:
            render = RecordingRender()
            store = studio_engine.StudioStore(d, render_fn=render)
            det0 = store.create_piece(801000)
            pid = det0["id"]
            pdir = store._piece_dir(pid)
            calls0 = render.calls

            for name, val in (
                ("intro_bars", "auto"),
                ("square_repeat", 1),
                ("mute_share", 0.0),
                ("octave_share", None),
                ("b_lead", "augmented"),
                ("trailing_rest", det0["options"]["trailing_rest"]),
                ("octave_piece", composer.V8["octave_piece"]),
                ("slide_max", composer.V8["slide_max"]),
                ("mute_share", -0.1),
            ):
                det = store.set_option(pid, name, val)
                self.assertFalse(det["can_undo"], (name, val))
                self.assertEqual(store._read_undo(pdir), [], (name, val))
                self.assertEqual(render.calls, calls0, (name, val))

            actions = [json.loads(line)["action"] for line in (pdir / "history.jsonl").read_text(encoding="utf-8").splitlines() if line]
            self.assertEqual(actions, ["new"])

            det1 = store.set_option(pid, "intro_bars", 8)
            self.assertTrue(det1["can_undo"])
            self.assertEqual(len(store._read_undo(pdir)), 1)
            actions = [json.loads(line)["action"] for line in (pdir / "history.jsonl").read_text(encoding="utf-8").splitlines() if line]
            self.assertEqual(actions, ["new", "option"])

            det2 = store.set_option(pid, "intro_bars", 8)
            self.assertTrue(det2["can_undo"])
            self.assertEqual(len(store._read_undo(pdir)), 1)
            actions = [json.loads(line)["action"] for line in (pdir / "history.jsonl").read_text(encoding="utf-8").splitlines() if line]
            self.assertEqual(actions, ["new", "option"])

            det_undo = store.undo(pid)
            self.assertEqual(det_undo["options"]["intro_bars"], "auto")
            self.assertFalse(det_undo["can_undo"])

        with tempfile.TemporaryDirectory() as d:
            srv = studio_server.create_server(d, host="127.0.0.1", port=0, render_fn=studio_render.fake_render)
            threading.Thread(target=srv.serve_forever, daemon=True).start()
            base = f"http://127.0.0.1:{srv.server_address[1]}"
            try:
                def post(path, body):
                    req = urllib.request.Request(
                        base + path,
                        data=json.dumps(body).encode("utf-8"),
                        method="POST",
                        headers={"Content-Type": "application/json"},
                    )
                    with urllib.request.urlopen(req, timeout=10) as r:
                        return r.status, json.loads(r.read())

                st, det = post("/api/new", {"seed": 801000})
                self.assertEqual(st, 200)
                pid = det["id"]
                st, out = post(f"/api/piece/{pid}/option", {"name": "intro_bars", "value": "auto"})
                self.assertEqual(st, 200)
                self.assertFalse(out["can_undo"])
                lines = [json.loads(line) for line in (Path(d) / pid / "history.jsonl").read_text(encoding="utf-8").splitlines() if line]
                self.assertEqual([x["action"] for x in lines], ["new"])
            finally:
                srv.shutdown()
                srv.server_close()

        with tempfile.TemporaryDirectory() as d:
            store = studio_engine.StudioStore(d, render_fn=studio_render.fake_render)
            pid = store.create_piece(801000)["id"]
            pdir = store._piece_dir(pid)
            rec = store._read_recipe(pdir)
            rec["studio"]["salts"]["comp@A"] = 1
            rec["studio"].pop("comp_doses", None)
            raw_before = json.dumps(rec, indent=1)
            (pdir / "recipe.json").write_text(raw_before, encoding="utf-8")

            det = store.set_option(pid, "slide_max", composer.V8["slide_max"])
            self.assertFalse(det["can_undo"])
            self.assertEqual((pdir / "recipe.json").read_text(encoding="utf-8"), raw_before)
            lines = [json.loads(line) for line in (pdir / "history.jsonl").read_text(encoding="utf-8").splitlines() if line]
            self.assertEqual([x["action"] for x in lines], ["new"])

    def test_intro_bars(self):
        for seed in [s * 1000 for s in range(801, 811)]:
            rec0 = studio_engine.new_recipe(seed)
            p0 = composer.compose(rec0)
            rec_auto = studio_engine.set_option(rec0, "intro_bars", "auto")
            self.assertEqual(midi_sha256(p0), midi_sha256(composer.compose(rec_auto)), seed)

            for count in (1, 2, 4, 8):
                rec_n = studio_engine.set_option(rec0, "intro_bars", count)
                p_n = composer.compose(rec_n)
                intro_chords = [c for c in p_n["chords"] if c["sec"] == "intro"]
                self.assertEqual(len(intro_chords), count, (seed, count))
                harm_a = rec0["harmony"]["A"]
                self.assertEqual(
                    [c["root_deg"] for c in intro_chords],
                    [harm_a[i % len(harm_a)] for i in range(count)],
                    (seed, count),
                )
            p8 = composer.compose(studio_engine.set_option(rec0, "intro_bars", 8))
            s0 = section_spans(p0)
            s8 = section_spans(p8)
            for (sn0, a0, b0), (sn8, a8, b8) in zip(s0[1:], s8[1:]):
                self.assertEqual(sn0, sn8)
                for g in ("g1", "g2"):
                    self.assertTrue(
                        boundary_equal(section_notes(p0, g, a0, b0), section_notes(p8, g, a8, b8), b0 - a0),
                        (seed, sn0, g),
                    )

    def test_square_repeat(self):
        seeds_20 = [s * 1000 for s in range(801, 821)]
        saw_a2 = False
        for seed in seeds_20:
            rec0 = studio_engine.new_recipe(seed)
            p0 = composer.compose(rec0)
            rec1 = studio_engine.set_option(rec0, "square_repeat", 1)
            self.assertEqual(midi_sha256(p0), midi_sha256(composer.compose(rec1)), seed)

            rec2 = studio_engine.set_option(rec0, "square_repeat", 2)
            p2 = composer.compose(rec2)
            sp0 = section_spans(p0)
            sp2 = section_spans(p2)
            self.assertEqual([x[0] for x in sp0], [x[0] for x in sp2], seed)

            grow = sum(e - s for sn, s, e in sp0 if sn in ("A", "A2"))
            self.assertEqual(p2["total16"], p0["total16"] + grow, seed)

            for sec in ("A", "A2"):
                if not any(sn == sec for sn, _, _ in sp0):
                    continue
                if sec == "A2":
                    saw_a2 = True
                bars0 = sum(1 for c in p0["chords"] if c["sec"] == sec)
                bars2 = sum(1 for c in p2["chords"] if c["sec"] == sec)
                self.assertEqual(bars2, bars0 * 2, (seed, sec))
                s0, e0 = span_of(p2, sec)
                half = (e0 - s0) // 2
                for g in ("g1", "g2"):
                    first_half = section_notes(p2, g, s0, s0 + half)
                    second_half = section_notes(p2, g, s0 + half, e0)
                    self.assertEqual(first_half, second_half, (seed, sec, g))
        self.assertTrue(saw_a2)

    def test_server_options_validation_and_ui_labels(self):
        with tempfile.TemporaryDirectory() as d:
            srv = studio_server.create_server(d, host="127.0.0.1", port=0, render_fn=studio_render.fake_render)
            threading.Thread(target=srv.serve_forever, daemon=True).start()
            base = f"http://127.0.0.1:{srv.server_address[1]}"
            try:
                with urllib.request.urlopen(base + "/", timeout=10) as resp:
                    html = resp.read().decode("utf-8")
                for label in (
                    "Глухие ноты (доля ударов ритм-гитары)",
                    "Глушит долю ударов ритм-гитары в секциях A, A2, «Кульминация» и B; применяется сразу.",
                    "Октавные аккорды (доля тактов)",
                    "Длина вступления (тактов)",
                    "Повтор квадрата в куплетах",
                    "Дополнительно",
                    "Вероятность октав при перегенерации ритма",
                    "Октавы шестнадцатыми (при перегенерации)",
                    "Глухие удары перед акцентами (только в октавных тактах, при перегенерации)",
                ):
                    self.assertIn(label, html)

                def post(path, body):
                    req = urllib.request.Request(
                        base + path,
                        data=json.dumps(body).encode("utf-8"),
                        method="POST",
                        headers={"Content-Type": "application/json"},
                    )
                    try:
                        with urllib.request.urlopen(req, timeout=10) as r:
                            return r.status, json.loads(r.read())
                    except urllib.error.HTTPError as exc:
                        return exc.code, json.loads(exc.read())

                st, det = post("/api/new", {"seed": 801000})
                self.assertEqual(st, 200)
                pid = det["id"]
                for name, val in (
                    ("mute_share", 0.35),
                    ("octave_share", 0.5),
                    ("octave_share", None),
                    ("intro_bars", 8),
                    ("intro_bars", "auto"),
                    ("square_repeat", 2),
                ):
                    st, out = post(f"/api/piece/{pid}/option", {"name": name, "value": val})
                    self.assertEqual(st, 200, (name, val, out))
                    self.assertEqual(out["options"][name], val)

                for name, bad_val in (
                    ("mute_share", True),
                    ("mute_share", "0.5"),
                    ("octave_share", True),
                    ("intro_bars", 3),
                    ("intro_bars", True),
                    ("intro_bars", "8"),
                    ("square_repeat", 3),
                    ("square_repeat", True),
                ):
                    st, out = post(f"/api/piece/{pid}/option", {"name": name, "value": bad_val})
                    self.assertEqual(st, 400, (name, bad_val, out))
            finally:
                srv.shutdown()
                srv.server_close()

    def test_composer_invariants_with_random_control_combinations(self):
        bad_clash = shared_clash = 0
        for s in range(801, 821):
            rng = random.Random(s + 9191)
            rec = studio_engine.new_recipe(s * 1000)
            rec = studio_engine.set_option(rec, "mute_share", rng.choice([0.0, 0.3, 0.6, 1.0]))
            rec = studio_engine.set_option(rec, "octave_share", rng.choice([None, 0.0, 0.5, 1.0]))
            rec = studio_engine.set_option(rec, "intro_bars", rng.choice(["auto", 1, 2, 4, 8]))
            rec = studio_engine.set_option(rec, "square_repeat", rng.choice([1, 2]))

            p = composer.compose(rec)

            for g, notes in p["parts"].items():
                for n in notes:
                    self.assertTrue(0 <= n["fret"] <= fretboard.MAX_FRET, (s, g, n))
                    self.assertEqual(fretboard.OPEN[n["string"]] + n["fret"], n["pitch"], (s, g, n))

            for g, notes in p["parts"].items():
                line = [
                    n for n in notes
                    if not n.get("chordtone") and not n.get("dyad") and not (n.get("fixed") and "preset_tech" not in n)
                ]
                for prev, n in zip(line, line[1:]):
                    if {"hammer", "pull", "slide"} & set(n["tech"]):
                        self.assertEqual(prev["string"], n["string"], (s, g, n))
                    if "hammer" in n["tech"]:
                        self.assertGreater(n["fret"], prev["fret"], (s, g, n))

            for g, notes in p["parts"].items():
                last = {}
                for n in sorted(notes, key=lambda x: (x["start"], x["string"], x["pitch"])):
                    if n.get("dyad") and "slide" in n["tech"]:
                        prev = last.get(n["string"])
                        self.assertTrue(
                            prev is not None and prev["fret"] != n["fret"] and "dead" not in prev.get("tech", []),
                            (s, g, prev, n),
                        )
                    last[n["string"]] = n

            for g, notes in p["parts"].items():
                for _, n in zip(notes, notes[1:]):
                    if "tap" in n.get("tech", []) and n.get("fixed"):
                        nxt = [
                            m for m in notes
                            if n["start"] < m["start"] <= n["start"] + 2
                            and m["string"] == n["string"]
                            and "pull" in m.get("tech", [])
                        ][:1]
                        for m in nxt:
                            self.assertGreater(n["fret"], m["fret"], (s, g, n))

            g2 = {}
            for n in p["parts"]["g2"]:
                g2.setdefault(n["start"], []).append(n["pitch"])
            for n in p["parts"]["g1"]:
                for q in g2.get(n["start"], []):
                    shared_clash += 1
                    bad_clash += abs(n["pitch"] - q) % 12 in (1, 11)

            a = [(n["start"], n["start"] + n["dur"], n["pitch"]) for n in p["parts"]["g1"]]
            b = [(n["start"], n["start"] + n["dur"], n["pitch"]) for n in p["parts"]["g2"]]
            for x, y in ((a, b), (b, a)):
                for s0, _, pa in x:
                    for s2, e2, q in y:
                        if s2 <= s0 < e2 and pa != q:
                            self.assertNotEqual(abs(pa - q) % 12, 1, (s, s0, pa, q))

            with tempfile.TemporaryDirectory() as d:
                composer.export(p, Path(d) / "x")
                root = ET.parse(Path(d) / "x.musicxml").getroot()
            for part in root.iter("part"):
                num = den = None
                for m in part.iter("measure"):
                    t = m.find("attributes/time")
                    if t is not None:
                        num, den = int(t.findtext("beats")), int(t.findtext("beat-type"))
                    total = sum(int(n.findtext("duration")) for n in m.iter("note") if n.find("chord") is None)
                    self.assertEqual(total, num * 16 // den, (s, part.get("id"), m.get("number")))

        self.assertLess(bad_clash / max(1, shared_clash), 0.03, (bad_clash, shared_clash))


KEY_TARGETS = (
    ("D", "aeolian"), ("E", "ionian"), ("G", "dorian"), ("F#", "locrian"),
    ("B", "phrygian"), ("C#", "mixolydian"), ("D", "lydian"),
)
KEY_SEEDS = [s * 1000 for s in range(801, 831)]
KEY_STATS = collections.Counter()

ORACLE_SCALES = {
    "ionian": (0, 2, 4, 5, 7, 9, 11), "dorian": (0, 2, 3, 5, 7, 9, 10), "phrygian": (0, 1, 3, 5, 7, 8, 10),
    "lydian": (0, 2, 4, 6, 7, 9, 11), "mixolydian": (0, 2, 4, 5, 7, 9, 10), "aeolian": (0, 2, 3, 5, 7, 8, 10),
    "locrian": (0, 1, 3, 5, 6, 8, 10),
}
ORACLE_PC = {"E": 4, "F#": 6, "A": 9, "B": 11, "D": 2, "G": 7, "C#": 1}


def oracle_pitch(p, old, new, shift):
    t_old = 48 + ORACLE_PC[old[0]]
    octave, r = divmod(p - t_old, 12)
    scale = ORACLE_SCALES[old[1]]
    idx = max(i for i, x in enumerate(scale) if x <= r)
    new_scale = ORACLE_SCALES[new[1]]
    gap = (new_scale[idx + 1] if idx < 6 else 12) - new_scale[idx]
    return t_old + shift + 12 * octave + new_scale[idx] + min(r - scale[idx], gap - 1)


def oracle_shift(old, new, ref):
    diff = (ORACLE_PC[new[0]] - ORACLE_PC[old[0]]) % 12
    if diff != 6:
        return diff if diff < 6 else diff - 12
    mean = sum(ref) / len(ref)
    return min((-6, 6), key=lambda s: (round(abs(sum(oracle_pitch(p, old, new, s) for p in ref) / len(ref) - mean), 6),
                                       abs(sum(oracle_pitch(p, old, new, s) for p in ref) / len(ref) - 63)))


def map_notes(pitches, old, new, ref=None):
    parts = {"g1": [{"pitch": p, "start": i, "dur": 1} for i, p in enumerate(pitches)], "g2": []}
    chords = [{"start": 0, "sec": "A"}]
    rep = composer.apply_key_to_parts(
        parts, chords, len(pitches), composer.Theory(composer.TONICS[old[0]], old[1]),
        composer.Theory(composer.TONICS[new[0]], new[1]), ref or pitches)
    return [n["pitch"] for n in parts["g1"]], rep


def sign(x):
    return (x > 0) - (x < 0)


def keyed(seed, tonic, mode, **opts):
    rec = studio_engine.new_recipe(seed)
    for name, val in opts.items():
        rec = studio_engine.set_option(rec, name, val)
    return rec, studio_engine.set_option(rec, "key", {"tonic": tonic, "mode": mode})


def notes_at(piece, g, start):
    return [n for n in piece["parts"][g] if n["start"] == start]


class TestStudioKey(unittest.TestCase):
    @classmethod
    def tearDownClass(cls):
        if KEY_STATS["pieces"]:
            print(f"\nkey change: {dict(KEY_STATS)}", file=sys.stderr)

    def assert_playable(self, p, ctx):
        for g, notes in p["parts"].items():
            for n in notes:
                self.assertTrue(40 <= n["pitch"] <= 88, (ctx, g, n))
                self.assertTrue(0 <= n["fret"] <= fretboard.MAX_FRET, (ctx, g, n))
                self.assertEqual(fretboard.OPEN[n["string"]] + n["fret"], n["pitch"], (ctx, g, n))
            seen = {}
            for n in notes:
                self.assertNotIn((n["start"], n["string"]), seen, (ctx, g, n))
                seen[(n["start"], n["string"])] = n
            chords = {}
            for n in notes:
                if "dead" not in n["tech"]:
                    chords.setdefault(n["start"], []).append(n)
            for start, group in chords.items():
                if len(group) < 2:
                    continue
                fretted = [n["fret"] for n in group if n["fret"]]
                self.assertLessEqual(max(fretted) - min(fretted) if fretted else 0, fretboard.SPAN, (ctx, g, start))
                pitches = sorted(n["pitch"] for n in group)
                self.assertEqual(len(set(pitches)), len(pitches), (ctx, g, start, pitches))
                self.assertFalse(any((b - a) % 12 == 1 for a, b in zip(pitches, pitches[1:])), (ctx, g, start, pitches))
                self.assertFalse(fretboard.low_cluster(pitches), (ctx, g, start, pitches))
            line = [n for n in notes if not n.get("chordtone") and not n.get("dyad")
                    and not (n.get("fixed") and "preset_tech" not in n)]
            for prev, n in zip(line, line[1:]):
                if {"hammer", "pull", "slide"} & set(n["tech"]):
                    self.assertEqual(prev["string"], n["string"], (ctx, g, n))
                if {"hammer", "pull"} & set(n["tech"]):
                    self.assertGreaterEqual(prev["start"] + prev["dur"], n["start"] - 0.01, (ctx, g, prev, n))
                if "hammer" in n["tech"]:
                    self.assertGreater(n["fret"], prev["fret"], (ctx, g, n))
                if "tap" in n["tech"] and not n.get("fixed"):
                    self.assertTrue(n["fret"] > 0, (ctx, g, n))
            for prev, n in zip(notes, notes[1:]):
                if "tap" in prev["tech"] and n["string"] == prev["string"] and n["fret"] > 0 and not n.get("fixed"):
                    self.assertGreater(prev["fret"], n["fret"], (ctx, g, prev, n))
            last = {}
            for n in sorted((x for x in notes if x.get("dyad")), key=lambda x: x["start"]):
                if "slide" in n["tech"]:
                    q = last.get(n["string"])
                    self.assertTrue(q is not None and q["fret"] != n["fret"], (ctx, g, n))
                last[n["string"]] = n
            for n in notes:
                if "tap" in n.get("tech", []) and n.get("fixed"):
                    nxt = [m for m in notes if n["start"] < m["start"] <= n["start"] + 2
                           and m["string"] == n["string"] and "pull" in m.get("tech", [])][:1]
                    for m in nxt:
                        self.assertGreater(n["fret"], m["fret"], (ctx, g, n))
        with tempfile.TemporaryDirectory() as d:
            composer.export(p, Path(d) / "x")
            root = ET.parse(Path(d) / "x.musicxml").getroot()
        for part in root.iter("part"):
            num = den = None
            for m in part.iter("measure"):
                t = m.find("attributes/time")
                if t is not None:
                    num, den = int(t.findtext("beats")), int(t.findtext("beat-type"))
                total = sum(int(n.findtext("duration")) for n in m.iter("note") if n.find("chord") is None)
                self.assertEqual(total, num * 16 // den, (ctx, part.get("id"), m.get("number")))

    def assert_no_minor_ninth(self, p, ctx):
        for x, y in (("g1", "g2"), ("g2", "g1")):
            for n in p["parts"][x]:
                if "dead" in n["tech"]:
                    continue
                for m in p["parts"][y]:
                    if "dead" not in m["tech"] and m["start"] <= n["start"] < m["start"] + m["dur"]:
                        self.assertFalse(composer.minor_ninth(n["pitch"], m["pitch"]), (ctx, n, m))

    def test_unset_key_is_byte_identical_and_own_key_is_unset(self):
        rng = random.Random(20261123)
        rand_seeds = rng.sample(range(1000, 999999), 20)
        for seed in [s * 1000 for s in (801, 802, 805, 807, 809, 810)] + rand_seeds:
            rec = studio_engine.new_recipe(seed)
            self.assertIsNone(rec["studio"]["options"]["key"])
            plain = {k: v for k, v in rec.items() if k != "studio"}
            base = midi_sha256(composer.compose(plain))
            self.assertEqual(midi_sha256(composer.compose(rec)), base, seed)
            legacy = copy.deepcopy(rec)
            del legacy["studio"]["options"]["key"]
            self.assertEqual(midi_sha256(composer.compose(legacy)), base, seed)
            own = {"tonic": rec["intent"]["tonic"], "mode": rec["intent"]["mode"]}
            self.assertEqual(studio_engine.set_option(rec, "key", own), rec, seed)
            there = studio_engine.set_option(rec, "key", {"tonic": "D", "mode": "locrian"})
            self.assertEqual(midi_sha256(composer.compose(studio_engine.set_option(there, "key", None))), base, seed)

    def test_hand_checked_degree_mapping(self):
        cases = (
            ((("E", "ionian"), ("E", "phrygian")), (64, 68, 71, 66, 65), (64, 67, 71, 65, 64)),
            ((("A", "aeolian"), ("D", "aeolian")), (69, 72), (74, 77)),
            ((("E", "ionian"), ("E", "locrian")), (71,), (70,)),
            ((("D", "locrian"), ("D", "ionian")), (56,), (57,)),
            ((("A", "phrygian"), ("A", "aeolian")), (58,), (59,)),
            ((("G", "mixolydian"), ("C#", "ionian")), (77,), (72,)),
            ((("G", "mixolydian"), ("C#", "phrygian")), (59,), (64,)),
        )
        for (old, new), src, expected in cases:
            for p, e in zip(src, expected):
                got, _ = map_notes([p], old, new)
                self.assertEqual(got, [e], (old, new, p))
        merged = map_notes([65, 66], ("E", "ionian"), ("E", "phrygian"))
        self.assertEqual(merged, ([64, 65], {"runs_moved": 0, "folded": 0, "merged": 1, "revoiced": 0,
                                  "chord_moved": 0, "chord_dropped": 0, "chord_collapsed": 0}))
        wide = map_notes([55, 60, 66, 64, 71, 85], ("E", "ionian"), ("B", "locrian"))[0]
        self.assertTrue(all(40 <= p <= 86 for p in wide), wide)

    def test_every_mode_pair_matches_the_independent_oracle(self):
        for pitches in (list(range(48, 85, 3)), list(range(55, 70, 2))):
            for old_tonic in ("E", "A", "D"):
                for old_mode in composer.KEY_MODES:
                    for new_tonic in composer.TONICS:
                        for new_mode in composer.KEY_MODES:
                            old, new = (old_tonic, old_mode), (new_tonic, new_mode)
                            got, rep = map_notes(pitches, old, new)
                            shift = oracle_shift(old, new, pitches)
                            want = [oracle_pitch(p, old, new, shift) for p in pitches]
                            high = max(want) > 86
                            order = (-1, 1, -2, 2) if high else (1, -1, 2, -2)
                            if min(want) >= 40 and not high:
                                expected = want
                            else:
                                k = next((k for k in order if all(40 <= p + 12 * k <= 86 for p in want)), None)
                                expected = want if k is None else [p + 12 * k for p in want]
                            if rep["folded"] == 0:
                                self.assertEqual(got, expected, (old, new))
                            else:
                                self.assertTrue(all((a - b) % 12 == 0 for a, b in zip(got, want)), (old, new))

    def test_round_trip_restores_pitches_and_fails_when_mapping_is_a_no_op(self):
        checked = 0
        for old in (("E", "ionian"), ("G", "mixolydian"), ("A", "aeolian"), ("D", "dorian")):
            scale = ORACLE_SCALES[old[1]]
            src = [p for p in range(50, 80) if (p - 48 - ORACLE_PC[old[0]]) % 12 in scale][::2]
            for new in (("D", "aeolian"), ("C#", "ionian"), ("E", "phrygian"), ("B", "locrian"), ("G", "lydian")):
                if new == old:
                    continue
                there, _ = map_notes(src, old, new)
                self.assertNotEqual(there, src, (old, new))
                back, _ = map_notes(there, new, old)
                self.assertEqual(back, src, (old, new))
                checked += 1
        self.assertEqual(map_notes([67], ("G", "ionian"), ("C#", "ionian"))[0], [61])
        self.assertEqual(map_notes([61], ("C#", "ionian"), ("G", "ionian"))[0], [67])
        self.assertGreater(checked, 15)

    def test_notes_move_to_the_same_scale_degree_and_nothing_else_changes(self):
        for seed in KEY_SEEDS:
            rec = studio_engine.new_recipe(seed)
            trace = {}
            src = composer.compose(rec, _trace=trace)
            pre = trace["pre"]
            ref = [n["pitch"] for g in ("g1", "g2") for n in pre[g]]
            old = (rec["intent"]["tonic"], rec["intent"]["mode"])
            for tonic, mode in KEY_TARGETS:
                if (tonic, mode) == old:
                    continue
                ktrace = {}
                krec = studio_engine.set_option(rec, "key", {"tonic": tonic, "mode": mode})
                dst = composer.compose(krec, _trace=ktrace)
                ctx = (seed, tonic, mode)
                rep = dst["report"]["key"]
                resolved = set(dst["report"]["m9_resolved"])
                shift = oracle_shift(old, (tonic, mode), ref)
                KEY_STATS["pieces"] += 1
                for name in ("runs_moved", "folded", "merged", "revoiced", "chord_moved", "chord_dropped", "chord_collapsed"):
                    KEY_STATS[name] += rep[name]
                KEY_STATS["pieces_with_moved_run"] += rep["runs_moved"] > 0
                KEY_STATS["pieces_with_folds"] += rep["folded"] > 0
                self.assertEqual(dst["total16"], src["total16"], ctx)
                self.assertEqual(dst["chords"], src["chords"], ctx)
                moved_notes = 0
                for g in ("g1", "g2"):
                    a_notes, b_notes = pre[g], ktrace["pre"][g]
                    if len(a_notes) != len(b_notes):
                        self.assertGreater(rep["chord_dropped"] + rep["chord_collapsed"], 0, ctx)
                        key = lambda n: (n["start"], n["dur"], n["vel"])
                        self.assertFalse(collections.Counter(map(key, b_notes)) - collections.Counter(map(key, a_notes)), ctx)
                        continue
                    deltas = []
                    off_degree = 0
                    for a, b in zip(a_notes, b_notes):
                        self.assertEqual((a["start"], a["dur"], a["vel"]), (b["start"], b["dur"], b["vel"]), (ctx, g, a))
                        delta = b["pitch"] - oracle_pitch(a["pitch"], old, (tonic, mode), shift)
                        off_degree += delta % 12 != 0
                        deltas.append(delta)
                    self.assertLessEqual(off_degree, rep["chord_moved"], (ctx, g))
                    moved_notes += sum(x != 0 for x in deltas)  # includes chord tones moved by tidy
                    for i in range(len(a_notes) - 1):
                        if deltas[i] == deltas[i + 1] and a_notes[i]["start"] < a_notes[i + 1]["start"]:
                            self.assertEqual(sign(b_notes[i + 1]["pitch"] - b_notes[i]["pitch"]),
                                             sign(a_notes[i + 1]["pitch"] - a_notes[i]["pitch"]), (ctx, g, i))
                adjusted = rep["runs_moved"] + rep["folded"] + rep["revoiced"] + rep["chord_moved"] + rep["chord_dropped"]
                self.assertEqual(adjusted > 0, moved_notes > 0, ctx)
                chord_budget = rep["chord_dropped"] + rep["chord_collapsed"]
                for g in ("g1", "g2"):
                    pool = collections.Counter((n["start"], n["vel"]) for n in dst["parts"][g])
                    for n in src["parts"][g]:
                        KEY_STATS["notes"] += 1
                        if pool[(n["start"], n["vel"])] > 0:
                            pool[(n["start"], n["vel"])] -= 1
                        else:
                            if n["start"] not in resolved:
                                chord_budget -= 1
                                self.assertGreaterEqual(chord_budget, 0, (ctx, g, n))
                            self.assertNotIn("dead", n["tech"], (ctx, g, n))
                            KEY_STATS["dropped"] += 1
                self.assertEqual(
                    sum("dead" in n["tech"] for g in dst["parts"] for n in dst["parts"][g]),
                    sum("dead" in n["tech"] for g in src["parts"] for n in src["parts"][g]), ctx)
                same = [sorted(a["tech"]) == sorted(b["tech"]) for g in ("g1", "g2")
                        if len(src["parts"][g]) == len(dst["parts"][g])
                        for a, b in zip(src["parts"][g], dst["parts"][g])]
                KEY_STATS["tech_differs"] += same.count(False)
                KEY_STATS["tech_compared"] += len(same)
                self.assert_playable(dst, ctx)
                self.assert_no_minor_ninth(dst, ctx)

    def chord(self, pitches, old, new):
        parts = {"g1": [{"pitch": p, "start": 0, "dur": 4, "vel": 80, "fixed": (1, max(0, p - 45)), "ring": True}
                        for p in pitches], "g2": []}
        rep = composer.apply_key_to_parts(
            parts, [{"start": 0, "sec": "A"}], 16, composer.Theory(composer.TONICS[old[0]], old[1]),
            composer.Theory(composer.TONICS[new[0]], new[1]), pitches)
        return parts["g1"], rep

    def test_regression_chord_tones_are_moved_dropped_or_collapsed(self):
        notes, rep = self.chord([57, 59, 64, 71], ("G", "mixolydian"), ("D", "aeolian"))
        ps = sorted(n["pitch"] for n in notes)
        self.assertEqual(len(set(ps)), len(ps))
        self.assertFalse(any(abs(a - b) % 12 == 1 for a in ps for b in ps), ps)
        self.assertGreater(rep["chord_moved"] + rep["chord_dropped"], 0)
        self.assertEqual(ps[0], 52)
        fretted = [n["fixed"][1] for n in notes if n["fixed"][1]]
        self.assertLessEqual(max(fretted) - min(fretted), fretboard.SPAN)
        notes, rep = self.chord([64, 65], ("E", "ionian"), ("E", "phrygian"))
        self.assertEqual([n["pitch"] for n in notes], [64])
        self.assertEqual(rep["chord_collapsed"], 1)
        notes, rep = self.chord([40, 44, 47, 52], ("E", "ionian"), ("E", "ionian"))
        self.assertEqual(len(notes), 4)
        cluster, rep = self.chord([55, 57, 60], ("A", "aeolian"), ("A", "dorian"))
        self.assertFalse(fretboard.low_cluster([n["pitch"] for n in cluster]))

    def test_regression_chords_keep_a_playable_span_and_stay_consonant(self):
        for seed, tonic, mode, g, start in ((986307, "C#", "ionian", "g2", 128), (801000, "D", "aeolian", "g1", 323)):
            _, krec = keyed(seed, tonic, mode)
            p = composer.compose(krec)
            self.assert_playable(p, (seed, tonic, mode))
            group = notes_at(p, g, start)
            self.assertGreaterEqual(len(group), 2)
            fretted = [n["fret"] for n in group if n["fret"]]
            self.assertLessEqual(max(fretted) - min(fretted) if fretted else 0, fretboard.SPAN)
        rec = studio_engine.new_recipe(986307)
        p = composer.compose(studio_engine.set_option(rec, "key", {"tonic": "D", "mode": "aeolian"}))
        self.assert_playable(p, 986307)

    def test_runs_move_whole_and_square_copies_decide_alike(self):
        for seed, tonic, mode in ((801000, "F#", "locrian"), (801000, "D", "aeolian"), (802000, "B", "phrygian")):
            rec, krec = keyed(seed, tonic, mode, square_repeat=2)
            p = composer.compose(krec)
            spans = section_spans(p)
            a_spans = [(s, e) for sec, s, e in spans if sec == "A"]
            half = (a_spans[0][1] - a_spans[0][0]) // 2
            for g in ("g1", "g2"):
                first = section_notes(p, g, a_spans[0][0], a_spans[0][0] + half)
                second = section_notes(p, g, a_spans[0][0] + half, a_spans[0][1])
                self.assertEqual(first, second, (seed, g))

    def test_regression_octave_fold_keeps_descent_and_octave_chord(self):
        _, krec = keyed(801000, "D", "aeolian")
        p = composer.compose(krec)
        a, b = notes_at(p, "g1", 5), notes_at(p, "g1", 6)
        self.assertTrue(a and b)
        self.assertGreater(a[0]["pitch"], b[0]["pitch"])
        rec = studio_engine.set_option(studio_engine.new_recipe(40), "octave_share", 1.0)
        p = composer.compose(studio_engine.set_option(rec, "key", {"tonic": "C#", "mode": "mixolydian"}))
        dyads = [n for g in p["parts"] for n in p["parts"][g] if n["start"] == 42 and n.get("dyad")]
        self.assertEqual(len(dyads), 2)
        self.assertEqual(abs(dyads[0]["pitch"] - dyads[1]["pitch"]), 12)
        self.assertNotEqual((dyads[0]["string"], dyads[0]["fret"]), (dyads[1]["string"], dyads[1]["fret"]))

    def test_regression_fingering_never_stacks_or_overlaps_a_string(self):
        for seed, tonic, mode in ((801000, "F#", "locrian"), (807000, "G", "dorian"), (70, "E", "ionian"),
                                  (834000, "G", "dorian"), (23, "D", "aeolian")):
            _, krec = keyed(seed, tonic, mode)
            p = composer.compose(krec)
            self.assert_playable(p, (seed, tonic, mode))
            for g in ("g1", "g2"):
                by_string = {}
                for n in sorted(p["parts"][g], key=lambda n: n["start"]):
                    prev = by_string.get(n["string"])
                    if prev is not None and prev["start"] < n["start"]:
                        self.assertLessEqual(prev["start"] + prev["dur"], n["start"], (seed, g, prev, n))
                    by_string[n["string"]] = n

    def test_regression_locked_intro_keeps_its_register_when_another_section_is_rerolled(self):
        rec = studio_engine.set_lock(studio_engine.new_recipe(802000), "intro", "intro", True)
        rec = studio_engine.set_option(rec, "key", {"tonic": "B", "mode": "phrygian"})
        before = composer.compose(rec)
        after = composer.compose(studio_engine.reroll(rec, "b", "B"))
        end = section_spans(before)[0][2]
        for g in ("g1", "g2"):
            self.assertEqual(section_notes(before, g, 0, end), section_notes(after, g, 0, end), g)

    def test_regression_dead_strokes_survive_and_legacy_recipes_get_the_minor_ninth_rule(self):
        _, krec = keyed(801000, "D", "aeolian", mute_share=1.0)
        p = composer.compose(krec)
        self.assertTrue(any("dead" in n["tech"] for g in p["parts"] for n in notes_at(p, g, 285)))
        args = argparse.Namespace(meter=None, mode=None, key=None, tempo=None)
        legacy = studio_engine._ensure_studio(composer.make_recipe(801000, args))
        self.assertFalse(legacy.get("fix") or legacy.get("v6"))
        for tonic, mode in (("F#", "locrian"), ("D", "aeolian")):
            p = composer.compose(studio_engine.set_option(legacy, "key", {"tonic": tonic, "mode": mode}))
            self.assert_no_minor_ninth(p, (tonic, mode))

    def test_locking_the_ending_with_a_key_set_does_not_change_the_music(self):
        rec = studio_engine.set_option(studio_engine.new_recipe(801000), "key", {"tonic": "D", "mode": "aeolian"})
        locked = studio_engine.set_lock(rec, "ending", "end", True)
        self.assertEqual(midi_sha256(composer.compose(locked)), midi_sha256(composer.compose(rec)))

    def test_key_option_validation_labels_and_history(self):
        rec = studio_engine.new_recipe(801000)
        for bad in ("D", ["D", "aeolian"], {"tonic": "D"}, {"mode": "aeolian"}, {"tonic": "H", "mode": "aeolian"},
                    {"tonic": "D", "mode": "blues"}, {"tonic": "D", "mode": "aeolian", "x": 1}, True, 0,
                    {"tonic": 4, "mode": "aeolian"}):
            with self.assertRaises(ValueError, msg=bad):
                studio_engine.set_option(rec, "key", bad)
        r1 = studio_engine.set_option(rec, "key", {"tonic": "D", "mode": "aeolian"})
        self.assertEqual(r1["studio"]["options"]["key"], {"tonic": "D", "mode": "aeolian"})
        self.assertEqual(studio_engine.change_label(rec, r1), "тональность: D натуральный минор")
        self.assertEqual(studio_engine.change_label(r1, rec), "тональность: как в пьесе")
        r2 = studio_engine.set_option(r1, "key", {"tonic": "F#", "mode": "locrian"})
        self.assertEqual(studio_engine.change_label(r1, r2), "тональность: F# локрийский")
        self.assertEqual(studio_engine.set_option(r2, "key", {"tonic": "F#", "mode": "locrian"}), r2)

    def test_store_undo_redo_log_and_detail_show_the_key(self):
        with tempfile.TemporaryDirectory() as d:
            render = RecordingRender()
            store = studio_engine.StudioStore(d, render_fn=render)
            det0 = store.create_piece(801000)
            pid = det0["id"]
            pdir = store._piece_dir(pid)
            self.assertIsNone(det0["options"]["key"])
            calls0 = render.calls
            own = {"tonic": det0["key"], "mode": det0["mode"]}
            det = store.set_option(pid, "key", own)
            self.assertFalse(det["can_undo"])
            self.assertEqual(render.calls, calls0)
            det1 = store.set_option(pid, "key", {"tonic": "D", "mode": "aeolian"})
            self.assertEqual((det1["key"], det1["mode"]), ("D", "aeolian"))
            self.assertEqual(det1["options"]["key"], {"tonic": "D", "mode": "aeolian"})
            self.assertEqual(det1["undo_label"], "тональность: D натуральный минор")
            self.assertEqual(store.list_pieces()[0]["key"], "D")
            self.assertEqual(len(store._read_undo(pdir)), 1)
            rows = [json.loads(x) for x in (pdir / "history.jsonl").read_text(encoding="utf-8").splitlines() if x]
            self.assertEqual(rows[-1]["value"], {"key": {"tonic": "D", "mode": "aeolian"}})
            piece = composer.compose(json.loads((pdir / "recipe.json").read_text(encoding="utf-8")))
            self.assertEqual(piece["theory"].tonic_pc, composer.TONICS["D"])
            det2 = store.undo(pid)
            self.assertIsNone(det2["options"]["key"])
            self.assertEqual((det2["key"], det2["mode"]), (det0["key"], det0["mode"]))
            self.assertEqual(det2["redo_label"], "тональность: D натуральный минор")
            det3 = store.redo(pid)
            self.assertEqual(det3["options"]["key"], {"tonic": "D", "mode": "aeolian"})
            self.assertEqual(det3["undo_label"], "тональность: D натуральный минор")

    def test_key_applies_with_other_whole_piece_options(self):
        for seed in KEY_SEEDS[:8]:
            rng = random.Random(seed)
            tonic, mode = rng.choice(KEY_TARGETS)
            _, krec = keyed(seed, tonic, mode, mute_share=rng.choice([0.3, 1.0]),
                            octave_share=rng.choice([None, 1.0]), intro_bars=rng.choice([1, 4]),
                            square_repeat=rng.choice([1, 2]))
            p = composer.compose(krec)
            self.assert_playable(p, (seed, tonic, mode))
            self.assert_no_minor_ninth(p, (seed, tonic, mode))

    def test_server_accepts_and_rejects_key_values_and_page_has_the_controls(self):
        with tempfile.TemporaryDirectory() as d:
            srv = studio_server.create_server(d, host="127.0.0.1", port=0, render_fn=studio_render.fake_render)
            threading.Thread(target=srv.serve_forever, daemon=True).start()
            base = f"http://127.0.0.1:{srv.server_address[1]}"
            try:
                with urllib.request.urlopen(base + "/", timeout=10) as resp:
                    html = resp.read().decode("utf-8")
                for text in ("Тональность", "Как в пьесе",
                             "Переводит все ноты в выбранную тональность по ступеням; ритм и форма не меняются. Применяется сразу.",
                             "натуральный минор"):
                    self.assertIn(text, html)

                def post(path, body):
                    req = urllib.request.Request(base + path, data=json.dumps(body).encode("utf-8"), method="POST",
                                                 headers={"Content-Type": "application/json"})
                    try:
                        with urllib.request.urlopen(req, timeout=10) as r:
                            return r.status, json.loads(r.read())
                    except urllib.error.HTTPError as exc:
                        return exc.code, json.loads(exc.read())

                st, det = post("/api/new", {"seed": 801000})
                pid = det["id"]
                st, out = post(f"/api/piece/{pid}/option", {"name": "key", "value": {"tonic": "G", "mode": "dorian"}})
                self.assertEqual(st, 200, out)
                self.assertEqual((out["key"], out["mode"]), ("G", "dorian"))
                st, out = post(f"/api/piece/{pid}/option", {"name": "key", "value": None})
                self.assertEqual(st, 200, out)
                self.assertIsNone(out["options"]["key"])
                for bad in ("G", {"tonic": "G"}, {"tonic": "G", "mode": "blues"}, {"tonic": "H", "mode": "dorian"}):
                    st, out = post(f"/api/piece/{pid}/option", {"name": "key", "value": bad})
                    self.assertEqual(st, 400, (bad, out))
            finally:
                srv.shutdown()
                srv.server_close()


class TestStudioKeyPage(unittest.TestCase):
    def test_key_controls_send_options_and_reflect_the_current_key(self):
        out = _run_page("""
show({ key: "A", mode: "aeolian", options: { key: null, octave_share: null, mute_share: 0 } });
const btns = (o) => findAll((e) => e.dataset && e.dataset.option === o && e.tagName === "button");
const snap = () => ({
  tonics: btns("key_tonic").map((b) => [b.dataset.value, b.getAttribute("aria-pressed")]),
  modes: btns("key_mode").map((b) => [b.dataset.value, b.textContent, b.getAttribute("aria-pressed")]),
  reset: btns("key").map((b) => [b.textContent, b.getAttribute("aria-pressed")]),
});
const first = snap();
respond = async (url, body) => {
  if (!url.endsWith("/option")) return { pieces: [] };
  const v = body.value;
  return piece({ key: v ? v.tonic : "A", mode: v ? v.mode : "aeolian", options: { key: v, octave_share: null, mute_share: 0 } });
};
const click = async (o, v) => { find((e) => e.dataset && e.dataset.option === o && e.dataset.value === v && e.tagName === "button").onclick(); await sleep(20); };
await click("key_tonic", "D");
await click("key_mode", "dorian");
const second = snap();
await click("key", "auto");
console.log(JSON.stringify({ first, second, sent: calls.filter((c) => c.url.endsWith("/option")).map((c) => c.body) }));
""")
        self.assertEqual([t[0] for t in out["first"]["tonics"]], ["E", "F#", "G", "A", "B", "C#", "D"])
        self.assertEqual([t[1] for t in out["first"]["tonics"]], ["false", "false", "false", "true", "false", "false", "false"])
        self.assertEqual(
            [m[1] for m in out["first"]["modes"]],
            ["мажор", "дорийский", "фригийский", "лидийский", "миксолидийский", "натуральный минор", "локрийский"],
        )
        self.assertEqual([m[2] for m in out["first"]["modes"]].count("true"), 1)
        self.assertEqual(out["first"]["reset"], [["Как в пьесе", "true"]])
        self.assertEqual(out["second"]["reset"], [["Как в пьесе", "false"]])
        self.assertEqual(
            out["sent"],
            [{"name": "key", "value": {"tonic": "D", "mode": "aeolian"}},
             {"name": "key", "value": {"tonic": "D", "mode": "dorian"}},
             {"name": "key", "value": None}],
        )


    def test_two_quick_key_clicks_end_in_the_combined_key(self):
        out = _run_page("""
show({ key: "E", mode: "lydian", options: { key: null, octave_share: null, mute_share: 0 } });
let held = [];
respond = (url, body) => {
  if (!url.endsWith("/option")) return { pieces: [] };
  const v = body.value;
  return new Promise((resolve) => held.push(() => resolve(piece({ key: v.tonic, mode: v.mode, options: { key: v, octave_share: null, mute_share: 0 } }))));
};
const click = (o, v) => find((e) => e.dataset && e.dataset.option === o && e.dataset.value === v && e.tagName === "button").onclick();
click("key_tonic", "D");
await sleep(20);
click("key_mode", "aeolian");
await sleep(20);
while (held.length) { held.shift()(); await sleep(20); }
console.log(JSON.stringify({ sent: calls.filter((c) => c.url.endsWith("/option")).map((c) => c.body.value) }));
""")
        self.assertEqual(out["sent"], [{"tonic": "D", "mode": "lydian"}, {"tonic": "D", "mode": "aeolian"}])


    def test_reset_while_a_key_request_is_pending_does_not_reuse_the_old_key(self):
        out = _run_page("""
show({ key: "E", mode: "lydian", own_key: { tonic: "E", mode: "lydian" }, options: { key: null, octave_share: null, mute_share: 0 } });
let held = [];
respond = (url, body) => {
  if (!url.endsWith("/option")) return { pieces: [] };
  const v = body.value || { tonic: "E", mode: "lydian" };
  return new Promise((resolve) => held.push(() => resolve(piece({ key: v.tonic, mode: v.mode, own_key: { tonic: "E", mode: "lydian" }, options: { key: body.value, octave_share: null, mute_share: 0 } }))));
};
const click = (o, v) => find((e) => e.dataset && e.dataset.option === o && e.dataset.value === v && e.tagName === "button").onclick();
click("key_tonic", "D");
await sleep(20);
click("key", "auto");
await sleep(20);
click("key_mode", "dorian");
await sleep(20);
while (held.length) { held.shift()(); await sleep(20); }
console.log(JSON.stringify({ sent: calls.filter((c) => c.url.endsWith("/option")).map((c) => c.body.value) }));
""")
        self.assertEqual(out["sent"], [{"tonic": "D", "mode": "lydian"}, None, {"tonic": "E", "mode": "dorian"}])


def meter_oracle(piece, t):
    """Bar and sixteenth (both 1-based) walked from the meter list, independent of the chord list."""
    t16 = t * piece["tempo"] / 15.0 + 1e-6
    start, bar = 0, 1
    while True:
        num, den = None, None
        for tick, n, d in piece["meters"]:
            if tick <= start * 120:
                num, den = n, d
        length = num * 16 // den
        if t16 < start + length or start + length >= piece["total16"]:
            return bar, int(t16 - start) + 1
        start += length
        bar += 1


MARK_SEEDS = {"4/4": 801000, "7/8": 801006, "7/8>5/4>7/8": 801022}


class TestStudioMarks(unittest.TestCase):
    def _store(self, d):
        return studio_engine.StudioStore(d, render_fn=studio_render.fake_render)

    def _history(self, store, pid):
        path = store._piece_dir(pid) / "history.jsonl"
        return [json.loads(x) for x in path.read_text(encoding="utf-8").splitlines() if x]

    def test_bar_and_sixteenth_for_plain_odd_and_mixed_meters(self):
        expected_meters = {
            "4/4": [(4, 4)],
            "7/8": [(7, 8)],
            "7/8>5/4>7/8": [(7, 8), (5, 4), (7, 8)],
        }
        for name, seed in MARK_SEEDS.items():
            rec = studio_engine.new_recipe(seed)
            piece = composer.compose(rec)
            self.assertEqual([(n, d) for _, n, d in piece["meters"]], expected_meters[name])
            sec = 15.0 / piece["tempo"]
            times = [0.0, sec * 0.9, sec, sec * 13.5, piece["total16"] * sec - 0.001]
            for c in piece["chords"]:
                times += [c["start"] * sec, c["start"] * sec + sec * 5.5, (c["start"] - 0.01) * sec]
            for t in times:
                if t < 0:
                    continue
                with self.subTest(meters=name, t=round(t, 4)):
                    pos = studio_engine.bar_position(piece, t)
                    self.assertEqual((pos["bar"], pos["sixteenth"]), meter_oracle(piece, t))

    def test_intro_bars_are_counted_from_one(self):
        rec = studio_engine.new_recipe(801000)
        piece = composer.compose(rec)
        sec = 15.0 / piece["tempo"]
        intro = [c for c in piece["chords"] if c["sec"] == "intro"]
        self.assertTrue(intro)
        first_a = next(c for c in piece["chords"] if c["sec"] == "A")
        self.assertEqual(studio_engine.bar_position(piece, 0.0), {"bar": 1, "sixteenth": 1})
        self.assertEqual(
            studio_engine.bar_position(piece, first_a["start"] * sec), {"bar": len(intro) + 1, "sixteenth": 1}
        )
        self.assertEqual(
            studio_engine.bar_position(piece, first_a["start"] * sec - sec * 0.5),
            {"bar": len(intro), "sixteenth": 16},
        )

    def test_mark_records_position_section_guitar_text_notes_and_revision(self):
        rec = studio_engine.new_recipe(801022)
        piece = composer.compose(rec)
        t = 21.37
        mark = studio_engine.make_mark(rec, piece, t, "lead", "лишняя пауза")
        for key in ("id", "t", "bar", "sixteenth", "section", "section_label", "guitar", "text", "notes", "rev"):
            self.assertIn(key, mark)
        self.assertEqual((mark["bar"], mark["sixteenth"]), meter_oracle(piece, t))
        self.assertEqual(mark["t"], t)
        self.assertEqual((mark["guitar"], mark["text"]), ("lead", "лишняя пауза"))
        bar = next(c for c in reversed(piece["chords"]) if c["start"] <= t * piece["tempo"] / 15.0)
        self.assertEqual(mark["section"], bar["sec"])
        self.assertEqual(mark["section_label"], studio_engine.SECTION_LABELS[bar["sec"]])
        self.assertEqual(mark["rev"], studio_engine.recipe_rev(rec))
        plain = studio_engine.make_mark(rec, piece, t)
        self.assertEqual((plain["guitar"], plain["text"]), ("both", ""))

    def test_notes_snapshot_matches_the_composed_notes_within_one_sixteenth(self):
        rec = studio_engine.new_recipe(801000)
        piece = composer.compose(rec)
        sec = 15.0 / piece["tempo"]
        for t in (0.0, 3.2, 7.77, 12.0, piece["total16"] * sec - 0.05):
            t16 = t / sec
            expected = []
            for guitar, g in ((1, "g1"), (2, "g2")):
                for n in piece["parts"][g]:
                    if n["start"] <= t16 + 1 and n["start"] + n["dur"] > t16 - 1:
                        expected.append(
                            (guitar, n["start"], n["dur"], n["pitch"], n["string"], n["fret"], tuple(n.get("tech", [])))
                        )
            mark = studio_engine.make_mark(rec, piece, t)
            got = [
                (n["guitar"], n["start"], n["dur"], n["pitch"], n["string"], n["fret"], tuple(n["tech"]))
                for n in mark["notes"]
            ]
            with self.subTest(t=t):
                self.assertEqual(sorted(got), sorted(expected))
        busy = studio_engine.make_mark(rec, piece, 3.2)
        self.assertTrue(busy["notes"])
        self.assertTrue({n["guitar"] for n in busy["notes"]} <= {1, 2})

    def test_mark_fields_are_validated(self):
        rec = studio_engine.new_recipe(801000)
        piece = composer.compose(rec)
        length = piece["total16"] * 15.0 / piece["tempo"]
        make = studio_engine.make_mark
        make(rec, piece, 1.0, "both", "я" * 80)
        for args in (
            (1.0, "both", "я" * 81),
            (1.0, "bass", ""),
            (1.0, "both", 5),
            (-0.1, "both", ""),
            (length + 1, "both", ""),
            (float("nan"), "both", ""),
            (True, "both", ""),
            ("1", "both", ""),
        ):
            with self.subTest(args=args):
                with self.assertRaises(ValueError):
                    make(rec, piece, *args)

    def test_marks_never_change_music_undo_or_redo_and_are_logged(self):
        with tempfile.TemporaryDirectory() as d:
            store = self._store(d)
            pid = store.create_piece(801000)["id"]
            store.reroll(pid, "comp", "A")
            store.reroll(pid, "texture", "A")
            store.undo(pid)
            pdir = store._piece_dir(pid)
            names = ("piece.mid", "piece.mp3", "piece.musicxml", "piece.tab.txt", "recipe.json", "undo.json", "redo.json")
            frozen = {n: (pdir / n).read_bytes() for n in names}
            self.assertEqual(len(store._read_redo(pdir)), 1)

            det = store.add_mark(pid, 4.0, "rhythm", "кривая нота")
            mark = det["mark"]
            self.assertEqual([m["id"] for m in det["marks"]], [mark["id"]])
            det = store.edit_mark(pid, mark["id"], guitar="lead", text="лишняя пауза")
            self.assertEqual((det["mark"]["guitar"], det["mark"]["text"]), ("lead", "лишняя пауза"))
            edited = det["mark"]
            det = store.add_mark(pid, 9.0)
            second = det["mark"]
            det = store.delete_mark(pid, mark["id"])
            self.assertEqual([m["id"] for m in det["marks"]], [second["id"]])
            self.assertTrue(det["can_redo"])
            self.assertTrue(det["can_undo"])

            for name, raw in frozen.items():
                self.assertEqual((pdir / name).read_bytes(), raw, name)
            rows = self._history(store, pid)
            self.assertEqual([r["action"] for r in rows[:-4]], ["new", "reroll", "reroll", "undo"])
            self.assertEqual([r["action"] for r in rows[-4:]], ["mark_add", "mark_edit", "mark_add", "mark_delete"])
            self.assertEqual(rows[-4]["value"], mark)
            self.assertEqual(rows[-3]["value"], edited)
            self.assertEqual(rows[-2]["value"], second)
            self.assertEqual(rows[-1]["value"], edited)
            det = store.redo(pid)
            self.assertEqual([m["id"] for m in det["marks"]], [second["id"]])

    def test_noop_edit_writes_and_logs_nothing(self):
        with tempfile.TemporaryDirectory() as d:
            store = self._store(d)
            pid = store.create_piece(801000)["id"]
            mark = store.add_mark(pid, 4.0, "lead", "x")["mark"]
            before = len(self._history(store, pid))
            store.edit_mark(pid, mark["id"], guitar="lead", text="x")
            store.edit_mark(pid, mark["id"])
            self.assertEqual(len(self._history(store, pid)), before)

    def test_unknown_mark_is_refused_and_bad_edits_keep_the_mark(self):
        with tempfile.TemporaryDirectory() as d:
            store = self._store(d)
            pid = store.create_piece(801000)["id"]
            mark = store.add_mark(pid, 4.0)["mark"]
            with self.assertRaises(KeyError):
                store.edit_mark(pid, "nope", text="x")
            with self.assertRaises(KeyError):
                store.delete_mark(pid, "nope")
            with self.assertRaises(ValueError):
                store.edit_mark(pid, mark["id"], text="я" * 81)
            with self.assertRaises(ValueError):
                store.edit_mark(pid, mark["id"], guitar="bass")
            self.assertEqual(store.get_piece(pid)["marks"][0]["text"], "")

    def test_marks_persist_in_marks_json_and_legacy_pieces_load(self):
        with tempfile.TemporaryDirectory() as d:
            store = self._store(d)
            pid = store.create_piece(801000)["id"]
            pdir = store._piece_dir(pid)
            self.assertFalse((pdir / "marks.json").exists())
            self.assertEqual(store.get_piece(pid)["marks"], [])
            mark = store.add_mark(pid, 4.0, "both", "текст")["mark"]
            self.assertTrue((pdir / "marks.json").is_file())
            self.assertEqual(json.loads((pdir / "marks.json").read_text(encoding="utf-8")), [mark])
            again = self._store(d).get_piece(pid)
            self.assertEqual([m["id"] for m in again["marks"]], [mark["id"]])
            self.assertEqual(again["marks"][0]["text"], "текст")
            self.assertIn(pid, {p["id"] for p in self._store(d).list_pieces()})

    def test_mark_made_before_a_reroll_is_stale_after_it(self):
        with tempfile.TemporaryDirectory() as d:
            store = self._store(d)
            pid = store.create_piece(801000)["id"]
            mark = store.add_mark(pid, 4.0)["mark"]
            self.assertFalse(store.get_piece(pid)["marks"][0]["stale"])
            det = store.reroll(pid, "comp", "A")
            self.assertTrue(det["marks"][0]["stale"])
            self.assertNotEqual(det["rev"], mark["rev"])
            fresh = store.add_mark(pid, 5.0)["mark"]
            marks = {m["id"]: m for m in store.get_piece(pid)["marks"]}
            self.assertTrue(marks[mark["id"]]["stale"])
            self.assertFalse(marks[fresh["id"]]["stale"])
            det = store.undo(pid)
            marks = {m["id"]: m for m in det["marks"]}
            self.assertFalse(marks[mark["id"]]["stale"])
            self.assertTrue(marks[fresh["id"]]["stale"])

    def test_detail_gives_the_audition_range_one_bar_each_side(self):
        rec = studio_engine.new_recipe(801022)
        piece = composer.compose(rec)
        sec = 15.0 / piece["tempo"]
        starts = [c["start"] for c in piece["chords"]] + [piece["total16"]]
        for t in (0.5, 21.4, piece["total16"] * sec - 0.2):
            mark = studio_engine.make_mark(rec, piece, t)
            view = studio_engine.marks_view(rec, piece, [mark])[0]
            i = max(k for k, s in enumerate(starts[:-1]) if s <= t / sec + 1e-6)
            bar = (starts[i + 1] - starts[i]) * sec
            self.assertAlmostEqual(view["from_sec"], max(0.0, t - bar), places=2)
            self.assertAlmostEqual(view["to_sec"], min(piece["total16"] * sec, t + bar), places=2)

    def test_marks_json_name_cannot_leave_the_piece_folder(self):
        with tempfile.TemporaryDirectory() as d:
            store = self._store(d)
            pdir = store._piece_dir(store.create_piece(801000)["id"])
            self.assertEqual(store._inside(pdir, "marks.json"), pdir / "marks.json")
            for name in ("../marks.json", "sub/../../marks.json"):
                with self.assertRaises(KeyError):
                    store._inside(pdir, name)

    def test_marks_json_link_outside_the_piece_is_refused(self):
        with tempfile.TemporaryDirectory() as d, tempfile.TemporaryDirectory() as outside:
            store = self._store(d)
            pid = store.create_piece(801000)["id"]
            pdir = store._piece_dir(pid)
            foreign = Path(outside) / "marks.json"
            foreign.write_text("[]", encoding="utf-8")
            try:
                (pdir / "marks.json").symlink_to(foreign)
            except (OSError, NotImplementedError):
                self.skipTest("cannot create file symlinks here")
            for call in (
                lambda: store.get_piece(pid),
                lambda: store.add_mark(pid, 4.0),
                lambda: store.marks_export(pid),
            ):
                with self.assertRaises(KeyError):
                    call()
            self.assertEqual(foreign.read_text(encoding="utf-8"), "[]")

    def test_export_has_seed_recipe_and_marks_for_one_or_all_pieces(self):
        with tempfile.TemporaryDirectory() as d:
            store = self._store(d)
            a = store.create_piece(801000)["id"]
            b = store.create_piece(801006)["id"]
            mark = store.add_mark(a, 4.0, "lead", "т")["mark"]
            one = store.marks_export(a)
            self.assertEqual(one["id"], a)
            self.assertEqual(one["seed"], 801000)
            self.assertEqual(
                one["recipe"], json.loads((store._piece_dir(a) / "recipe.json").read_text(encoding="utf-8"))
            )
            self.assertEqual([m["id"] for m in one["marks"]], [mark["id"]])
            everything = store.all_marks_export()
            self.assertEqual({p["id"] for p in everything["pieces"]}, {a, b})
            self.assertEqual({p["id"]: len(p["marks"]) for p in everything["pieces"]}, {a: 1, b: 0})

    def test_summary_counts_marks_per_piece(self):
        with tempfile.TemporaryDirectory() as d:
            store = self._store(d)
            a = store.create_piece(801000)["id"]
            b = store.create_piece(801006)["id"]
            m1 = store.add_mark(a, 4.0)["mark"]
            store.add_mark(a, 6.0)
            store.edit_mark(a, m1["id"], text="x")
            store.add_mark(b, 3.0)
            store.delete_mark(b, store.get_piece(b)["marks"][0]["id"])
            stats = studio_summary.summarize(d)
            self.assertEqual(stats["marks_per_piece"], {a: 2, b: 0})
            self.assertIn(f"{a}: 2", studio_summary.format_summary(stats))
            self.assertEqual(studio_summary.summarize(Path(d) / "missing")["marks_per_piece"], {})

    def test_server_endpoints_for_marks(self):
        with tempfile.TemporaryDirectory() as d:
            srv = studio_server.create_server(d, host="127.0.0.1", port=0, render_fn=studio_render.fake_render)
            port = srv.server_address[1]
            t = threading.Thread(target=srv.serve_forever, daemon=True)
            t.start()
            base = f"http://127.0.0.1:{port}"

            def call(path, body=None):
                data = None if body is None else json.dumps(body).encode("utf-8")
                req = urllib.request.Request(
                    base + path, data=data, method="GET" if body is None else "POST",
                    headers={"Content-Type": "application/json"},
                )
                try:
                    with urllib.request.urlopen(req, timeout=10) as resp:
                        return resp.status, dict(resp.headers), resp.read()
                except urllib.error.HTTPError as err:
                    return err.code, dict(err.headers), err.read()

            try:
                _, _, raw = call("/api/new", {"seed": 801000})
                pid = json.loads(raw)["id"]
                st, _, raw = call(f"/api/piece/{pid}/mark_add", {"t": 4.0, "guitar": "lead", "text": "a"})
                self.assertEqual(st, 200)
                det = json.loads(raw)
                mid = det["mark"]["id"]
                self.assertEqual([m["id"] for m in det["marks"]], [mid])
                st, _, raw = call(f"/api/piece/{pid}/mark_edit", {"id": mid, "text": "b"})
                self.assertEqual((st, json.loads(raw)["mark"]["text"]), (200, "b"))
                st, hdrs, raw = call(f"/api/piece/{pid}/marks.json")
                self.assertEqual(st, 200)
                self.assertIn(f"{pid}.marks.json", hdrs["Content-Disposition"])
                self.assertEqual(json.loads(raw)["marks"][0]["text"], "b")
                st, _, raw = call("/api/marks.json")
                self.assertEqual([p["id"] for p in json.loads(raw)["pieces"]], [pid])
                self.assertEqual(call(f"/api/piece/{pid}/mark_add", {"t": "x"})[0], 400)
                self.assertEqual(call(f"/api/piece/{pid}/mark_add", {})[0], 400)
                self.assertEqual(call(f"/api/piece/{pid}/mark_add", {"t": 4.0, "text": "я" * 81})[0], 400)
                self.assertEqual(call(f"/api/piece/{pid}/mark_edit", {"id": "nope"})[0], 404)
                self.assertEqual(call(f"/api/piece/{pid}/mark_edit", {})[0], 400)
                self.assertEqual(call(f"/api/piece/{pid}/mark_delete", {"id": "nope"})[0], 404)
                self.assertEqual(call("/api/piece/missing/mark_add", {"t": 1})[0], 404)
                self.assertEqual(call("/api/piece/missing/marks.json")[0], 404)
                st, _, raw = call(f"/api/piece/{pid}/mark_delete", {"id": mid})
                self.assertEqual((st, json.loads(raw)["marks"]), (200, []))
            finally:
                srv.shutdown()
                srv.server_close()
                t.join(timeout=5)

    def test_web_api_and_store_agree_on_the_same_marks(self):
        from studio import web_api

        def strip(marks):
            return [{k: v for k, v in m.items() if k != "id"} for m in marks]

        with tempfile.TemporaryDirectory() as d:
            store = self._store(d)
            pid = store.create_piece(801022)["id"]
            store.reroll(pid, "comp", "A")
            st = web_api.new_piece(801022)
            st = web_api.reroll(st, "comp", "A")
            for t, guitar, text in ((2.5, "both", ""), (21.4, "lead", "пауза"), (23.9, "rhythm", "нота")):
                store_det = store.add_mark(pid, t, guitar, text)
                web_res = web_api.mark_add(st, t, guitar, text)
                st = web_res["state"]
                self.assertEqual(strip([store_det["mark"]]), strip([web_res["mark"]]))
            store.edit_mark(pid, store.get_piece(pid)["marks"][1]["id"], text="иначе")
            st = web_api.mark_edit(st, st["marks"][1]["id"], text="иначе")["state"]
            store.delete_mark(pid, store.get_piece(pid)["marks"][0]["id"])
            st = web_api.mark_delete(st, st["marks"][0]["id"])["state"]
            self.assertEqual(strip(store.get_piece(pid)["marks"]), strip(web_api.detail(st)["marks"]))
            self.assertEqual(strip(store.marks_export(pid)["marks"]), strip(web_api.export_marks(st)["marks"]))
            self.assertEqual(store.get_piece(pid)["rev"], web_api.detail(st)["rev"])


MARKS_PAGE_FIXTURE = """
const SEC = [{ section: "A", label: "Куплет A", start_sec: 0, end_sec: 10, layers: [] }];
const mk = (id, t, extra) => Object.assign({ id, t, bar: 2, sixteenth: 5, section: "A", section_label: "Куплет A", guitar: "both", text: "", stale: false, from_sec: Math.max(0, t - 2), to_sec: Math.min(10, t + 2), notes: [], rev: "r" }, extra || {});
const withMarks = (marks, extra) => piece(Object.assign({ length: 10, sections: SEC, marks }, extra || {}));
const press = (ev) => (docListeners.keydown || []).forEach((fn) => fn(Object.assign({ preventDefault() {}, target: { tagName: "BODY" } }, ev)));
const cls = (e, name) => String(e.className || "").split(" ").includes(name);
const marksCalls = () => calls.filter((c) => /\\/mark_(add|edit|delete)$/.test(c.url));
const getPlayer = () => {
  const p = ctx.document.getElementById("player");
  p.played = 0; p.paused = 0;
  p.play = () => { p.played++; return Promise.resolve(); };
  p.pause = () => { p.paused++; };
  return p;
};
"""


class TestStudioMarksPage(unittest.TestCase):
    def test_markup_has_the_buttons_hint_and_styles(self):
        html = studio_server.INDEX_HTML.read_text(encoding="utf-8")
        self.assertIn("Тут не так", html)
        self.assertIn("Скачать отметки", html)
        self.assertIn("Скачать отметки всех пьес", html)
        self.assertIn("Послушать место", html)
        self.assertIn("(до изменения)", html)
        self.assertIn(
            "Отметьте место, где звучит не по-человечески: кнопка «Тут не так», клавиша M "
            "или Shift+клик по таймлайну. Музыку это не меняет.",
            html,
        )
        self.assertRegex(html, r"\.mark-flag\.stale\s*\{")

    def test_m_adds_a_mark_at_the_playback_position_but_not_while_typing(self):
        out = _run_page(MARKS_PAGE_FIXTURE + """
show(withMarks([]));
const player = getPlayer();
player.currentTime = 3.456;
respond = async (url, body) => url.endsWith("/mark_add") ? Object.assign(withMarks([mk("m1", 3.456)]), { mark: mk("m1", 3.456) }) : { pieces: [] };
press({ key: "m", code: "KeyM" });
await sleep(30);
const sent = marksCalls().map((c) => [c.url, c.body]);
for (const ev of [{ target: { tagName: "INPUT" } }, { target: { tagName: "TEXTAREA" } }, { ctrlKey: true }, { metaKey: true }, { altKey: true }]) {
  press(Object.assign({ key: "m", code: "KeyM" }, ev));
}
press({ key: "n", code: "KeyN" });
await sleep(30);
console.log(JSON.stringify({
  sent, total: marksCalls().length,
  title: ctx.document.getElementById("marksTitle").textContent,
  flags: findAll((e) => cls(e, "mark-flag")).length,
  editor: Boolean(find((e) => e.id === "markEditor")),
  played: player.played,
}));
""")
        self.assertEqual(out["sent"], [["/api/piece/p1/mark_add", {"t": 3.456}]])
        self.assertEqual(out["total"], 1)
        self.assertEqual(out["title"], "Отметки (1)")
        self.assertEqual(out["flags"], 1)
        self.assertTrue(out["editor"])
        self.assertEqual(out["played"], 0)

    def test_button_adds_a_mark_and_no_piece_means_no_request(self):
        out = _run_page(MARKS_PAGE_FIXTURE + """
press({ key: "m", code: "KeyM" });
await sleep(20);
const none = marksCalls().length;
show(withMarks([]));
const player = getPlayer();
player.currentTime = 7;
respond = async (url) => url.endsWith("/mark_add") ? Object.assign(withMarks([mk("m1", 7)]), { mark: mk("m1", 7) }) : { pieces: [] };
ctx.document.getElementById("markBtn").onclick();
await sleep(30);
console.log(JSON.stringify({ none, sent: marksCalls().map((c) => c.body) }));
""")
        self.assertEqual(out["none"], 0)
        self.assertEqual(out["sent"], [{"t": 7}])

    def test_shift_click_adds_a_mark_and_a_plain_click_only_seeks(self):
        out = _run_page(MARKS_PAGE_FIXTURE + """
show(withMarks([]));
const player = getPlayer();
respond = async (url) => url.endsWith("/mark_add") ? Object.assign(withMarks([mk("m1", 2.5)]), { mark: mk("m1", 2.5) }) : { pieces: [] };
const blk = find((e) => cls(e, "sec-block"));
blk.getBoundingClientRect = () => ({ left: 100, width: 200 });
blk.onclick({ shiftKey: true, clientX: 150 });
await sleep(30);
const afterShift = marksCalls().map((c) => c.body);
player.currentTime = 9;
blk.onclick({ shiftKey: false, clientX: 150 });
await sleep(20);
console.log(JSON.stringify({ afterShift, after: marksCalls().length, time: player.currentTime, played: player.played }));
""")
        self.assertEqual(out["afterShift"], [{"t": 2.5}])
        self.assertEqual(out["after"], 1)
        self.assertEqual(out["time"], 0)
        self.assertEqual(out["played"], 1)

    def test_editor_enter_done_and_esc(self):
        out = _run_page(MARKS_PAGE_FIXTURE + """
show(withMarks([]));
const player = getPlayer();
player.currentTime = 4;
let marks = [];
respond = async (url, body) => {
  if (url.endsWith("/mark_add")) { marks = marks.concat([mk("m" + (marks.length + 1), body.t)]); return Object.assign(withMarks(marks), { mark: marks[marks.length - 1] }); }
  if (url.endsWith("/mark_edit")) { marks = marks.map((m) => m.id === body.id ? Object.assign({}, m, { guitar: body.guitar || m.guitar, text: body.text === undefined ? m.text : body.text }) : m); return Object.assign(withMarks(marks), { mark: marks.find((m) => m.id === body.id) }); }
  if (url.endsWith("/mark_delete")) { marks = marks.filter((m) => m.id !== body.id); return withMarks(marks); }
  return { pieces: [] };
};
const editor = () => find((e) => e.id === "markEditor");
const out = {};
press({ key: "m", code: "KeyM" });
await sleep(30);
out.opened = Boolean(editor());
const guitars = findAll((e) => e.dataset && e.dataset.markGuitar).map((b) => [b.dataset.markGuitar, b.getAttribute("aria-pressed")]);
out.guitars = guitars;
findAll((e) => e.dataset && e.dataset.markGuitar === "lead")[0].onclick();
const input = find((e) => e.id === "markText");
out.maxlength = input.getAttribute("maxlength");
input.value = "лишняя пауза";
find((e) => e.id === "markDone").onclick();
await sleep(30);
out.afterDone = { editor: Boolean(editor()), marks: marks.map((m) => [m.guitar, m.text]) };
press({ key: "Escape" });
await sleep(30);
out.escAfterDone = marks.length;
press({ key: "m", code: "KeyM" });
await sleep(30);
press({ key: "Escape", target: find((e) => e.id === "markText") });
await sleep(30);
out.escNew = { marks: marks.length, editor: Boolean(editor()) };
press({ key: "m", code: "KeyM" });
await sleep(30);
find((e) => e.id === "markText").onkeydown({ key: "Enter" });
await sleep(30);
out.enter = { marks: marks.length, editor: Boolean(editor()) };
out.urls = marksCalls().map((c) => c.url.split("/").pop());
console.log(JSON.stringify(out));
""")
        self.assertTrue(out["opened"])
        self.assertEqual(out["guitars"], [["lead", "false"], ["rhythm", "false"], ["both", "true"]])
        self.assertEqual(out["maxlength"], "80")
        self.assertEqual(out["afterDone"], {"editor": False, "marks": [["lead", "лишняя пауза"]]})
        self.assertEqual(out["escAfterDone"], 1)
        self.assertEqual(out["escNew"], {"marks": 1, "editor": False})
        self.assertEqual(out["enter"], {"marks": 2, "editor": False})
        self.assertEqual(
            out["urls"], ["mark_add", "mark_edit", "mark_add", "mark_delete", "mark_add"]
        )

    def test_list_flags_stale_marks_and_seeking(self):
        out = _run_page(MARKS_PAGE_FIXTURE + """
show(withMarks([
  mk("m1", 3, { bar: 2, sixteenth: 5, guitar: "lead", text: "лишняя пауза" }),
  mk("m2", 5, { bar: 3, sixteenth: 1, guitar: "rhythm", text: "старая", stale: true }),
]));
const player = getPlayer();
const rows = findAll((e) => cls(e, "mark-row"));
const flags = findAll((e) => cls(e, "mark-flag"));
const text = (e) => walk(e, () => true).map((x) => x.textContent).join(" ");
const out = {
  title: ctx.document.getElementById("marksTitle").textContent,
  hint: ctx.document.getElementById("marksHint").textContent,
  rows: rows.map((r) => r.children.map((c) => c.textContent).join(" ")),
  flagLeft: flags.map((f) => f.style.left),
  flagStale: flags.map((f) => cls(f, "stale")),
  rowStale: rows.map((r) => cls(r, "stale")),
  flagTitle: flags[1].title,
};
flags[0].onclick();
out.afterFlag = { time: player.currentTime, editor: Boolean(find((e) => e.id === "markEditor")) };
find((e) => e.id === "markDone").onclick();
await sleep(20);
player.currentTime = 0;
findAll((e) => cls(e, "mark-open"))[1].onclick();
out.afterRow = { time: player.currentTime, editor: Boolean(find((e) => e.id === "markEditor")) };
out.requests = marksCalls().length;
console.log(JSON.stringify(out));
""")
        self.assertEqual(out["title"], "Отметки (2)")
        self.assertEqual(
            out["hint"],
            "Отметьте место, где звучит не по-человечески: кнопка «Тут не так», клавиша M "
            "или Shift+клик по таймлайну. Музыку это не меняет.",
        )
        self.assertIn("2:5", out["rows"][0])
        self.assertIn("лишняя пауза", out["rows"][0])
        self.assertIn("соло", out["rows"][0])
        self.assertNotIn("до изменения", out["rows"][0])
        self.assertIn("3:1", out["rows"][1])
        self.assertIn("(до изменения)", out["rows"][1])
        self.assertEqual(out["flagLeft"], ["30%", "50%"])
        self.assertEqual(out["flagStale"], [False, True])
        self.assertEqual(out["rowStale"], [False, True])
        self.assertIn("до изменения", out["flagTitle"])
        self.assertEqual(out["afterFlag"], {"time": 1, "editor": True})
        self.assertEqual(out["afterRow"], {"time": 3, "editor": True})
        self.assertEqual(out["requests"], 0)

    def test_listen_plays_one_bar_each_side_and_stops(self):
        out = _run_page(MARKS_PAGE_FIXTURE + """
show(withMarks([mk("m1", 5)]));
const player = getPlayer();
const listen = findAll((e) => cls(e, "mark-audition"))[0];
const out = { label: listen.textContent };
listen.onclick();
out.start = { time: player.currentTime, played: player.played };
const tick = () => (player._ls.timeupdate || []).forEach((fn) => fn({}));
player.currentTime = 6.5;
tick();
out.mid = player.paused;
player.currentTime = 7.1;
tick();
out.end = player.paused;
player.currentTime = 1;
tick();
out.later = player.paused;
console.log(JSON.stringify(out));
""")
        self.assertEqual(out["label"], "Послушать место")
        self.assertEqual(out["start"], {"time": 3, "played": 1})
        self.assertEqual(out["mid"], 0)
        self.assertEqual(out["end"], 1)
        self.assertEqual(out["later"], 1)

    def test_download_links_point_at_marks_json(self):
        out = _run_page(MARKS_PAGE_FIXTURE + """
show(withMarks([]));
console.log(JSON.stringify({ one: ctx.document.getElementById("marksDlBtn").href }));
""")
        self.assertEqual(out["one"], "/api/piece/p1/marks.json")
        html = studio_server.INDEX_HTML.read_text(encoding="utf-8")
        self.assertRegex(
            html, r'<a class="btn" id="allMarksBtn" href="/api/marks\.json" download>Скачать отметки всех пьес</a>'
        )


if __name__ == "__main__":
    unittest.main()
