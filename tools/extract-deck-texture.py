"""Extract the board's wood texture from the player's own WoW 3.3.5a client (never commit the output: Blizzard content).
python -I tools/extract-deck-texture.py "C:\...\3.3.5a\Data" [WOODPLANK04]  ->  crates/host-core/assets/deck.rgba
Needs: pip install mpyq pillow"""
import io, struct, sys, os
import mpyq
from PIL import Image
data = sys.argv[1]
name = (sys.argv[2] if len(sys.argv) > 2 else "WOODPLANK04").upper()
path = f"DUNGEONS\TEXTURES\WOOD\{name}.BLP"
blob = None
for mpq in ("common.mpq", "common-2.mpq", "expansion.mpq", "lichking.mpq"):
    a = mpyq.MPQArchive(os.path.join(data, mpq))
    try:
        blob = a.read_file(path)
    except Exception:
        blob = None
    if blob:
        break
if not blob:
    sys.exit("texture not found: " + path)
im = Image.open(io.BytesIO(blob)).convert("RGBA").resize((128, 128))
out = os.path.join(os.path.dirname(__file__), "..", "crates", "host-core", "assets", "deck.rgba")
with open(out, "wb") as f:
    f.write(struct.pack("<II", 128, 128) + im.tobytes())
im.save(os.path.join(os.path.dirname(__file__), "..", "deck-preview.png"))
print("wrote", out, im.size)
