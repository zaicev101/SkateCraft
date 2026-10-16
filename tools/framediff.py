import sys, subprocess, time
from PIL import Image, ImageChops
label=sys.argv[1]
frames=[]
for k in range(12):
    subprocess.run([sys.executable,"-I","tools/wowshot.py","d%d.png"%k],capture_output=True)
    frames.append(Image.open("d%d.png"%k).convert("L"))
    time.sleep(0.05)
W,H=frames[0].size
# background strip: top quarter (sky/buildings), away from the character
box=(int(W*0.05),int(H*0.1),int(W*0.95),int(H*0.45))
import statistics
diffs=[]
for a,b in zip(frames,frames[1:]):
    d=ImageChops.difference(a.crop(box),b.crop(box))
    h=d.histogram(); tot=sum(h); mean=sum(i*c for i,c in enumerate(h))/tot; big=sum(h[24:])/tot
    diffs.append((mean,big))
print(label,"mean abs diff per frame pair: %.2f  fraction of pixels changed >24: %.4f"%(statistics.mean(x[0] for x in diffs),statistics.mean(x[1] for x in diffs)))
