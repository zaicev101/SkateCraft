"""Capture consecutive frames of the WoW window in a burst (~30 fps) and write a contact sheet: python -I tools/burst.py out.png [n]"""
import sys, time, subprocess, ctypes, ctypes.wintypes as wt
from PIL import Image
n = int(sys.argv[2]) if len(sys.argv) > 2 else 12
u = ctypes.WinDLL("user32"); g32 = ctypes.WinDLL("gdi32")
hwnd = None
def cb(h, _):
    global hwnd
    b = ctypes.create_unicode_buffer(64); u.GetWindowTextW(h, b, 64)
    if b.value == "World of Warcraft" and u.IsWindowVisible(h): hwnd = h
    return True
u.EnumWindows(ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)(cb), 0)
subprocess.run([sys.executable, "-I", __file__.rsplit("\\", 1)[0] + "\wowinput.py", "key", "0x10"], capture_output=True)
time.sleep(0.8)
r = wt.RECT(); u.GetClientRect(hwnd, ctypes.byref(r)); w, h = r.right, r.bottom
class BI(ctypes.Structure):
    _fields_ = [("s", wt.DWORD), ("w", wt.LONG), ("h", wt.LONG), ("p", wt.WORD), ("b", wt.WORD), ("c", wt.DWORD), ("si", wt.DWORD), ("x", wt.LONG), ("y", wt.LONG), ("u", wt.DWORD), ("i", wt.DWORD)]
def grab():
    hdc = u.GetDC(hwnd); mdc = g32.CreateCompatibleDC(hdc); bmp = g32.CreateCompatibleBitmap(hdc, w, h)
    g32.SelectObject(mdc, bmp); u.PrintWindow(hwnd, mdc, 3)
    bi = BI(ctypes.sizeof(BI), w, -h, 1, 32, 0, 0, 0, 0, 0, 0); buf = ctypes.create_string_buffer(w * h * 4)
    g32.GetDIBits(mdc, bmp, 0, h, buf, ctypes.byref(bi), 0)
    g32.DeleteObject(bmp); g32.DeleteDC(mdc); u.ReleaseDC(hwnd, hdc)
    return Image.frombuffer("RGBA", (w, h), buf, "raw", "BGRA", 0, 1).convert("RGB")
t0 = time.time(); frames = [grab() for _ in range(n)]; dt = time.time() - t0
tw, th = 480, 270
cols = 4; rows = (n + cols - 1) // cols
sheet = Image.new("RGB", (tw * cols, th * rows))
for i, f in enumerate(frames):
    W, H = f.size
    sheet.paste(f.crop((int(W * .15), int(H * .15), int(W * .85), int(H * .95))).resize((tw, th)), ((i % cols) * tw, (i // cols) * th))
sheet.save(sys.argv[1]); print("captured %d frames in %.2fs (%.0f fps)" % (n, dt, n / dt))
