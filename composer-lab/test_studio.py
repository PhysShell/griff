import argparse
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
                for n in sorted((x for x in notes if x.get("dyad")), key=lambda x: x["start"]):
                    if "slide" in n["tech"]:
                        prev = last.get(n["string"])
                        self.assertTrue(prev is not None and prev["fret"] != n["fret"], (s, g, n))
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
    get innerHTML() { return ""; }, set onclick(v) {}, value: "", currentTime: 0,
    play() { return Promise.resolve(); } };
}
const ctx = {
  document: { getElementById: () => el(), createElement: () => el() },
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


class TestStudioControls(unittest.TestCase):
    def _v8_plain(self, seed):
        args = argparse.Namespace(meter=None, mode=None, key=None, tempo=None, v8=True)
        return composer.make_recipe(seed, args)

    def test_defaults_byte_identical(self):
        rng = random.Random(20261123)
        seeds = [s * 1000 for s in (801, 802, 805, 807, 809, 810)] + rng.sample(range(1000, 999999), 20)
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
                if b["sec"] in ("A", "A2") and b.get("role") and not b.get("tex"):
                    self.assertEqual(b["role"], "octave", (seed, i, b))
                    s0 = p1["chords"][i]["start"]
                    s1 = p1["chords"][i + 1]["start"] if i + 1 < len(p1["chords"]) else p1["total16"]
                    dyads = [n for n in p1["parts"][other] if s0 <= n["start"] < s1 and n.get("dyad")]
                    self.assertTrue(dyads, (seed, i))
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
                    "Октавные аккорды (доля тактов)",
                    "Длина вступления (тактов)",
                    "Повтор квадрата в куплетах",
                    "Дополнительно",
                    "Вероятность октав при перегенерации ритма",
                    "Октавы шестнадцатыми (при перегенерации)",
                    "Глухие перед акцентами в октавах (при перегенерации)",
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
                for n in sorted((x for x in notes if x.get("dyad")), key=lambda x: x["start"]):
                    if "slide" in n["tech"]:
                        prev = last.get(n["string"])
                        self.assertTrue(prev is not None and prev["fret"] != n["fret"], (s, g, n))
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


if __name__ == "__main__":
    unittest.main()
