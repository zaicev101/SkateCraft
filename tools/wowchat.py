"""Send a chat/slash command to the WoW window by pasting it (keyboard-layout independent): python -I tools/wowchat.py "/reload" """
import ctypes, subprocess, sys, time
u = ctypes.WinDLL("user32"); k = ctypes.WinDLL("kernel32")
k.GlobalAlloc.restype = ctypes.c_void_p; k.GlobalLock.restype = ctypes.c_void_p
k.GlobalLock.argtypes = [ctypes.c_void_p]; k.GlobalUnlock.argtypes = [ctypes.c_void_p]
u.SetClipboardData.argtypes = [ctypes.c_uint, ctypes.c_void_p]
def set_clipboard(text):
    data = (text + "\0").encode("utf-16-le")
    h = k.GlobalAlloc(0x42, len(data)); p = k.GlobalLock(h); ctypes.memmove(p, data, len(data)); k.GlobalUnlock(h)
    u.OpenClipboard(0); u.EmptyClipboard(); u.SetClipboardData(13, h); u.CloseClipboard()
here = __file__.rsplit("\\", 1)[0] if "\\" in __file__ else "."
inp = [sys.executable, "-I", here + "\wowinput.py"]
set_clipboard(sys.argv[1])
subprocess.run(inp + ["key", "0x0D"], check=True)  # Enter opens the chat box
time.sleep(0.4)
u.keybd_event(0x11, 0, 0, 0); u.keybd_event(0x56, 0, 0, 0); time.sleep(0.05)
u.keybd_event(0x56, 0, 2, 0); u.keybd_event(0x11, 0, 2, 0); time.sleep(0.3)
subprocess.run(inp + ["key", "0x0D"], check=True)
print("sent", sys.argv[1])
