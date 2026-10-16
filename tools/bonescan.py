"""Reverse-engineering aid: find runtime bone-matrix arrays in the 3.3.5a client and the player's own.
python -I tools/bonescan.py scan      list runs of 4x4 transforms (candidate bone arrays) and which ones animate
python -I tools/bonescan.py link      walk pointers from the local player object to those arrays"""
import sys, struct, math, time
sys.path.insert(0, __file__.rsplit("\\", 1)[0] if "\\" in __file__ else ".")
from wowmem import *


def orth(a, b, c):
    n = lambda v: math.sqrt(v[0] ** 2 + v[1] ** 2 + v[2] ** 2)
    return (all(0.5 < n(v) < 1.6 for v in (a, b, c))
            and abs(sum(x * y for x, y in zip(a, b))) < 0.1 * n(a) * n(b) + 0.02
            and abs(sum(x * y for x, y in zip(a, c))) < 0.1 * n(a) * n(c) + 0.02)


def kind(f):
    if f[3] == 0 and f[7] == 0 and f[11] == 0 and f[15] == 1:
        return "rowmajor-T" if orth(f[0:3], f[4:7], f[8:11]) else None
    if f[12] == 0 and f[13] == 0 and f[14] == 0 and f[15] == 1:
        return "colmajor" if orth((f[0], f[4], f[8]), (f[1], f[5], f[9]), (f[2], f[6], f[10])) else None
    return None


def scan(p, minrun=60):
    res = []
    for base, size in p.regions(True):
        off = 0
        while off < size:
            n = min(0x400000, size - off)
            data = p.read(base + off, n)
            if not data:
                break
            cnt = len(data) // 4
            fl = struct.unpack_from("<%df" % cnt, data, 0)
            run = 0
            start = 0
            kd = None
            i = 0
            while i + 16 <= cnt:
                k = kind(fl[i:i + 16])
                if k and kd in (None, k):
                    if run == 0:
                        start = i
                    run += 1
                    kd = k
                    i += 16
                else:
                    if run >= minrun:
                        res.append((base + off + start * 4, run, kd))
                    run = 0
                    kd = None
                    i += 1 if not k else 16
            if run >= minrun:
                res.append((base + off + start * 4, run, kd))
            off += n
    return res


def changes(x, y):
    if not x or not y:
        return -1
    fx = struct.unpack("<%df" % (len(x) // 4), x)
    fy = struct.unpack("<%df" % (len(y) // 4), y)
    return sum(1 for i in range(0, len(fx), 16) if any(abs(fx[i + j] - fy[i + j]) > 1e-4 for j in range(16)))


if __name__ == "__main__":
    cmd = sys.argv[1]
    p = Proc(pid_of_wow())
    res = scan(p)
    cands = [(a, r) for a, r, k in res]
    if cmd == "scan":
        s1 = {a: p.read(a, 64 * r) for a, r in cands}
        time.sleep(0.5)
        s2 = {a: p.read(a, 64 * r) for a, r in cands}
        for a, r in cands:
            print(hex(a), r, changes(s1[a], s2[a]))
    elif cmd == "link":
        o = player_object(p)
        print("player obj", hex(o))

        def in_cand(v):
            for a, r in cands:
                if a <= v < a + r * 64:
                    return (a, r)
            return None
        seen = {o}
        frontier = [(o, 0x1400, "obj")]
        found = []
        for depth in range(3):
            nxt = []
            for base, size, path in frontier:
                d = p.read(base, size)
                if not d:
                    continue
                for off in range(0, len(d) - 3, 4):
                    v = struct.unpack_from("<I", d, off)[0]
                    if v < 0x10000 or v > 0x7FFE0000 or v in seen:
                        continue
                    c = in_cand(v)
                    if c:
                        found.append((path + "+%#x" % off, hex(v), hex(c[0]), c[1]))
                        continue
                    seen.add(v)
                    nxt.append((v, 0x400, path + "+%#x" % off))
            frontier = nxt
            print("depth", depth, "frontier", len(frontier), "found", len(found))
        for f in found[:40]:
            print(f)
