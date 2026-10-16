import sys
sys.path.insert(0, "tools")
from wowmem import *
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
addr = int(sys.argv[1], 16)
back = int(sys.argv[2], 16) if len(sys.argv) > 2 else 0x80
fwd = int(sys.argv[3], 16) if len(sys.argv) > 3 else 0x100
p = Proc(pid_of_wow())
md = Cs(CS_ARCH_X86, CS_MODE_32)
base = addr - back
code = p.read(base, back + fwd)
best = None
for off in range(0, 24):
    addrs = [i.address for i in md.disasm(code[off:], base + off)]
    if addr in addrs:
        best = off
        break
for i in md.disasm(code[best:], base + best):
    print("%08x  %-8s %s%s" % (i.address, i.mnemonic, i.op_str, "   <== write" if i.address == addr else ""))
