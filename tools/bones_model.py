import sys, struct
sys.path.insert(0, "tools")
import numpy as np
from wowmem import *
p = Proc(pid_of_wow())
o = player_object(p)
m = p.u32(o + 0xB4); x = p.u32(m + 0x38); arr = p.u32(x + 0x64)
H = 0x1fbc1030
nb, ob = struct.unpack_from("<II", p.read(H, 0x40), 0x2C)
raw = p.read(ob, nb * 0x58)
piv = [struct.unpack_from("<3f", raw[i*0x58:(i+1)*0x58], 0x4C) for i in range(nb)]
par = [struct.unpack_from("<h", raw[i*0x58:(i+1)*0x58], 8)[0] for i in range(nb)]
mats = np.frombuffer(p.read(arr, 64 * nb), dtype="<f4").reshape(nb, 4, 4).astype(float)
W = mats[0]
Wi = np.linalg.inv(W)
np.set_printoptions(precision=3, suppress=True)
print("W =\n", W)
for b in (1, 2, 3, 5, 16, 44):
    B = mats[b] @ Wi
    print("bone", b, "parent", par[b], "pivot", piv[b])
    print(B)
