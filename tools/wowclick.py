"""SendInput-based click: python -I tools/wowclick.py X Y   (client coords of the WoW window, 2048x1152)"""
import ctypes, ctypes.wintypes as wt, sys, time
u = ctypes.WinDLL("user32")
try: ctypes.WinDLL("shcore").SetProcessDpiAwareness(2)
except Exception: pass
hwnd = None
def cb(h, _):
    global hwnd
    b = ctypes.create_unicode_buffer(64); u.GetWindowTextW(h, b, 64)
    if b.value == "World of Warcraft" and u.IsWindowVisible(h): hwnd = h
    return True
u.EnumWindows(ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)(cb), 0)
u.ShowWindow(hwnd, 9); u.SetForegroundWindow(hwnd); time.sleep(0.5)
pt = wt.POINT(0, 0); u.ClientToScreen(hwnd, ctypes.byref(pt))
x, y = pt.x + int(sys.argv[1]), pt.y + int(sys.argv[2])
vw, vh = u.GetSystemMetrics(78), u.GetSystemMetrics(79); vx, vy = u.GetSystemMetrics(76), u.GetSystemMetrics(77)
class MI(ctypes.Structure): _fields_ = [("dx", wt.LONG), ("dy", wt.LONG), ("d", wt.DWORD), ("f", wt.DWORD), ("t", wt.DWORD), ("e", ctypes.c_size_t)]
class IN(ctypes.Structure):
    class U(ctypes.Union): _fields_ = [("mi", MI)]
    _anonymous_ = ("u",); _fields_ = [("type", wt.DWORD), ("u", U)]
def send(flags, dx=0, dy=0): u.SendInput(1, ctypes.byref(IN(0, IN.U(MI(dx, dy, 0, flags, 0, 0)))), ctypes.sizeof(IN))
nx, ny = int((x - vx) * 65535 / (vw - 1)), int((y - vy) * 65535 / (vh - 1))
send(0x8001 | 0x4000, nx, ny); time.sleep(0.2); send(0x8001 | 0x4000, nx, ny); time.sleep(0.2)
send(0x0002); time.sleep(0.1); send(0x0004)
print("clicked at", x, y, "virtual", vx, vy, vw, vh)
