import sys, struct
sys.path.insert(0, "tools")
from wowmem import *
p = Proc(pid_of_wow())
o = player_object(p)
m = p.u32(o + 0xB4)
print("obj", hex(o), "[+0xB4] =", hex(m))
x = p.u32(m + 0x38)
print("[m+0x38] =", hex(x))
arr = p.u32(x + 0x64)
print("[x+0x64] =", hex(arr))
d = p.read(x, 0x100)
for i in range(0, 0x100, 4):
    v = struct.unpack_from("<I", d, i)[0]; f = struct.unpack_from("<f", d, i)[0]
    print("  x+%#04x: %08x %s" % (i, v, ("%.4f" % f) if 1e-3 < abs(f) < 1e5 else ""))
raw = p.read(arr, 64 * 12)
for b in range(12):
    f = struct.unpack_from("<16f", raw, b * 64)
    print(b, " ".join("%7.3f" % v for v in f))
