#!/usr/bin/env python3
"""Join a light and a dark screenshot along the diagonal, the way the
store and README screenshots show both themes: light above the line
from the top-right to the bottom-left corner, dark below it.

    compose-screenshot.py LIGHT.png DARK.png OUT.png [MARGIN]

Both inputs must be the same size (the same window or screen in both
themes). With MARGIN (Linux window shots, 25 for the README ones), the
result is cut to the opaque window plus MARGIN px of its shadow on
each side. Needs Pillow.
"""
import sys

from PIL import Image, ImageDraw


def main() -> None:
    if len(sys.argv) not in (4, 5):
        sys.exit(__doc__)
    light, dark = (Image.open(p).convert("RGBA") for p in sys.argv[1:3])
    if light.size != dark.size:
        sys.exit(f"sizes differ: {light.size} vs {dark.size}")
    if len(sys.argv) == 5:
        m = int(sys.argv[4])
        left, top, right, bottom = light.getchannel("A").point(lambda a: 255 if a == 255 else 0).getbbox()
        box = (left - m, top - m, right + m, bottom + m)
        light, dark = light.crop(box), dark.crop(box)
    w, h = light.size
    mask = Image.new("L", (w, h), 0)
    ImageDraw.Draw(mask).polygon([(w, 0), (w, h), (0, h)], fill=255)
    out = Image.composite(dark, light, mask)
    # Phone screenshots carry no alpha; keep them RGB like the old ones.
    if light.getextrema()[3] == (255, 255) and dark.getextrema()[3] == (255, 255):
        out = out.convert("RGB")
    out.save(sys.argv[3], optimize=True)


if __name__ == "__main__":
    main()
