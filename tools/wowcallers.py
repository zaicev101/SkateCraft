import sys, struct
sys.path.insert(0, "tools")
from wowmem import *
target = int(sys.argv[1], 16)
p = Proc(pid_of_wow())
lo, hi = 0x401000, 0xA00000
code = p.read(lo, hi - lo)
out = []
i = code.find(b"\xE8")
while i != -1 and i + 5 <= len(code):
    rel = struct.unpack_from("<i", code, i + 1)[0]
    if (lo + i + 5 + rel) & 0xFFFFFFFF == target:
        out.append(lo + i)
    i = code.find(b"\xE8", i + 1)
print("calls to %#x from:" % target, [hex(a) for a in out])
# also jmp / pointers
ptr = struct.pack("<I", target)
j = code.find(ptr)
refs = []
while j != -1:
    refs.append(hex(lo + j)); j = code.find(ptr, j + 1)
print("absolute refs:", refs[:10])
