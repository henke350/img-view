"""Draw the img-view app icon (an eye) and save it as assets/eye.ico.

Run from the repo root:  python tools/make_icon.py
Needs Pillow (pip install pillow). The .ico holds every size Windows asks for,
each drawn large and scaled down so the edges stay smooth.
"""

from pathlib import Path

from PIL import Image, ImageChops, ImageDraw

SIZES = [16, 24, 32, 48, 64, 128, 256]
BIG = 1024  # draw at this size, then scale down

OUTLINE = (31, 41, 55, 255)  # dark slate
SCLERA = (255, 255, 255, 255)
IRIS = (47, 128, 237, 255)  # blue
IRIS_RING = (22, 78, 160, 255)
PUPIL = (15, 18, 25, 255)
HIGHLIGHT = (255, 255, 255, 235)


def lens_mask(size: int, cx: float, cy: float, a: float, b: float) -> Image.Image:
    """Almond (lens) shape: the overlap of two circles, half-width a, half-height b."""
    r = (a * a + b * b) / (2 * b)  # radius of each lid's arc
    off = r - b  # how far each circle's centre sits from the eye centre
    upper = Image.new("L", (size, size), 0)
    lower = Image.new("L", (size, size), 0)
    ImageDraw.Draw(upper).ellipse((cx - r, cy + off - r, cx + r, cy + off + r), fill=255)
    ImageDraw.Draw(lower).ellipse((cx - r, cy - off - r, cx + r, cy - off + r), fill=255)
    return ImageChops.multiply(upper, lower)


def draw_eye(size: int) -> Image.Image:
    s = size
    cx = cy = s / 2
    a, b = 0.46 * s, 0.27 * s  # outer almond
    line = 0.055 * s  # outline thickness

    outer = lens_mask(s, cx, cy, a, b)
    inner = lens_mask(s, cx, cy, a - line * 1.6, b - line)

    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    img.paste(OUTLINE, (0, 0), outer)
    img.paste(SCLERA, (0, 0), inner)

    # Iris, pupil and highlight, all clipped to the white of the eye.
    detail = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(detail)
    ri = 0.19 * s
    d.ellipse((cx - ri, cy - ri, cx + ri, cy + ri), fill=IRIS_RING)
    ri2 = ri * 0.84
    d.ellipse((cx - ri2, cy - ri2, cx + ri2, cy + ri2), fill=IRIS)
    rp = ri * 0.48
    d.ellipse((cx - rp, cy - rp, cx + rp, cy + rp), fill=PUPIL)
    rh = ri * 0.2
    hx, hy = cx + ri * 0.32, cy - ri * 0.36
    d.ellipse((hx - rh, hy - rh, hx + rh, hy + rh), fill=HIGHLIGHT)

    clip = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    clip.paste(detail, (0, 0), ImageChops.multiply(detail.getchannel("A"), inner))
    return Image.alpha_composite(img, clip)


def main() -> None:
    out = Path(__file__).resolve().parent.parent / "assets" / "eye.ico"
    big = draw_eye(BIG)
    images = [big.resize((n, n), Image.LANCZOS) for n in SIZES]
    images[-1].save(out, format="ICO", sizes=[(n, n) for n in SIZES], append_images=images[:-1])
    big.resize((256, 256), Image.LANCZOS).save(out.with_suffix(".png"))
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
