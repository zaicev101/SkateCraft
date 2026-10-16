"""Temporal flicker while moving: per-frame luminance statistics of the world (no UI), second-difference RMS."""
import sys, time, math, ctypes, ctypes.wintypes as wt
from PIL import Image
exec(open("tools/flicker.py", encoding="utf-8").read().split("p = Proc")[0])  # reuse grab()
def stats(n=60):
    lum=[]; edge=[]
    for _ in range(n):
        im = grab(); W, H = im.size
        box = im.crop((int(W*.05), int(H*.1), int(W*.95), int(H*.55)))
        px = list(box.getdata()); lum.append(sum(px)/len(px))
        w2, h2 = box.size
        e = 0
        for y in range(0, h2 - 1, 3):
            row = px[y*w2:(y+1)*w2]
            e += sum(abs(row[i+1]-row[i]) for i in range(0, w2-1, 3))
        edge.append(e)
    d2 = lambda v: [v[i+1]-2*v[i]+v[i-1] for i in range(1, len(v)-1)]
    rms = lambda v: math.sqrt(sum(x*x for x in v)/len(v))
    return rms(d2(lum)), rms(d2(edge))/ (sum(edge)/len(edge))
label = sys.argv[1]
l, e = stats()
print("%-28s luminance jitter %.3f | texture-energy jitter %.4f" % (label, l, e))
