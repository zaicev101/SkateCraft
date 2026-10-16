"""Capture only the WoW window (never the whole desktop): python -I tools/wowshot.py out.png"""
import ctypes, ctypes.wintypes as wt, sys
from PIL import Image
u32 = ctypes.WinDLL("user32"); g32 = ctypes.WinDLL("gdi32")
hwnd = None
def cb(h, _):
    global hwnd
    buf = ctypes.create_unicode_buffer(64); u32.GetWindowTextW(h, buf, 64)
    pid = wt.DWORD(); u32.GetWindowThreadProcessId(h, ctypes.byref(pid))
    if buf.value == "World of Warcraft" and u32.IsWindowVisible(h):
        hwnd = h
    return True
u32.EnumWindows(ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)(cb), 0)
r = wt.RECT(); u32.GetClientRect(hwnd, ctypes.byref(r)); w, h = r.right, r.bottom
hdc = u32.GetDC(hwnd); mdc = g32.CreateCompatibleDC(hdc); bmp = g32.CreateCompatibleBitmap(hdc, w, h)
g32.SelectObject(mdc, bmp)
ok = u32.PrintWindow(hwnd, mdc, 3)  # PW_CLIENTONLY | PW_RENDERFULLCONTENT
class BI(ctypes.Structure):
    _fields_ = [("s", wt.DWORD), ("w", wt.LONG), ("h", wt.LONG), ("p", wt.WORD), ("b", wt.WORD), ("c", wt.DWORD), ("si", wt.DWORD), ("x", wt.LONG), ("y", wt.LONG), ("u", wt.DWORD), ("i", wt.DWORD)]
bi = BI(ctypes.sizeof(BI), w, -h, 1, 32, 0, 0, 0, 0, 0, 0)
buf = ctypes.create_string_buffer(w * h * 4)
g32.GetDIBits(mdc, bmp, 0, h, buf, ctypes.byref(bi), 0)
im = Image.frombuffer("RGBA", (w, h), buf, "raw", "BGRA", 0, 1).convert("RGB")
im.thumbnail((1280, 720)); im.save(sys.argv[1])
print("printwindow", ok, w, h)
