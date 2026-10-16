"""Minimal input to the WoW window for tests: python -I tools/wowinput.py click X Y | wheel N | key VK ... (client coords of the 2048x1152 window)"""
import ctypes, ctypes.wintypes as wt, sys, time
u = ctypes.WinDLL("user32")
hwnd = None
def cb(h, _):
    global hwnd
    b = ctypes.create_unicode_buffer(64); u.GetWindowTextW(h, b, 64)
    if b.value == "World of Warcraft" and u.IsWindowVisible(h): hwnd = h
    return True
u.EnumWindows(ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)(cb), 0)
u.ShowWindow(hwnd, 9); u.SetForegroundWindow(hwnd); time.sleep(0.4)
pt = wt.POINT(0, 0); u.ClientToScreen(hwnd, ctypes.byref(pt))
def move(x, y): u.SetCursorPos(pt.x + x, pt.y + y)
def mouse(flags, data=0): u.mouse_event(flags, 0, 0, data, 0)
cmd = sys.argv[1]
if cmd == "click":
    for k in range(3):
        move(int(sys.argv[2]) + k, int(sys.argv[3])); time.sleep(0.15)
    mouse(2); time.sleep(0.12); mouse(4)
elif cmd == "wheel":
    move(1024, 576)
    for _ in range(abs(int(sys.argv[2]))):
        mouse(0x800, (120 if int(sys.argv[2]) > 0 else -120) & 0xFFFFFFFF); time.sleep(0.05)
elif cmd == "key":
    for k in sys.argv[2:]:
        u.keybd_event(int(k, 0), 0, 0, 0); time.sleep(0.05); u.keybd_event(int(k, 0), 0, 2, 0)
if cmd == "type":
    for ch in sys.argv[2]:
        v = u.VkKeyScanW(ord(ch)); vk = v & 0xFF; sh = (v >> 8) & 1
        if sh: u.keybd_event(0x10, 0, 0, 0)
        u.keybd_event(vk, 0, 0, 0); time.sleep(0.02); u.keybd_event(vk, 0, 2, 0)
        if sh: u.keybd_event(0x10, 0, 2, 0)
        time.sleep(0.02)
print("ok")
