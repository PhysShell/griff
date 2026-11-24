"""Read the tablature MusicXML that composer-lab writes: onset, pitch, string/fret and techniques per note."""
import xml.etree.ElementTree as ET

STEP = {"C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11}


def _midi(p):
    return 12 * (int(p.findtext("octave")) + 1) + STEP[p.findtext("step")] + int(float(p.findtext("alter") or 0))


def read(path):
    """Returns {part_id: {"name": str, "notes": [...], "pm_marks": [onset16...]}}; onsets are in sixteenths."""
    root = ET.parse(path).getroot()
    names = {sp.get("id"): sp.findtext("part-name") for sp in root.iter("score-part")}
    out = {}
    for part in root.iter("part"):
        pid = part.get("id")
        div = 4
        t = 0.0
        last_onset = 0.0
        notes, pm_marks = [], []
        for meas in part.iter("measure"):
            for el in meas:
                if el.tag == "attributes" and el.findtext("divisions"):
                    div = int(el.findtext("divisions"))
                elif el.tag == "direction":
                    w = el.findtext("direction-type/words")
                    if w and w.strip().upper().startswith("P.M"):
                        pm_marks.append(t * 4 / div)
                elif el.tag == "backup":
                    t -= int(el.findtext("duration"))
                elif el.tag == "forward":
                    t += int(el.findtext("duration"))
                elif el.tag == "note":
                    dur = int(el.findtext("duration") or 0)
                    is_chord = el.find("chord") is not None
                    onset = last_onset if is_chord else t
                    if not is_chord:
                        last_onset = t
                        t += dur
                    if el.find("rest") is not None:
                        continue
                    ties = [x.get("type") for x in el.findall("tie")]
                    if "stop" in ties:
                        continue
                    tech = el.find("notations/technical")
                    rec = {"onset16": onset * 4 / div, "pitch": _midi(el.find("pitch")), "tech": [],
                           "string": None, "fret": None}
                    if tech is not None:
                        if tech.findtext("string"):
                            rec["string"] = int(tech.findtext("string"))
                            rec["fret"] = int(tech.findtext("fret"))
                        for k, name in (("hammer-on", "hammer"), ("pull-off", "pull")):
                            for x in tech.findall(k):
                                if x.get("type") == "stop":
                                    rec["tech"].append(name)
                                elif x.get("type") == "start":
                                    rec["tech"].append(name + "_from")
                        if tech.find("tap") is not None:
                            rec["tech"].append("tap")
                    for x in el.findall("notations/slide"):
                        rec["tech"].append("slide" if x.get("type") == "stop" else "slide_from")
                    if el.findtext("notehead") == "x":
                        rec["tech"].append("dead")
                    notes.append(rec)
        out[pid] = {"name": names.get(pid, pid), "notes": notes, "pm_marks": pm_marks}
    return out
