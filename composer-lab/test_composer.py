import hashlib
import json
import tempfile
import unittest
from pathlib import Path

import composer
import fretboard


def build(seed, **kw):
    args = composer.argparse.Namespace(meter=None, mode=None, key=None, tempo=None, **kw)
    return composer.compose(composer.make_recipe(seed, args))


class ComposerInvariants(unittest.TestCase):
    seeds = range(1, 41)

    def test_same_seed_same_midi_bytes(self):
        with tempfile.TemporaryDirectory() as d:
            digests = []
            for i in range(2):
                composer.export(build(7), Path(d) / f"x{i}")
                digests.append(hashlib.sha256((Path(d) / f"x{i}.mid").read_bytes()).hexdigest())
            self.assertEqual(digests[0], digests[1], "fixed seed must reproduce the MIDI bytes")

    def test_every_note_is_on_its_string_and_fret(self):
        for seed in self.seeds:
            for g, notes in build(seed)["parts"].items():
                for n in notes:
                    self.assertTrue(0 <= n["fret"] <= fretboard.MAX_FRET, (seed, g, n))
                    self.assertEqual(fretboard.OPEN[n["string"]] + n["fret"], n["pitch"], (seed, g, n))

    def test_legato_only_between_notes_on_the_same_string(self):
        for seed in self.seeds:
            for g, notes in build(seed)["parts"].items():
                line = [n for n in notes if not n.get("chordtone") and not n.get("dyad")
                        and not (n.get("fixed") and "preset_tech" not in n)]
                for prev, n in zip(line, line[1:]):
                    if {"hammer", "pull", "slide"} & set(n["tech"]):
                        self.assertEqual(prev["string"], n["string"], (seed, g, n))
                    if "hammer" in n["tech"]:
                        self.assertGreater(n["fret"], prev["fret"], (seed, g, n))

    def test_octave_shape_slides_from_the_same_string(self):
        for seed in self.seeds:
            for g, notes in build(seed)["parts"].items():
                last = {}
                for n in sorted((x for x in notes if x.get("dyad")), key=lambda x: x["start"]):
                    if "slide" in n["tech"]:
                        p = last.get(n["string"])
                        self.assertTrue(p is not None and p["fret"] != n["fret"], (seed, g, n))
                    last[n["string"]] = n

    def test_tapped_notes_are_above_the_fretting_hand(self):
        for seed in self.seeds:
            for g, notes in build(seed)["parts"].items():
                for prev, n in zip(notes, notes[1:]):
                    if "tap" in n.get("tech", []) and n.get("fixed"):
                        nxt = [m for m in notes if n["start"] < m["start"] <= n["start"] + 2
                               and m["string"] == n["string"] and "pull" in m.get("tech", [])][:1]
                        for m in nxt:
                            self.assertGreater(n["fret"], m["fret"], (seed, g, n))

    def test_no_semitone_clash_on_shared_accented_onsets(self):
        bad = shared = 0
        for seed in self.seeds:
            p = build(seed)
            g2 = {}
            for n in p["parts"]["g2"]:
                g2.setdefault(n["start"], []).append(n["pitch"])
            for n in p["parts"]["g1"]:
                for q in g2.get(n["start"], []):
                    shared += 1
                    bad += abs(n["pitch"] - q) % 12 in (1, 11)
        self.assertLess(bad / shared, 0.03, f"semitone clashes {bad}/{shared} between guitars must stay rare")


class Harmony(unittest.TestCase):
    def test_no_minor_ninth_against_the_chord_root_in_chords_and_arpeggios(self):
        for seed in range(1, 41):
            p = build(seed)
            th = p["theory"]
            starts = [c["start"] for c in p["chords"]] + [p["total16"]]
            for i, c in enumerate(p["chords"]):
                root_pc = th.midi(c["root_deg"]) % 12
                for g in ("g1", "g2"):
                    for n in p["parts"][g]:
                        if starts[i] <= n["start"] < starts[i + 1] and (n.get("fixed") or n.get("ring")):
                            self.assertNotEqual((n["pitch"] - root_pc) % 12, 1, (seed, g, n))


class RhythmSkeleton(unittest.TestCase):
    def recipe(self, seed, skeleton):
        args = composer.argparse.Namespace(meter=None, mode=None, key=None, tempo=None, skeleton=skeleton)
        return composer.make_recipe(seed, args)

    def test_v1_without_flag_reproduces_the_frozen_test_set(self):
        frozen = Path(__file__).resolve().parents[2] / "_griff_out" / "test_2e421df" / "new"
        if not frozen.exists():
            self.skipTest("frozen test set not on this machine")
        for seed in (101, 105, 112):
            rec = json.loads((frozen / f"new_{seed:03d}.recipe.json").read_text(encoding="utf-8"))
            with tempfile.TemporaryDirectory() as d:
                composer.export(composer.compose(rec), Path(d) / "x")
                self.assertEqual((Path(d) / "x.mid").read_bytes(), (frozen / f"new_{seed:03d}.mid").read_bytes(),
                                 f"v1 output for seed {seed} must stay byte-identical")

    def test_v2_recipe_differs_from_v1_only_by_the_skeleton(self):
        for seed in range(201, 213):
            a, b = self.recipe(seed * 1000, False), self.recipe(seed * 1000, True)
            sk = b["motifs"].pop("skeleton")
            self.assertTrue(sk["A"] and sk["B"], seed)
            a.pop("generator"), b.pop("generator")
            self.assertEqual(a, b, f"seed {seed}: only the skeleton may differ")

    def test_both_guitars_attack_every_skeleton_hit_in_a_sections(self):
        missing = total = 0
        for seed in range(201, 213):
            rec = self.recipe(seed * 1000, True)
            p = composer.compose(rec)
            hits = rec["motifs"]["skeleton"]["A"]
            for c, bar in zip(p["chords"], rec["form"]):
                if bar["sec"] not in ("A", "A2", "A3") or bar["prog"] in ("call",):
                    continue
                for h in hits:
                    total += 1
                    for g in ("g1", "g2"):
                        if not any(abs(n["start"] - (c["start"] + h)) < 1e-6 for n in p["parts"][g]):
                            missing += 1
                            break
        self.assertLess(missing / total, 0.1, f"{missing}/{total} skeleton hits not shared by both guitars")


class ArrangedInvariants(ComposerInvariants):
    seeds = range(301, 341)

    def setUp(self):
        global build
        self._build = build
        build = lambda seed, **kw: self._build(seed, arrange=True, **kw)

    def tearDown(self):
        global build
        build = self._build


def busy_bars(p, form, lead):
    """Intro and A-type bars where the accompanying guitar attacks at least 3/4 of the sixteenths."""
    other = "g2" if lead == "g1" else "g1"
    busy = 0
    for i, c in enumerate(p["chords"]):
        if form[i]["sec"] not in ("intro", "A", "A2", "A3"):
            continue
        end = p["chords"][i + 1]["start"] if i + 1 < len(p["chords"]) else p["total16"]
        ons = {n["start"] for n in p["parts"][other] if c["start"] <= n["start"] < end}
        busy += len(ons) >= 0.75 * (end - c["start"])
    return busy, sum(b["sec"] in ("intro", "A", "A2", "A3") for b in form)


class Arrangement(unittest.TestCase):
    seeds = range(301, 341)

    def recipe(self, seed, arrange, tempo=None):
        args = composer.argparse.Namespace(meter=None, mode=None, key=None, tempo=tempo, arrange=arrange)
        return composer.make_recipe(seed, args)

    def test_v3_recipe_keeps_every_v1_field_except_form(self):
        for seed in self.seeds:
            a, b = self.recipe(seed * 1000, False), self.recipe(seed * 1000, True)
            self.assertIn("arrange", b)
            for k in ("seed", "intent", "harmony", "motifs"):
                self.assertEqual(a[k], b[k], (seed, k))

    def test_sixteenth_texture_is_dosed(self):
        busy = total = v1_busy = v1_total = 0
        for seed in self.seeds:
            for arr in (False, True):
                rec = self.recipe(seed * 1000, arr)
                b, t = busy_bars(composer.compose(rec), rec["form"], rec["intent"]["lead"])
                if arr:
                    busy, total = busy + b, total + t
                    self.assertLessEqual(b, t / 2, f"seed {seed}: sixteenth texture in {b}/{t} bars")
                else:
                    v1_busy, v1_total = v1_busy + b, v1_total + t
        self.assertLess(busy / total, 0.75 * v1_busy / v1_total, (busy, total, v1_busy, v1_total))

    def test_duration_stays_within_a_quarter_of_the_v1_twin(self):
        for seed in self.seeds:
            a, b = self.recipe(seed * 1000, False), self.recipe(seed * 1000, True)
            da = composer.form_seconds(a["form"], a["intent"], a["intent"]["tempo"])
            db = composer.form_seconds(b["form"], b["intent"], b["arrange"]["tempo"])
            self.assertTrue(0.79 <= db / da <= 1.26, (seed, da, db))

    def test_pieces_vary_in_tempo_form_and_density(self):
        recs = [self.recipe(seed * 1000, True) for seed in self.seeds]
        slower = 0
        for r in recs:
            self.assertLessEqual(r["arrange"]["tempo"], r["intent"]["tempo"], r["seed"])
            slower += r["arrange"]["tempo"] < 0.9 * r["intent"]["tempo"]
        self.assertGreaterEqual(slower, len(recs) // 4)
        self.assertEqual({r["arrange"]["template"] for r in recs}, {"classic", "double_b", "no_a2"})
        self.assertEqual({v for r in recs for v in r["arrange"]["b"]}, {"chug", "half", "clean"})
        self.assertGreater(len({len(r["form"]) for r in recs}), 2)
        for r in recs:
            self.assertGreater(len({b["dens"] for b in r["form"] if "dens" in b}), 1, r["seed"])

    def test_midi_tempo_is_the_arranged_tempo_and_respects_the_cli(self):
        rec = self.recipe(301000, True)
        self.assertEqual(composer.compose(rec)["tempo"], rec["arrange"]["tempo"])
        self.assertEqual(self.recipe(301000, True, tempo=99)["arrange"]["tempo"], 99)

    def test_eighth_grid_keeps_every_accent(self):
        for grouping in ([3, 4], [2, 2, 3], [3, 3, 4, 4], [3, 3]):
            acc = composer.accents_of(grouping)
            grid = composer.eighths(acc, sum(grouping))
            self.assertTrue(acc <= grid, grouping)
            self.assertTrue(all(b - a >= 1 for a, b in zip(sorted(grid), sorted(grid)[1:])))
            self.assertLessEqual(len(grid), sum(grouping) // 2 + len(acc), grouping)


class RepeatedInvariants(ComposerInvariants):
    seeds = range(401, 441)

    def setUp(self):
        global build
        self._build = build
        build = lambda seed, **kw: self._build(seed, repeat=True, **kw)

    def tearDown(self):
        global build
        build = self._build


def bar_spans(p):
    ch = p["chords"]
    return [(c["start"], ch[i + 1]["start"] if i + 1 < len(ch) else p["total16"]) for i, c in enumerate(ch)]


def lead_repeats(p, form, lead):
    """Share of A-type bars whose lead line repeats the previous bar verbatim."""
    bars = []
    for (s, e), b in zip(bar_spans(p), form):
        bars.append((b["sec"], tuple(sorted((n["start"] - s, n["pitch"]) for n in p["parts"][lead] if s <= n["start"] < e))))
    pairs = [(a, b) for a, b in zip(bars, bars[1:]) if a[0] == b[0] and a[0] in ("A", "A2", "A3")]
    return sum(a[1] == b[1] and a[1] != () for a, b in pairs) / len(pairs)


def both_busy(p, form):
    """A-type bars in which both guitars attack at least half of the sixteenths."""
    n = 0
    for (s, e), b in zip(bar_spans(p), form):
        if b["sec"] in ("A", "A2", "A3"):
            n += all(len({x["start"] for x in p["parts"][g] if s <= x["start"] < e}) >= (e - s) / 2 for g in ("g1", "g2"))
    return n


class Repetition(unittest.TestCase):
    seeds = range(401, 441)

    def recipe(self, seed, repeat):
        args = composer.argparse.Namespace(meter=None, mode=None, key=None, tempo=None, arrange=True, repeat=repeat)
        return composer.make_recipe(seed, args)

    def test_v4_recipe_differs_from_v3_only_in_phrase_programs_and_roles(self):
        for seed in self.seeds:
            a, b = self.recipe(seed * 1000, False), self.recipe(seed * 1000, True)
            self.assertIn("repeat", b)
            for k in ("seed", "intent", "harmony", "motifs", "arrange"):
                self.assertEqual(a[k], b[k], (seed, k))
            strip = lambda f: [{k: v for k, v in x.items() if k not in ("prog", "role")} for x in f]
            self.assertEqual(strip(a["form"]), strip(b["form"]), seed)

    def test_lead_riff_repeats_verbatim(self):
        v3 = v4 = 0
        for seed in self.seeds:
            for rep in (False, True):
                rec = self.recipe(seed * 1000, rep)
                r = lead_repeats(composer.compose(rec), rec["form"], rec["intent"]["lead"])
                if rep:
                    v4 += r
                    self.assertGreaterEqual(r, 0.3, seed)
                else:
                    v3 += r
        self.assertGreater(v4, 3 * v3 + 0.2 * len(self.seeds), (v3, v4))

    def test_guitars_rarely_chatter_together(self):
        v3 = v4 = 0
        for seed in self.seeds:
            for rep in (False, True):
                rec = self.recipe(seed * 1000, rep)
                n = both_busy(composer.compose(rec), rec["form"])
                if rep:
                    v4 += n
                else:
                    v3 += n
        self.assertLess(v4, v3, (v3, v4))


class TexturedInvariants(ComposerInvariants):
    seeds = range(501, 541)

    def setUp(self):
        global build
        self._build = build
        build = lambda seed, **kw: self._build(seed, texture=True, **kw)

    def tearDown(self):
        global build
        build = self._build


class V6Invariants(ComposerInvariants):
    seeds = range(601, 641)

    def setUp(self):
        global build
        self._build = build
        build = lambda seed, **kw: self._build(seed, v6=True, **kw)

    def tearDown(self):
        global build
        build = self._build


def sounding_intervals(p):
    a, b = [[(n["start"], n["start"] + n["dur"], n["pitch"]) for n in p["parts"][g]] for g in ("g1", "g2")]
    for x, y in ((a, b), (b, a)):
        for s, e, pa in x:
            yield [abs(pa - q) for s2, e2, q in y if s2 <= s < e2 and pa != q]


class V6(unittest.TestCase):
    seeds = range(601, 641)

    def recipe(self, seed, v6):
        args = composer.argparse.Namespace(meter=None, mode=None, key=None, tempo=None, texture=not v6, v6=v6)
        return composer.make_recipe(seed, args)

    def test_v6_recipe_differs_from_v5_only_in_its_plan_and_ending(self):
        for seed in self.seeds:
            a, b = self.recipe(seed * 1000, False), self.recipe(seed * 1000, True)
            self.assertIn("v6", b)
            for k in ("seed", "intent", "harmony", "motifs", "arrange", "repeat", "texture"):
                self.assertEqual(a[k], b[k], (seed, k))
            strip = lambda f: [{k: v for k, v in x.items() if k != "end"} for x in f]
            self.assertEqual(strip(a["form"]), strip(b["form"]), seed)

    def test_no_minor_ninths_but_major_sevenths_remain(self):
        m9 = m7 = 0
        for seed in self.seeds:
            for ds in sounding_intervals(composer.compose(self.recipe(seed * 1000, True))):
                m9 += any(d % 12 == 1 for d in ds)
                m7 += any(d % 12 == 11 for d in ds)
        self.assertEqual(m9, 0)
        self.assertGreater(m7, 100)

    def test_riff_stays_verbatim(self):
        for seed in self.seeds:
            a, b = self.recipe(seed * 1000, False), self.recipe(seed * 1000, True)
            lead = a["intent"]["lead"]
            pa, pb = composer.compose(a), composer.compose(b)
            for (s, e), bar in zip(zip([c["start"] for c in pa["chords"]], [c["start"] for c in pa["chords"][1:]] + [pa["total16"]]), a["form"]):
                if bar["prog"] == "riff" and not b["v6"]["rests"]:
                    la = [(n["start"], n["pitch"]) for n in pa["parts"][lead] if s <= n["start"] < e]
                    lb = [(n["start"], n["pitch"]) for n in pb["parts"][lead] if s <= n["start"] < e]
                    self.assertEqual(la, lb, (seed, s))

    def test_fast_moves_rarely_jump_or_skip(self):
        bad = fast = 0
        for seed in self.seeds:
            p = composer.compose(self.recipe(seed * 1000, True))
            sec = 15 / p["tempo"]
            for ns in p["parts"].values():
                line = sorted((x for x in ns if not x.get("fixed")), key=lambda x: x["start"])
                for a, b in zip(line, line[1:]):
                    if 0 < (b["start"] - a["start"]) * sec <= 0.12:
                        fast += 1
                        bad += bool(a["fret"] and b["fret"] and abs(b["fret"] - a["fret"]) >= 7) or \
                            abs(b["string"] - a["string"]) >= 3
        self.assertLess(bad / fast, 0.025, (bad, fast))

    def test_every_piece_has_a_rest_at_a_square_boundary(self):
        for seed in self.seeds:
            rec = self.recipe(seed * 1000, True)
            self.assertTrue(rec["v6"]["rests"], seed)


class V7Invariants(ComposerInvariants):
    seeds = range(701, 741)

    def setUp(self):
        global build
        self._build = build
        build = lambda seed, **kw: self._build(seed, v7=True, **kw)

    def tearDown(self):
        global build
        build = self._build


class V7(unittest.TestCase):
    seeds = range(701, 741)

    def piece(self, seed, **flags):
        args = composer.argparse.Namespace(meter=None, mode=None, key=None, tempo=None, **flags)
        rec = composer.make_recipe(seed * 1000, args)
        return rec, composer.compose(rec)

    def accented_triad_share(self, **flags):
        on = tot = 0
        for seed in self.seeds:
            rec, p = self.piece(seed, **flags)
            th, lead = p["theory"], rec["intent"]["lead"]
            accs = composer.accents_of(rec["intent"]["grouping"])
            for i, (ch, bar) in enumerate(zip(p["chords"], rec["form"])):
                if bar["sec"] not in ("A", "A2", "A3"):
                    continue
                end = p["chords"][i + 1]["start"] if i + 1 < len(p["chords"]) else p["total16"]
                triad = {th.midi(ch["root_deg"] + k) % 12 for k in (0, 2, 4)}
                for n in p["parts"][lead]:
                    if ch["start"] <= n["start"] < end and n["start"] - ch["start"] in accs:
                        tot += 1
                        on += n["pitch"] % 12 in triad
        return on / tot

    def test_lead_uses_colour_tones_on_accents(self):
        self.assertLess(self.accented_triad_share(v7=True), self.accented_triad_share(repeat=True) - 0.15)

    def test_no_minor_ninths_between_guitars(self):
        for seed in self.seeds:
            for ds in sounding_intervals(self.piece(seed, v7=True)[1]):
                self.assertFalse(any(d % 12 == 1 for d in ds), seed)

    def test_modes_lean_minor(self):
        modes = [self.piece(seed, v7=True)[0]["intent"]["mode"] for seed in self.seeds]
        self.assertGreaterEqual(sum(m in ("aeolian", "dorian") for m in modes), len(modes) // 2)

    def test_v7_has_no_v5_or_v6_plans(self):
        rec, _ = self.piece(701, v7=True)
        self.assertNotIn("texture", rec)
        self.assertNotIn("v6", rec)
        self.assertIn("repeat", rec)


class V8Invariants(ComposerInvariants):
    seeds = range(801, 841)

    def setUp(self):
        global build
        self._build = build
        build = lambda seed, **kw: self._build(seed, v8=True, **kw)

    def tearDown(self):
        global build
        build = self._build


class V8(unittest.TestCase):
    seeds = range(801, 841)

    def piece(self, seed, flag):
        args = composer.argparse.Namespace(meter=None, mode=None, key=None, tempo=None, **{flag: True})
        rec = composer.make_recipe(seed * 1000, args)
        return rec, composer.compose(rec)

    def test_octave_shapes_are_octaves_on_a_and_g_strings(self):
        sections = 0
        for seed in self.seeds:
            rec, p = self.piece(seed, "v8")
            sections += len(rec["v8"]["octave"])
            for g, notes in p["parts"].items():
                by = {}
                for n in notes:
                    if n.get("dyad"):
                        by.setdefault(n["start"], []).append(n)
                planned = [b for b in rec["form"] if b.get("role") == "octave"]
                self.assertEqual(bool(rec["v8"]["octave"]), bool(planned), seed)
                for pair in by.values():
                    self.assertEqual(sorted(x["string"] for x in pair), [1, 3], (seed, pair))
                    lo, hi = sorted(x["pitch"] for x in pair)
                    self.assertEqual(hi - lo, 12, (seed, pair))
        self.assertTrue(0.3 * 40 <= sections <= 0.7 * 40, sections)

    def test_dead_notes_and_slides_appear_but_stay_rare(self):
        dead = slide = dyads = 0
        for seed in self.seeds:
            _, p = self.piece(seed, "v8")
            for notes in p["parts"].values():
                for n in notes:
                    if n.get("dyad"):
                        dyads += 1
                        dead += "dead" in n["tech"]
                        slide += "slide" in n["tech"]
        self.assertGreater(slide, 0)
        self.assertTrue(0 < dead < 0.15 * dyads, (dead, dyads))

    def test_no_low_close_clusters_in_chords(self):
        for seed in self.seeds:
            _, p = self.piece(seed, "v8")
            for g, notes in p["parts"].items():
                by = {}
                for n in notes:
                    if n.get("fixed") and not n.get("dyad") and "preset_tech" not in n:
                        by.setdefault(n["start"], []).append(n["pitch"])
                for start, pitches in by.items():
                    self.assertFalse(fretboard.low_cluster(pitches), (seed, g, start, pitches))

    def test_arpeggio_cycle_skips(self):
        steps = lambda cyc: sum(abs(a - b) <= 1 for a, b in zip(cyc, cyc[1:] + cyc[:1]))
        worse = 0
        for seed in self.seeds:
            rec, _ = self.piece(seed, "v8")
            cyc = rec["motifs"]["arp"]["cycle"]
            worse += steps(composer.leap_cycle(cyc)) > steps(cyc)
        self.assertEqual(worse, 0)

    def test_no_minor_ninths_between_guitars(self):
        for seed in self.seeds:
            for ds in sounding_intervals(self.piece(seed, "v8")[1]):
                self.assertFalse(any(d % 12 == 1 for d in ds), seed)


class V9Invariants(ComposerInvariants):
    seeds = range(901, 941)

    def setUp(self):
        global build
        self._build = build
        build = lambda seed, **kw: self._build(seed, v9=True, **kw)

    def tearDown(self):
        global build
        build = self._build


class V9(unittest.TestCase):
    seeds = range(901, 941)

    def comping(self, flag):
        from collections import Counter
        pats = Counter()
        recs = []
        for seed in self.seeds:
            args = composer.argparse.Namespace(meter=None, mode=None, key=None, tempo=None, **{flag: True})
            rec = composer.make_recipe(seed * 1000, args)
            p = composer.compose(rec)
            recs.append((rec, p))
            other = "g2" if rec["intent"]["lead"] == "g1" else "g1"
            for i, ch in enumerate(p["chords"]):
                b = rec["form"][i]
                if b["sec"] in ("A", "A2") and (b.get("role") == "chords" or b["prog"] == "cad_open"):
                    end = p["chords"][i + 1]["start"] if i + 1 < len(p["chords"]) else p["total16"]
                    ons = tuple(sorted({n["start"] - ch["start"] for n in p["parts"][other]
                                        if ch["start"] <= n["start"] < end}))
                    pats[(rec["intent"]["meter"], ons)] += 1
        return pats, recs

    def test_comping_patterns_differ_between_pieces(self):
        v8, _ = self.comping("v8")
        v9, _ = self.comping("v9")
        self.assertGreaterEqual(len(v9), 3 * len(v8), (len(v8), len(v9)))

    def test_variations_land_mostly_on_bar_four(self):
        _, recs = self.comping("v9")
        where = [k.split(":")[1] for rec, _ in recs for k in rec["v9"]["events"]]
        self.assertGreater(where.count("3"), where.count("0") + where.count("1"), where)
        self.assertGreater(where.count("0") + where.count("1"), 0, where)

    def test_every_intro_hands_over(self):
        _, recs = self.comping("v9")
        for rec, _ in recs:
            intro = [b for b in rec["form"] if b["sec"] == "intro"]
            if rec["v9"]["intro"] == "short":
                self.assertLessEqual(len(intro), 1)
            else:
                self.assertEqual(intro[-1].get("handover"), rec["v9"]["intro"])

    def test_dead_strums_are_short_and_quiet(self):
        _, recs = self.comping("v9")
        dead = [n for _, p in recs for ns in p["parts"].values() for n in ns
                if "dead" in n.get("tech", []) and not n.get("dyad")]
        self.assertTrue(dead)
        self.assertTrue(all(n["dur"] == 1 and n["vel"] <= 50 for n in dead))


class NoTail(unittest.TestCase):
    """--notail removes the trailing rest of the riff bar and changes nothing else."""
    liked = (801, 802, 805, 807, 809, 810)

    def recipe(self, seed, notail):
        args = composer.argparse.Namespace(meter=None, mode=None, key=None, tempo=None, v8=True, notail=notail)
        return composer.make_recipe(seed * 1000, args)

    def test_only_the_hook_differs_from_v8(self):
        for seed in list(self.liked) + list(range(1001, 1041)):
            a, b = self.recipe(seed, False), self.recipe(seed, True)
            for r in (a, b):
                r["motifs"] = {k: v for k, v in r["motifs"].items() if k != "hook"}
                r["lead"] = {k: v for k, v in r["lead"].items() if k not in ("tie", "pickup")}
                r.pop("generator")
            self.assertEqual(a, b, seed)

    def test_pieces_without_a_trailing_rest_are_unchanged(self):
        for seed in self.liked:
            a, b = self.recipe(seed, False), self.recipe(seed, True)
            if a["motifs"]["hook"]["rhythm"] == b["motifs"]["hook"]["rhythm"]:
                with tempfile.TemporaryDirectory() as d:
                    composer.export(composer.compose(a), Path(d) / "a")
                    composer.export(composer.compose(b), Path(d) / "b")
                    self.assertEqual((Path(d) / "a.mid").read_bytes(), (Path(d) / "b.mid").read_bytes(), seed)

    def test_riff_bars_rarely_end_in_a_long_rest(self):
        def share(notail):
            long_tail = bars = 0
            for seed in range(1001, 1041):
                rec = self.recipe(seed, notail)
                hook = rec["motifs"]["hook"]["rhythm"]
                L = composer.METERS[rec["intent"]["meter"]][0]
                bars += 1
                long_tail += L - hook[-1] >= 4
            return long_tail / bars
        self.assertLess(share(True), share(False) / 2, (share(False), share(True)))


class CorpusDoses(unittest.TestCase):
    """v5 lands inside the corpus interquartile ranges of CORPUS-STUDY.md."""
    seeds = range(501, 541)

    @classmethod
    def setUpClass(cls):
        import sys
        sys.path.insert(0, str(Path(__file__).with_name("corpus")))
        import study
        cls.study = study
        cls.stats = study.generator_stats(cls.seeds, texture=True)

    def recipe(self, seed, texture):
        args = composer.argparse.Namespace(meter=None, mode=None, key=None, tempo=None, arrange=True, repeat=True,
                                           texture=texture)
        return composer.make_recipe(seed, args)

    def median(self, f):
        import statistics
        return statistics.median(f(s) for s in self.stats)

    def test_v5_recipe_differs_from_v4_only_in_the_form_plan(self):
        for seed in self.seeds:
            a, b = self.recipe(seed * 1000, False), self.recipe(seed * 1000, True)
            self.assertIn("texture", b)
            for k in ("seed", "intent", "harmony", "motifs", "arrange", "repeat"):
                self.assertEqual(a[k], b[k], (seed, k))
            strip = lambda f: [{k: v for k, v in x.items() if k not in ("prog", "role", "tex", "end")} for x in f]
            self.assertEqual(strip(a["form"]), strip(b["form"]), seed)

    def test_one_guitar_plays_alone_in_corpus_range(self):
        m = self.median(lambda s: s["relation"]["one_silent"])
        self.assertTrue(0.11 <= m <= 0.65, m)

    def test_same_rhythm_doubling_in_corpus_range(self):
        m = self.median(lambda s: sum(s["relation"][k] for k in ("unison", "octave", "harmony", "same_rhythm")))
        self.assertTrue(0.11 <= m <= 0.52, m)

    def test_square_ends_vary_rarely(self):
        m = self.median(lambda s: s["square_varied"] if s["square_varied"] is not None else 0)
        self.assertLessEqual(m, 0.4)

    def test_endings_are_not_all_held(self):
        held = sum(s["last_bar"] == "held" for s in self.stats) / len(self.stats)
        self.assertTrue(0.3 <= held <= 0.8, held)

    def test_octave_doubling_is_rare(self):
        n = sum(any(b.get("tex") == "octave" for b in self.recipe(seed * 1000, True)["form"]) for seed in self.seeds)
        self.assertTrue(0.1 * len(self.seeds) <= n <= 0.45 * len(self.seeds), n)


class MusicXmlExport(unittest.TestCase):
    def test_every_measure_fills_its_meter_exactly(self):
        import xml.etree.ElementTree as ET
        for seed in range(1, 21):
            with tempfile.TemporaryDirectory() as d:
                composer.export(build(seed, arrange=seed % 4 == 1, repeat=seed % 4 == 2, texture=seed % 4 == 3), Path(d) / "x")
                root = ET.parse(Path(d) / "x.musicxml").getroot()
            for part in root.iter("part"):
                num = den = None
                for m in part.iter("measure"):
                    t = m.find("attributes/time")
                    if t is not None:
                        num, den = int(t.findtext("beats")), int(t.findtext("beat-type"))
                    total = sum(int(n.findtext("duration")) for n in m.iter("note") if n.find("chord") is None)
                    self.assertEqual(total, num * 16 // den, (seed, part.get("id"), m.get("number")))


if __name__ == "__main__":
    unittest.main()
