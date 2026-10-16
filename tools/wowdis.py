"""Disassemble code of the running 3.3.5a client:  python tools/dis.py <start-hex> [<bytes-hex>] [wow-index]"""
import sys
sys.path.insert(0, "tools")
from wowmem import *
from capstone import Cs, CS_ARCH_X86, CS_MODE_32

start = int(sys.argv[1], 16)
n = int(sys.argv[2], 16) if len(sys.argv) > 2 else 0x200
p = Proc(pid_of_wow(int(sys.argv[3]) if len(sys.argv) > 3 else 0))
code = p.read(start, n)
md = Cs(CS_ARCH_X86, CS_MODE_32)
for i in md.disasm(code, start):
    print("%08x  %-10s %s" % (i.address, i.mnemonic, i.op_str))
