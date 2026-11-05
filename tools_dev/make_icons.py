"""Draws every Aura Portfolio icon from one SVG master, assets/icon.svg:

    python tools_dev/make_icons.py

  assets/icon.ico          16/24/32/48/64/256 - the Windows program (build.rs embeds it) and the setup
  assets/icon.icns         16..1024           - the Mac app (release.yml, src/install.rs)
  assets/icon-<size>.png   16..256            - the window icon and in-app logo (icon-256.png, decoded at runtime)
                                                and the Linux app-menu icon
  assets/wizard-large.bmp  164x314 (+ -200)   - the Windows setup's Welcome / Finished picture
  assets/wizard-small.bmp  55x55   (+ -200)   - the setup's corner picture

Needs only Pillow. It reads the small part of SVG the master uses: <linearGradient> (x1..y2 as fractions of the
shape's box, stops), <rect> (x, y, width, height, rx), <circle> and <path> with absolute M/L/C/Z, each filled with a
colour or url(#gradient). Every size is drawn 2-8x larger and averaged down, so small icons stay crisp. Run it again
after changing icon.svg and commit the results.
"""
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets"
ICO_SIZES = (16, 24, 32, 48, 64, 256)
PNG_SIZES = (16, 24, 32, 48, 64, 128, 256)
ICNS_SIZES = (16, 32, 64, 128, 256, 512, 1024)
NS = "{http://www.w3.org/2000/svg}"


def supersampling(size):
    return max(2, min(8, 2048 // size))  # (per side: 8x for small icons, less for big ones)


def colour(text):
    text = text.strip().lstrip("#")
    if len(text) == 3:
        text = "".join(c * 2 for c in text)
    return tuple(int(text[i:i + 2], 16) for i in (0, 2, 4))


def path_points(d, scale, steps=48):
    """The outline of an absolute M/L/C/Z path as polygons, curves flattened."""
    tokens = re.findall(r"[MLCZmlcz]|-?\d*\.?\d+(?:e-?\d+)?", d)
    polys, cur, pos, cmd, i = [], [], (0.0, 0.0), None, 0

    def num():
        nonlocal i
        i += 1
        return float(tokens[i - 1]) * scale

    while i < len(tokens):
        if tokens[i].isalpha():
            cmd = tokens[i]
            i += 1
            if cmd in "mlcz":
                sys.exit(f"icon.svg: only absolute path commands are read (found {cmd!r})")
            if cmd == "Z":
                if cur:
                    polys.append(cur)
                cur = []
                continue
        if cmd == "M":
            if cur:
                polys.append(cur)
            pos = (num(), num())
            cur = [pos]
            cmd = "L"
        elif cmd == "L":
            pos = (num(), num())
            cur.append(pos)
        elif cmd == "C":
            p1, p2, p3 = (num(), num()), (num(), num()), (num(), num())
            p0 = pos
            for s in range(1, steps + 1):
                t = s / steps
                u = 1 - t
                cur.append(tuple(u ** 3 * a + 3 * u * u * t * b + 3 * u * t * t * c + t ** 3 * e
                                 for a, b, c, e in zip(p0, p1, p2, p3)))
            pos = p3
        else:
            sys.exit(f"icon.svg: can't read path data near {tokens[i]!r}")
    if cur:
        polys.append(cur)
    return polys


def gradient(el, box, size):
    """An RGBA image of a linear gradient across `box` (x0, y0, x1, y1), over the whole canvas."""
    x0, y0, x1, y1 = box
    f = lambda k, default: float(el.get(k, default))  # noqa: E731
    ax, ay = x0 + f("x1", 0) * (x1 - x0), y0 + f("y1", 0) * (y1 - y0)
    bx, by = x0 + f("x2", 1) * (x1 - x0), y0 + f("y2", 0) * (y1 - y0)
    stops = sorted((float(s.get("offset", 0)), colour(s.get("stop-color", "#000"))) for s in el.iter(NS + "stop"))
    dx, dy = bx - ax, by - ay
    length = dx * dx + dy * dy or 1.0
    ramp_n = 1024
    ramp = []
    for k in range(ramp_n):
        t = k / (ramp_n - 1)
        lo = max((s for s in stops if s[0] <= t), default=stops[0])
        hi = min((s for s in stops if s[0] >= t), default=stops[-1])
        w = 0.0 if hi[0] == lo[0] else (t - lo[0]) / (hi[0] - lo[0])
        ramp.append(tuple(round(a + (b - a) * w) for a, b in zip(lo[1], hi[1])) + (255,))
    img = Image.new("RGBA", (size, size))
    img.putdata([ramp[min(ramp_n - 1, max(0, int(((x + .5 - ax) * dx + (y + .5 - ay) * dy) / length * (ramp_n - 1))))]
                 for y in range(size) for x in range(size)])
    return img


def draw(svg, size, skip_ids=()):
    """The SVG at `size` px (RGBA). `skip_ids` leaves out elements by id (("tile",) = the bars alone)."""
    root = ET.parse(svg).getroot()
    view = [float(v) for v in root.get("viewBox", "0 0 256 256").split()]
    ss = supersampling(size)
    big = size * ss
    scale = big / view[2]
    grads = {g.get("id"): g for g in root.iter(NS + "linearGradient")}
    canvas = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    for el in root.iter():
        tag = el.tag.replace(NS, "")
        if el.get("id") in skip_ids:
            continue
        if tag == "rect":
            x, y, w, h = (float(el.get(k, 0)) * scale for k in ("x", "y", "width", "height"))
            rx = float(el.get("rx", 0)) * scale
            box = (x, y, x + w, y + h)
            shape = lambda d, b=box, r=rx: d.rounded_rectangle(b, radius=r, fill=255)  # noqa: E731
        elif tag == "circle":
            cx, cy, r = (float(el.get(k)) * scale for k in ("cx", "cy", "r"))
            box = (cx - r, cy - r, cx + r, cy + r)
            shape = lambda d, b=box: d.ellipse(b, fill=255)  # noqa: E731
        elif tag == "path":
            polys = path_points(el.get("d"), scale)
            xs = [p[0] for poly in polys for p in poly]
            ys = [p[1] for poly in polys for p in poly]
            box = (min(xs), min(ys), max(xs), max(ys))
            shape = lambda d, ps=polys: [d.polygon(p, fill=255) for p in ps]  # noqa: E731
        else:
            continue
        mask = Image.new("L", (big, big), 0)
        shape(ImageDraw.Draw(mask))
        fill = el.get("fill", "#000")
        m = re.fullmatch(r"url\(#(.+)\)", fill)
        paint = gradient(grads[m.group(1)], box, big) if m else Image.new("RGBA", (big, big), colour(fill) + (255,))
        layer = Image.new("RGBA", (big, big), (0, 0, 0, 0))
        layer.paste(paint, mask=mask)
        canvas = Image.alpha_composite(canvas, layer)
    return canvas.reduce(ss)


def wizard_large(svg, w, h):
    """The setup's side picture (Welcome / Finished pages): the icon's gradient top to bottom, the white bars."""
    root = ET.parse(svg).getroot()
    stops = [colour(s.get("stop-color")) for s in root.iter(NS + "stop")]
    top, bottom = stops[0], stops[-1]
    img = Image.new("RGB", (w, h))
    img.putdata([tuple(round(a + (b - a) * y / (h - 1)) for a, b in zip(top, bottom)) for y in range(h) for _ in range(w)])
    s = round(w * 0.72)
    bars = draw(svg, s, skip_ids=("tile",))
    img.paste(bars, ((w - s) // 2, round(h * 0.40) - s // 2), bars)
    return img


def wizard_small(svg, s):
    """The setup's corner picture (the other pages): the icon on white."""
    img = Image.new("RGB", (s, s), (255, 255, 255))
    pad = round(s * 0.06)
    icon = draw(svg, s - 2 * pad)
    img.paste(icon, (pad, pad), icon)
    return img


def main():
    svg = ASSETS / "icon.svg"
    images = {s: draw(svg, s) for s in sorted(set(ICO_SIZES) | set(PNG_SIZES) | set(ICNS_SIZES))}
    for s in PNG_SIZES:
        images[s].save(ASSETS / f"icon-{s}.png", optimize=True)
    biggest = images[max(ICO_SIZES)]
    biggest.save(ASSETS / "icon.ico", sizes=[(s, s) for s in ICO_SIZES],
                 append_images=[images[s] for s in ICO_SIZES if s != max(ICO_SIZES)])
    images[max(ICNS_SIZES)].save(ASSETS / "icon.icns",
                                 append_images=[images[s] for s in ICNS_SIZES if s != max(ICNS_SIZES)])
    for scale in (1, 2):
        sfx = "" if scale == 1 else f"-{scale}00"
        wizard_large(svg, 164 * scale, 314 * scale).save(ASSETS / f"wizard-large{sfx}.bmp")
        wizard_small(svg, 55 * scale).save(ASSETS / f"wizard-small{sfx}.bmp")
    print(f"Wrote icon.ico ({', '.join(map(str, ICO_SIZES))}), icon.icns ({', '.join(map(str, ICNS_SIZES))}), "
          f"icon-<size>.png ({', '.join(map(str, PNG_SIZES))}) and the setup wizard's pictures in "
          f"{ASSETS.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
