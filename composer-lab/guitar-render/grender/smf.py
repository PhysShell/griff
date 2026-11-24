"""Standard MIDI File reader/writer (standard library only) with a full tempo map."""
import struct
from dataclasses import dataclass, field


def _vlq_read(data, i):
    v = 0
    while True:
        b = data[i]
        i += 1
        v = (v << 7) | (b & 0x7F)
        if b < 0x80:
            return v, i


def _vlq(n):
    out = [n & 0x7F]
    n >>= 7
    while n:
        out.append(0x80 | (n & 0x7F))
        n >>= 7
    return bytes(reversed(out))


@dataclass
class Track:
    name: str
    events: list = field(default_factory=list)  # (tick, order, status_or_meta, bytes payload)


@dataclass
class Song:
    fmt: int
    ppq: int
    tracks: list
    tempos: list   # (tick, usec_per_quarter), sorted, first at tick 0
    meters: list   # (tick, num, den)

    def seconds(self, tick):
        t, s, us = 0, 0.0, 500000
        for tt, u in self.tempos:
            if tt >= tick:
                break
            s += (tt - t) * us / 1e6 / self.ppq
            t, us = tt, u
        return s + (tick - t) * us / 1e6 / self.ppq


def read(path):
    d = open(path, "rb").read()
    if d[:4] != b"MThd":
        raise ValueError("not a MIDI file")
    hl, fmt, ntrk, div = struct.unpack(">IHHH", d[4:14])
    if div & 0x8000:
        raise ValueError("SMPTE time division is not supported")
    pos = 8 + hl
    tracks, tempos, meters = [], [], []
    seq = 0
    while pos < len(d) and len(tracks) < ntrk:
        tag, ln = d[pos:pos + 4], struct.unpack(">I", d[pos + 4:pos + 8])[0]
        body = d[pos + 8:pos + 8 + ln]
        pos += 8 + ln
        if tag != b"MTrk":
            continue
        tr = Track(name="")
        i, tick, run = 0, 0, None
        while i < len(body):
            dt, i = _vlq_read(body, i)
            tick += dt
            st = body[i]
            if st >= 0x80:
                i += 1
                if st < 0xF0:
                    run = st
            else:
                st = run
            if st == 0xFF:
                mt = body[i]
                ml, j = _vlq_read(body, i + 1)
                payload = body[j:j + ml]
                i = j + ml
                if mt == 0x2F:
                    break
                if mt == 0x51:
                    tempos.append((tick, seq, int.from_bytes(payload, "big")))
                elif mt == 0x58:
                    meters.append((tick, payload[0], 2 ** payload[1]))
                elif mt == 0x03 and not tr.name:
                    tr.name = payload.decode("latin-1")
                tr.events.append((tick, seq, ("meta", mt), bytes(payload)))
            elif st in (0xF0, 0xF7):
                ml, j = _vlq_read(body, i)
                tr.events.append((tick, seq, ("sysex", st), bytes(body[j:j + ml])))
                i = j + ml
            else:
                n = 1 if (st & 0xF0) in (0xC0, 0xD0) else 2
                tr.events.append((tick, seq, ("ch", st), bytes(body[i:i + n])))
                i += n
            seq += 1
        tracks.append(tr)
    # Same-tick tempo conflicts: the last event in file order wins.
    tempos.sort(key=lambda t: (t[0], t[1]))
    tm = {}
    for tick, _s, us in tempos:
        tm[tick] = us
    tempo_list = sorted(tm.items())
    if not tempo_list or tempo_list[0][0] != 0:
        tempo_list.insert(0, (0, 500000))
    return Song(fmt, div, tracks, tempo_list, sorted(set(meters)))


def notes(track):
    """Pair note-ons and note-offs per (channel, pitch), first-in first-out; returns dicts."""
    out, open_ = [], {}
    for tick, seq, kind, pl in sorted(track.events, key=lambda e: (e[0], e[1])):
        if kind[0] != "ch":
            continue
        st = kind[1]
        typ, ch = st & 0xF0, st & 0x0F
        if typ == 0x90 and pl[1] > 0:
            open_.setdefault((ch, pl[0]), []).append((tick, pl[1]))
        elif typ == 0x80 or (typ == 0x90 and pl[1] == 0):
            q = open_.get((ch, pl[0]))
            if q:
                on, vel = q.pop(0)
                out.append({"on": on, "off": tick, "pitch": pl[0], "vel": vel, "ch": ch})
    hanging = sum(len(q) for q in open_.values())
    out.sort(key=lambda n: (n["on"], n["pitch"]))
    return out, hanging


def channel_controls(track):
    """Non-note channel events (CC, pitch bend, program, aftertouch) with ticks."""
    out = []
    for tick, seq, kind, pl in sorted(track.events, key=lambda e: (e[0], e[1])):
        if kind[0] == "ch" and (kind[1] & 0xF0) not in (0x80, 0x90):
            out.append((tick, kind[1], pl))
    return out


def write(path, ppq, tempos, meters, tracks):
    """tracks: list of (name, [(tick, order, bytes_event)]) where bytes_event is a complete channel or meta message."""
    def chunk(events):
        data, last = bytearray(), 0
        for tick, _o, ev in sorted(events, key=lambda e: (e[0], e[1])):
            data += _vlq(tick - last) + ev
            last = tick
        data += _vlq(0) + b"\xff\x2f\x00"
        return b"MTrk" + struct.pack(">I", len(data)) + bytes(data)

    cond = [(t, 0, b"\xff\x51\x03" + us.to_bytes(3, "big")) for t, us in tempos]
    for t, num, den in meters:
        dd = {1: 0, 2: 1, 4: 2, 8: 3, 16: 4, 32: 5}[den]
        cond.append((t, 1, b"\xff\x58\x04" + bytes([num, dd, 24, 8])))
    chunks = [chunk(cond)]
    for name, evs in tracks:
        nm = name.encode()
        chunks.append(chunk([(0, -1, b"\xff\x03" + _vlq(len(nm)) + nm)] + evs))
    data = b"MThd" + struct.pack(">IHHH", 6, 1, len(chunks), ppq) + b"".join(chunks)
    with open(path, "wb") as f:
        f.write(data)
