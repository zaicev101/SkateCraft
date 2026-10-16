"""Frame-to-frame difference of the static background under different effect switches (see skatecraft_off.txt)."""
import sys, time, struct, subprocess, math, os
sys.path.insert(0, "tools")
from PIL import Image, ImageChops
from wowmem import *
G = r"C:\WOW_335_ONPREM\WOW_CLIENT_335\3.3.5a"
import ctypes, ctypes.wintypes as wt
u = ctypes.WinDLL("user32"); g32 = ctypes.WinDLL("gdi32")
hwnd = None
def cb(h, _):
    global hwnd
    b = ctypes.create_unicode_buffer(64); u.GetWindowTextW(h, b, 64)
    if b.value == "World of Warcraft" and u.IsWindowVisible(h): hwnd = h
    return True
u.EnumWindows(ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)(cb), 0)
r = wt.RECT(); u.GetClientRect(hwnd, ctypes.byref(r)); w, h = r.right, r.bottom
class BI(ctypes.Structure):
    _fields_ = [("s", wt.DWORD), ("w", wt.LONG), ("h", wt.LONG), ("p", wt.WORD), ("b", wt.WORD), ("c", wt.DWORD), ("si", wt.DWORD), ("x", wt.LONG), ("y", wt.LONG), ("u", wt.DWORD), ("i", wt.DWORD)]
def grab():
    hdc = u.GetDC(hwnd); mdc = g32.CreateCompatibleDC(hdc); bmp = g32.CreateCompatibleBitmap(hdc, w, h)
    g32.SelectObject(mdc, bmp); u.PrintWindow(hwnd, mdc, 3)
    bi = BI(ctypes.sizeof(BI), w, -h, 1, 32, 0, 0, 0, 0, 0, 0); buf = ctypes.create_string_buffer(w * h * 4)
    g32.GetDIBits(mdc, bmp, 0, h, buf, ctypes.byref(bi), 0)
    g32.DeleteObject(bmp); g32.DeleteDC(mdc); u.ReleaseDC(hwnd, hdc)
    return Image.frombuffer("RGBA", (w, h), buf, "raw", "BGRA", 0, 1).convert("L").resize((w // 4, h // 4))
p = Proc(pid_of_wow()); cam = p.u32(p.u32(0xB7436C) + 0x7E20)
def campos(): return struct.unpack("<3f", p.read(cam + 8, 12))
def run(label, off):
    open(os.path.join(G, "skatecraft_off.txt"), "w").write(off)
    time.sleep(2.5)
    c0 = campos(); t0 = time.time(); frames = []
    while len(frames) < 24:
        frames.append(grab())
    dt = time.time() - t0; c1 = campos()
    W, H = frames[0].size; box = (int(W * .05), int(H * .08), int(W * .95), int(H * .40))
    ds = []
    for a, b in zip(frames, frames[1:]):
        d = ImageChops.difference(a.crop(box), b.crop(box)); hi = d.histogram()
        ds.append(sum(i * c for i, c in enumerate(hi)) / sum(hi))
    print("%-22s capture %.1f fps | camera moved %.3f yd | mean frame diff %.2f (max %.2f)" % (label, len(frames) / dt, math.dist(c0, c1), sum(ds) / len(ds), max(ds)))
subprocess.run([sys.executable, "-I", "tools/wowinput.py", "key", "0x10"], check=True, capture_output=True)
for label, off in [("all effects on", ""), ("camera off", "cam"), ("board off", "board"), ("bones off", "bones"), ("cam+board+bones off", "cam board bones")]:
    run(label, off)
open(os.path.join(G, "skatecraft_off.txt"), "w").write("")
