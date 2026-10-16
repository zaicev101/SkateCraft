import sys
sys.path.insert(0, "tools")
from wowmem import *
addr = int(sys.argv[1], 16); span = int(sys.argv[2], 16)
p = Proc(pid_of_wow())
base = addr - span
code = p.read(base, span + 0x10)
print([hex(base + i) for i in range(len(code) - 3) if code[i:i+3] == b"\x55\x8b\xec"][-6:])
print("int3 padding runs ending before target:", [hex(base+i+1) for i in range(len(code)-1) if code[i]==0xCC and code[i+1]!=0xCC][-4:])
