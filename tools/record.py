"""Record the WoW window as JPEG frames (~30 fps, half size) for N seconds: python -I tools/record.py outdir seconds"""
import sys, os, time, ctypes, ctypes.wintypes as wt
from PIL import Image
out = sys.argv[1]; secs = float(sys.argv[2]); os.makedirs(out, exist_ok=True)
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
t0 = time.time(); n = 0
while time.time() - t0 < secs:
    hdc = u.GetDC(hwnd); mdc = g32.CreateCompatibleDC(hdc); bmp = g32.CreateCompatibleBitmap(hdc, w, h)
    g32.SelectObject(mdc, bmp); u.PrintWindow(hwnd, mdc, 3)
    bi = BI(ctypes.sizeof(BI), w, -h, 1, 32, 0, 0, 0, 0, 0, 0); buf = ctypes.create_string_buffer(w * h * 4)
    g32.GetDIBits(mdc, bmp, 0, h, buf, ctypes.byref(bi), 0)
    g32.DeleteObject(bmp); g32.DeleteDC(mdc); u.ReleaseDC(hwnd, hdc)
    stamp = int((time.time() - t0) * 1000)
    Image.frombuffer("RGBA", (w, h), buf, "raw", "BGRA", 0, 1).convert("RGB").resize((w // 2, h // 2)).save(os.path.join(out, "%06d.jpg" % stamp), quality=60)
    n += 1
print("recorded", n, "frames in %.1fs" % (time.time() - t0))
