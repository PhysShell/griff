"""MusicXML tablature export (opens in Guitar Pro, TuxGuitar, MuseScore).

Written durations run onset-to-next-onset inside a bar; ringing is shown with
let-ring text instead of overlapping voices. Techniques are the generator's
own decisions: tap, hammer-on, pull-off, slide, palm mute.
"""
from xml.sax.saxutils import escape

DIV = 4  # divisions per quarter = sixteenths
TYPES = {16: ("whole", 0), 12: ("half", 1), 8: ("half", 0), 6: ("quarter", 1), 4: ("quarter", 0),
         3: ("eighth", 1), 2: ("eighth", 0), 1: ("16th", 0)}
STEPS = ["C", "C", "D", "D", "E", "F", "F", "G", "G", "A", "A", "B"]
ALTER = [0, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0]
TUNING = [("E", 2), ("A", 2), ("D", 3), ("G", 3), ("B", 3), ("E", 4)]


def split_dur(d):
    out = []
    for v in sorted(TYPES, reverse=True):
        while d >= v:
            out.append(v)
            d -= v
    return out


def _pitch(p):
    a = ALTER[p % 12]
    return (f"<pitch><step>{STEPS[p % 12]}</step>" + (f"<alter>{a}</alter>" if a else "")
            + f"<octave>{p // 12 - 1}</octave></pitch>")


def _bars(piece):
    starts = [c["start"] for c in piece["chords"]] + [piece["total16"]]
    meters = sorted(piece["meters"])
    out = []
    for i in range(len(starts) - 1):
        t = starts[i]
        num, den = next((n, d) for tick, n, d in reversed(meters) if tick <= t * 120)
        out.append((t, starts[i + 1], num, den))
    return out


def part_xml(piece, g, pid):
    notes = [n for n in piece["parts"][g] if n["start"] >= 0]
    xml = [f'<part id="{pid}">']
    prev_meter = None
    marks_next = {}
    seq = sorted(notes, key=lambda n: (n["start"], n["string"]))
    by_string_prev = {}
    for n in seq:
        p = by_string_prev.get(n["string"])
        for tech in n.get("tech", []):
            if tech in ("hammer", "pull", "slide") and p is not None:
                marks_next.setdefault(id(p), []).append(("start", tech))
                marks_next.setdefault(id(n), []).append(("stop", tech))
        by_string_prev[n["string"]] = n
    for bi, (b0, b1, num, den) in enumerate(_bars(piece)):
        xml.append(f'<measure number="{bi + 1}">')
        if bi == 0 or (num, den) != prev_meter:
            xml.append("<attributes>")
            if bi == 0:
                xml.append(f"<divisions>{DIV}</divisions>")
            xml.append(f"<time><beats>{num}</beats><beat-type>{den}</beat-type></time>")
            if bi == 0:
                xml.append('<clef><sign>TAB</sign><line>5</line></clef><staff-details><staff-lines>6</staff-lines>')
                for i, (st, oc) in enumerate(TUNING):
                    xml.append(f'<staff-tuning line="{i + 1}"><tuning-step>{st}</tuning-step>'
                               f'<tuning-octave>{oc}</tuning-octave></staff-tuning>')
                xml.append("</staff-details>")
            xml.append("</attributes>")
            if bi == 0:
                xml.append(f'<direction placement="above"><direction-type><metronome><beat-unit>quarter</beat-unit>'
                           f'<per-minute>{piece["tempo"]}</per-minute></metronome></direction-type>'
                           f'<sound tempo="{piece["tempo"]}"/></direction>')
            prev_meter = (num, den)
        inbar = [n for n in seq if b0 <= n["start"] < b1 and float(n["start"]).is_integer()]
        onsets = sorted({int(n["start"]) for n in inbar})
        cur = b0
        pm_on = False
        for k, on in enumerate(onsets):
            if on > cur:
                for d in split_dur(on - cur):
                    xml.append(f"<note><rest/><duration>{d}</duration><type>{TYPES[d][0]}</type>"
                               + ("<dot/>" if TYPES[d][1] else "") + "</note>")
            nxt = onsets[k + 1] if k + 1 < len(onsets) else b1
            group = [n for n in inbar if int(n["start"]) == on]
            longest = max(n["dur"] for n in group)
            written = max(1, min(nxt - on, int(round(longest)) if nxt == b1 else nxt - on))
            pieces = split_dur(written)
            is_pm = any("pm" in n.get("tech", []) for n in group)
            if is_pm and not pm_on:
                xml.append('<direction placement="above"><direction-type><words>P.M.</words></direction-type></direction>')
            pm_on = is_pm
            for ci, n in enumerate(group):
                for pi, d in enumerate(pieces):
                    tie = []
                    if len(pieces) > 1 and pi < len(pieces) - 1:
                        tie.append("start")
                    if pi > 0:
                        tie.append("stop")
                    x = ["<note>"]
                    if ci > 0:
                        x.append("<chord/>")
                    x.append(_pitch(n["pitch"]))
                    x.append(f"<duration>{d}</duration>")
                    x += [f'<tie type="{t}"/>' for t in tie]
                    x.append(f"<type>{TYPES[d][0]}</type>" + ("<dot/>" if TYPES[d][1] else ""))
                    if pi == 0 and "dead" in n.get("tech", []):
                        x.append("<notehead>x</notehead>")
                    tech = [f"<string>{6 - n['string']}</string><fret>{n['fret']}</fret>"]
                    other = [f'<tied type="{t}"/>' for t in tie]
                    if pi == 0:
                        if "tap" in n.get("tech", []):
                            tech.append("<tap/>")
                        for kind, tch in marks_next.get(id(n), []):
                            if tch == "hammer":
                                tech.append(f'<hammer-on type="{kind}">H</hammer-on>')
                            elif tch == "pull":
                                tech.append(f'<pull-off type="{kind}">P</pull-off>')
                            else:
                                other.append(f'<slide type="{kind}"/>')
                    x.append("<notations><technical>" + "".join(tech) + "</technical>" + "".join(other) + "</notations>")
                    x.append("</note>")
                    xml.append("".join(x))
            cur = on + written
        if cur < b1:
            for d in split_dur(b1 - cur):
                xml.append(f"<note><rest/><duration>{d}</duration><type>{TYPES[d][0]}</type>"
                           + ("<dot/>" if TYPES[d][1] else "") + "</note>")
        xml.append("</measure>")
    xml.append("</part>")
    return "\n".join(xml)


def musicxml_text(piece, title):
    parts = [("g1", "P1", "Guitar 1"), ("g2", "P2", "Guitar 2")]
    head = ['<?xml version="1.0" encoding="UTF-8"?>',
            '<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" '
            '"http://www.musicxml.org/dtds/partwise.dtd">',
            '<score-partwise version="4.0">',
            f"<work><work-title>{escape(title)}</work-title></work>", "<part-list>"]
    for g, pid, name in parts:
        head.append(f'<score-part id="{pid}"><part-name>{name}</part-name>'
                    f'<score-instrument id="{pid}-I1"><instrument-name>Electric Guitar</instrument-name>'
                    f'</score-instrument><midi-instrument id="{pid}-I1"><midi-program>28</midi-program>'
                    f"</midi-instrument></score-part>")
    head.append("</part-list>")
    body = [part_xml(piece, g, pid) for g, pid, _ in parts]
    return "\n".join(head + body + ["</score-partwise>"])


def write_musicxml(piece, path, title):
    text = musicxml_text(piece, title)
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)
