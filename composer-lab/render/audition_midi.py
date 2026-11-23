"""Copy a MIDI file forcing every channel onto program 0 (the single-preset
guitar SoundFont) so every generator is heard through the same instrument."""
import struct
import sys


def _vlq(data, i):
    v = 0
    while True:
        b = data[i]
        i += 1
        v = (v << 7) | (b & 0x7F)
        if b < 0x80:
            return v, i


def remap(src, dst):
    d = bytearray(open(src, "rb").read())
    pos = 14
    out = bytearray(d[:14])
    while pos < len(d):
        tag, ln = d[pos:pos + 4], struct.unpack(">I", d[pos + 4:pos + 8])[0]
        body = d[pos + 8:pos + 8 + ln]
        if tag == b"MTrk":
            i, run, seen = 0, None, set()
            new = bytearray()
            while i < len(body):
                start = i
                _, i = _vlq(body, i)
                delta_bytes = body[start:i]
                st = body[i]
                if st >= 0x80:
                    i += 1
                    if st < 0xF0:
                        run = st
                else:
                    st = run
                if st == 0xFF:
                    mt = body[i]
                    l, j = _vlq(body, i + 1)
                    new += delta_bytes + bytes([0xFF]) + body[i:j + l]
                    i = j + l
                elif st in (0xF0, 0xF7):
                    l, j = _vlq(body, i)
                    new += delta_bytes + bytes([st]) + body[i:j + l]
                    i = j + l
                else:
                    kind = st & 0xF0
                    n = 1 if kind in (0xC0, 0xD0) else 2
                    args = bytearray(body[i:i + n])
                    i += n
                    ch = st & 0x0F
                    if kind == 0xC0:
                        args[0] = 0
                        seen.add(ch)
                    if kind == 0x90 and ch not in seen:
                        new += delta_bytes + bytes([0xC0 | ch, 0])
                        delta_bytes = b"\x00"
                        seen.add(ch)
                    new += delta_bytes + bytes([st]) + args
            body = new
        out += tag + struct.pack(">I", len(body)) + body
        pos += 8 + ln
    open(dst, "wb").write(out)


if __name__ == "__main__":
    remap(sys.argv[1], sys.argv[2])
