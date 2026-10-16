"""Read-only helpers for poking at the running 3.3.5a client's memory (reverse-engineering aid).
Run with: python -I tools/wowmem.py <command> ...   (needs no injection; uses ReadProcessMemory)."""
import ctypes, ctypes.wintypes as wt, struct, sys

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
k32.OpenProcess.restype = wt.HANDLE
k32.ReadProcessMemory.argtypes = [wt.HANDLE, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]


class MBI(ctypes.Structure):
    _fields_ = [("BaseAddress", ctypes.c_void_p), ("AllocationBase", ctypes.c_void_p), ("AllocationProtect", wt.DWORD),
                ("RegionSize", ctypes.c_size_t), ("State", wt.DWORD), ("Protect", wt.DWORD), ("Type", wt.DWORD)]


def pid_of_wow(index=0):
    import subprocess
    out = subprocess.check_output(["tasklist", "/FI", "IMAGENAME eq Wow.exe", "/FO", "CSV", "/NH"], text=True)
    pids = [int(l.split(",")[1].strip('"')) for l in out.splitlines() if l.startswith('"Wow.exe"')]
    return pids[index]


class Proc:
    def __init__(self, pid):
        self.h = k32.OpenProcess(0x0410, False, pid)  # QUERY_INFORMATION | VM_READ
        if not self.h:
            raise OSError("OpenProcess failed")

    def read(self, addr, n):
        buf = ctypes.create_string_buffer(n)
        got = ctypes.c_size_t(0)
        if not k32.ReadProcessMemory(self.h, ctypes.c_void_p(addr), buf, n, ctypes.byref(got)):
            return None
        return buf.raw[: got.value]

    def u32(self, a):
        b = self.read(a, 4)
        return struct.unpack("<I", b)[0] if b and len(b) == 4 else None

    def f32(self, a):
        b = self.read(a, 4)
        return struct.unpack("<f", b)[0] if b and len(b) == 4 else None

    def regions(self, writable_only=True):
        mbi = MBI()
        addr = 0x10000
        while addr < 0x7FFE0000:
            if not k32.VirtualQueryEx(self.h, ctypes.c_void_p(addr), ctypes.byref(mbi), ctypes.sizeof(mbi)):
                break
            if mbi.State == 0x1000 and not (mbi.Protect & 0x101):  # committed, not NOACCESS/GUARD
                if not writable_only or (mbi.Protect & 0xCC):  # any write/exec-write
                    yield mbi.BaseAddress or 0, mbi.RegionSize
            addr = (mbi.BaseAddress or 0) + mbi.RegionSize

    def find_all(self, needle, writable_only=True):
        hits = []
        for base, size in self.regions(writable_only):
            off = 0
            while off < size:
                chunk = self.read(base + off, min(0x100000, size - off))
                if not chunk:
                    break
                i = chunk.find(needle)
                while i != -1:
                    hits.append(base + off + i)
                    i = chunk.find(needle, i + 1)
                off += 0x100000
        return hits


def player_object(p):
    conn = p.u32(0xC79CE0)
    om = p.u32(conn + 0x2ED0)
    guid = p.read(om + 0xC0, 8)
    obj = p.u32(om + 0xAC)
    n = 0
    while obj and not (obj & 1) and n < 5000:
        if p.read(obj + 0x30, 8) == guid:
            return obj
        obj = p.u32(obj + 0x3C)
        n += 1
    return None


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "md20"
    p = Proc(pid_of_wow(int(sys.argv[2]) if len(sys.argv) > 2 else 0))
    if cmd == "md20":
        # every loaded M2 model header: magic, version 264, name, bone count/offset
        for a in p.find_all(b"MD20\x08\x01\x00\x00", writable_only=False):
            h = p.read(a, 0x60)
            if not h:
                continue
            (nlen, noff, flags, ngs, ogs, nanim, oanim, nal, oal, nbones, obones) = struct.unpack_from("<11I", h, 8)
            name = (p.read(noff, min(nlen, 64)) or b"")[:nlen].split(b"\0")[0] if noff > 0x10000 else b"?"
            print(f"MD20 at {a:#x}: name={name!r} bones={nbones} at {obones:#x} anims={nanim}")
    elif cmd == "player":
        o = player_object(p)
        print(f"player object {o:#x}")
        for off in range(0, 0x1000, 4):
            v = p.u32(o + off)
            if v and 0x10000 < v < 0x7FFE0000:
                print(f"  +{off:#x}: ptr {v:#x}")
