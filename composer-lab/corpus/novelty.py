"""Copy check: share of generated 8-note windows (pitch intervals plus onset
gaps, transposition-invariant) that also occur in the tab corpus.

python corpus/novelty.py <tabs-dir> <seeds-from> <seeds-to> <flag>...   (flags: arrange repeat texture)
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import study

N = 7


def grams(line):
    """line: (absolute onset16, pitch) sorted; the top pitch per onset."""
    top = {}
    for o, p in line:
        top[o] = max(p, top.get(o, p))
    seq = sorted(top.items())
    tok = [(b[1] - a[1], b[0] - a[0]) for a, b in zip(seq, seq[1:]) if b[0] - a[0] <= 16]
    return {tuple(tok[i:i + N]) for i in range(len(tok) - N + 1)}


def corpus_grams(tabs):
    out, songs = set(), 0
    for p in sorted(Path(tabs).iterdir()):
        if p.suffix.lower() not in (".gp3", ".gp4", ".gp5"):
            continue
        try:
            song, _ = study.load_song(p)
        except Exception:
            continue
        if song is None:
            continue
        songs += 1
        t = 0
        lines = ([], [])
        for b in song["bars"]:
            for k in (0, 1):
                lines[k].extend((t + o, pitch) for o, pitch in b["parts"][k])
            t += b["L"]
        for line in lines:
            out |= grams(line)
    return out, songs


def piece_lines(piece):
    return [[(n["start"], n["pitch"]) for n in piece["parts"][g]] for g in ("g1", "g2")]


def main():
    import composer
    tabs, a, b = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    flags = {f: True for f in sys.argv[4:]}
    ref, songs = corpus_grams(tabs)
    hit = tot = 0
    worst = (0, None)
    for seed in range(a, b + 1):
        args = composer.argparse.Namespace(meter=None, mode=None, key=None, tempo=None, **flags)
        piece = composer.compose(composer.make_recipe(seed * 1000, args))
        g = set().union(*(grams(x) for x in piece_lines(piece)))
        h = len(g & ref)
        hit, tot = hit + h, tot + len(g)
        worst = max(worst, (h / max(1, len(g)), seed))
    print(f"corpus: {songs} songs, {len(ref)} distinct windows; generated {sorted(flags)} seeds {a}-{b}: "
          f"{hit}/{tot} windows found in corpus ({hit / max(1, tot):.3f}); worst piece seed {worst[1]} {worst[0]:.3f}")


if __name__ == "__main__":
    main()
