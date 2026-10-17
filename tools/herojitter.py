"""Hero jitter from recorded frames: centroid of yellow trousers pixels per frame; second-difference rms (px) vs path length."""
import os, sys, math
from PIL import Image
D = sys.argv[1]
names = sorted(os.listdir(D))
pts = []
for n in names:
    im = Image.open(os.path.join(D, n)).convert("RGB").resize((240, 135))
    W, H = im.size; px = im.load(); sx = sy = c = 0
    for y in range(H):
        for x in range(W):
            r, g, b = px[x, y]
            if r > 150 and g > 120 and b < 70 and r - b > 100:   # trousers yellow
                sx += x; sy += y; c += 1
    if c > 8:
        pts.append((int(n[:-4]), sx / c, sy / c, c))
print(os.path.basename(D), "frames with the hero:", len(pts), "of", len(names))
if len(pts) > 10:
    dt = [(pts[i + 1][0] - pts[i][0]) for i in range(len(pts) - 1)]
    d2 = [math.hypot(pts[i + 1][1] - 2 * pts[i][1] + pts[i - 1][1], pts[i + 1][2] - 2 * pts[i][2] + pts[i - 1][2]) for i in range(1, len(pts) - 1)]
    size = [p[3] for p in pts]
    d2s = [abs(size[i + 1] - 2 * size[i] + size[i - 1]) / max(1, size[i]) for i in range(1, len(size) - 1)]
    print("hero position second-difference rms %.2f px (240x135 image); size change rms %.3f" % (math.sqrt(sum(x * x for x in d2) / len(d2)), math.sqrt(sum(x * x for x in d2s) / len(d2s))))
