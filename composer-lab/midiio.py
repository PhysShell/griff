"""Minimal standard-library MIDI type-1 writer."""
import struct

PPQ = 480
SIXTEENTH = PPQ // 4


def _vlq(n):
    out = [n & 0x7F]
    n >>= 7
    while n:
        out.append(0x80 | (n & 0x7F))
        n >>= 7
    return bytes(reversed(out))


def _track(events):
    events = sorted(events, key=lambda e: (e[0], e[1]))
    data = bytearray()
    last = 0
    for tick, _order, payload in events:
        data += _vlq(tick - last) + payload
        last = tick
    data += _vlq(0) + b"\xff\x2f\x00"
    return b"MTrk" + struct.pack(">I", len(data)) + bytes(data)


def midi_bytes(tempo_bpm, meters, tracks):
    """meters: list of (start_tick, num, den). tracks: list of dicts with
    name, channel, program, pan, notes=[(start16, dur16, pitch, vel)]."""
    conductor = [(0, 0, b"\xff\x51\x03" + int(60_000_000 / tempo_bpm).to_bytes(3, "big"))]
    for tick, num, den in meters:
        dd = {2: 1, 4: 2, 8: 3, 16: 4}[den]
        conductor.append((tick, 1, b"\xff\x58\x04" + bytes([num, dd, 24, 8])))
    chunks = [_track(conductor)]
    for t in tracks:
        ch = t["channel"]
        name = t["name"].encode()
        ev = [
            (0, 0, b"\xff\x03" + _vlq(len(name)) + name),
            (0, 1, bytes([0xC0 | ch, t["program"]])),
            (0, 2, bytes([0xB0 | ch, 10, t["pan"]])),
        ]
        for start, dur, pitch, vel in t["notes"]:
            on = int(round(start * SIXTEENTH))
            off = max(on + 1, int(round((start + dur) * SIXTEENTH)) - 6)
            ev.append((on, 4, bytes([0x90 | ch, pitch, vel])))
            ev.append((off, 3, bytes([0x80 | ch, pitch, 0])))
        chunks.append(_track(ev))
    header = b"MThd" + struct.pack(">IHHH", 6, 1, len(chunks), PPQ)
    return header + b"".join(chunks)


def write_midi(path, tempo_bpm, meters, tracks):
    data = midi_bytes(tempo_bpm, meters, tracks)
    with open(path, "wb") as f:
        f.write(data)
