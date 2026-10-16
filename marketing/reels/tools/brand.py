"""The reels' look: the website's, read from its own files, never typed in by eye.

  website/app/globals.css     the `.dark` token block (the site is dark by default: `<html class="dark">`),
                              `--radius`, and the `.gradient-text` and `.aurora` rules
  website/public/logo.svg     the mark's two fills (its shapes are drawn in `render.py`)
  Inter                       the site's font (`app/layout.tsx`, next/font). ffmpeg and Pillow need a TTF/OTF,
                              so the files are Debian/Ubuntu's `fonts-inter` 4.0+ds-1 (SIL OFL, the licence text is
                              /usr/share/doc/fonts-inter/copyright), the same ones
                              apps/desktop/e2e-native/make-demo-videos.mjs sets the demos' captions in.
                              No font file is added to the repo. EDYTLAB_REEL_FONT_DIR points at another copy
                              (a folder holding Inter-Medium.otf, Inter-SemiBold.otf, Inter-Bold.otf, or the same
                              names as .ttf from Google Fonts).

Colours are (r, g, b) tuples. `check()` runs at import: it fails the render if a text colour on its own fill is
under the 4.5:1 contrast the reels promise, so a palette change on the site cannot slip an unreadable caption in.
"""
import os
import random
import re
from pathlib import Path
from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

REPO_ROOT = Path(__file__).resolve().parents[3]
SITE = REPO_ROOT / "website"
_CSS = (SITE / "app" / "globals.css").read_text()
_LOGO = (SITE / "public" / "logo.svg").read_text()


def hsl(h, s, l):
    """CSS hsl() (h in degrees, s and l in percent) to an (r, g, b) tuple of 0..255."""
    s, l = s / 100, l / 100
    c = (1 - abs(2 * l - 1)) * s
    hp = (h % 360) / 60
    x = c * (1 - abs(hp % 2 - 1))
    r, g, b = [(c, x, 0), (x, c, 0), (0, c, x), (0, x, c), (x, 0, c), (c, 0, x)][int(hp) % 6]
    m = l - c / 2
    return tuple(int((v + m) * 255 + 0.5) for v in (r, g, b))


def _rule(selector):
    """The body of the first `selector { ... }` rule of globals.css, braces matched."""
    start = _CSS.index(selector + " {")
    open_ = _CSS.index("{", start)
    depth = 0
    for i in range(open_, len(_CSS)):
        depth += {"{": 1, "}": -1}.get(_CSS[i], 0)
        if depth == 0:
            return _CSS[open_ + 1:i]
    raise ValueError(f"unbalanced braces after {selector}")


_HSL = r"hsl\(\s*([\d.]+)\s+([\d.]+)%\s+([\d.]+)%\s*\)"


def _token(block, name):
    m = re.search(rf"--{name}:\s*([\d.]+)\s+([\d.]+)%\s+([\d.]+)%", block)
    if not m:
        raise ValueError(f"--{name} not found in the site's .dark block")
    return hsl(*map(float, m.groups()))


def _hsls(text):
    return [hsl(*map(float, m)) for m in re.findall(_HSL, text)]


# The site's dark --card, --foreground and --primary (website/app/globals.css): the colours apps/desktop/e2e-native/make-demo-videos.mjs burns its captions in.
_DARK = _rule(".dark")
BACKGROUND = _token(_DARK, "background")          # the page
CARD = _token(_DARK, "card")                      # cards, and the demos' burned-in captions
FOREGROUND = _token(_DARK, "foreground")          # body text
PRIMARY = _token(_DARK, "primary")                # the violet accent
MUTED_FG = _token(_DARK, "muted-foreground")      # secondary text
BORDER = _token(_DARK, "border")
RADIUS_PX = round(float(re.search(r"--radius:\s*([\d.]+)rem", _CSS).group(1)) * 16)   # the site's --radius, in px at 1x
GRADIENT = _hsls(_rule(".gradient-text"))[:2]     # the headline gradient, light violet to magenta-violet
_aurora = _CSS[_CSS.index(".aurora::before {"):]
AURORA = _hsls(_aurora)[:2]                       # the two washes behind a section (violet, magenta)
AURORA_OPACITY = float(re.search(r"opacity:\s*([\d.]+)", _rule(".aurora::before,\n  .aurora::after")).group(1))

def _logo_colours():
    fills = re.findall(r'(?:fill|stroke)="#([0-9A-Fa-f]{6})"', _LOGO)
    uniq = list(dict.fromkeys(fills))
    if len(uniq) != 2:
        raise ValueError(f"expected two colours in website/public/logo.svg, found {uniq}")
    return tuple(tuple(int(v[i:i + 2], 16) for i in (0, 2, 4)) for v in uniq)


LOGO_BARS, LOGO_LINE = _logo_colours()


# ------------------------------------------------------------------ contrast
def _lin(c):
    c /= 255
    return c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4


def luminance(rgb):
    r, g, b = (_lin(c) for c in rgb)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast(a, b):
    """WCAG 2 contrast ratio of two (r, g, b) colours."""
    la, lb = sorted((luminance(a), luminance(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


MIN_CONTRAST = 4.5


def check():
    """Every text colour the reels use, on the fill it is drawn on."""
    pairs = {
        "caption text on card": (FOREGROUND, CARD),
        "caption accent on card": (PRIMARY, CARD),
        "pill text on card": (FOREGROUND, CARD),
        "note on background": (MUTED_FG, BACKGROUND),
        "title on background": (FOREGROUND, BACKGROUND),
        "gradient start on background": (GRADIENT[0], BACKGROUND),
        "gradient end on background": (GRADIENT[1], BACKGROUND),
        "url on background": (PRIMARY, BACKGROUND),
    }
    for name, (fg, bg) in pairs.items():
        ratio = contrast(fg, bg)
        if ratio < MIN_CONTRAST:
            raise ValueError(f"{name}: contrast {ratio:.2f}:1 is under {MIN_CONTRAST}:1")
    return {name: round(contrast(fg, bg), 1) for name, (fg, bg) in pairs.items()}


# ---------------------------------------------------------------------- fonts
FONT_DIR = os.environ.get("EDYTLAB_REEL_FONT_DIR") or "/usr/share/fonts/opentype/inter"


def font(weight, size):
    """Inter at `weight` ("Medium", "SemiBold" or "Bold") and `size` px."""
    for ext in ("otf", "ttf"):
        p = Path(FONT_DIR) / f"Inter-{weight}.{ext}"
        if p.exists():
            return ImageFont.truetype(str(p), size)
    raise FileNotFoundError(f"no Inter-{weight}.otf/.ttf in {FONT_DIR}: install fonts-inter, or set EDYTLAB_REEL_FONT_DIR")


# ----------------------------------------------------------- shared drawing
# Used by the reels (render.py) and the launch video (marketing/launch-video/tools/engine.py), so both are drawn
# from one place: the site's logo, its `.aurora` washes behind a section and the film grain.
def rgba(c, a=255):
    return tuple(c) + (a,)


def wash(bg, shape, colour, opacity, blur):
    """Paste `colour` onto `bg` (an RGB image, in place) through a blurred mask: `shape(draw)` fills the mask with 255 and
    `opacity` (0..1) scales it."""
    m = Image.new("L", bg.size, 0)
    shape(ImageDraw.Draw(m))
    m = m.filter(ImageFilter.GaussianBlur(blur)).point(lambda v: min(255, int(v * opacity)))
    bg.paste(Image.new("RGB", bg.size, colour), (0, 0), m)


def aurora_wash(bg, strength=1.0):
    """The site's `.aurora` (a violet and a magenta wash at opposite corners, `.aurora` in globals.css) on `bg`, in place.
    `strength` 1 is the site's own opacity (0.16). The two blobs are 1120 px across, at the top-left and bottom-right corners."""
    w, h = bg.size
    wash(bg, lambda d: d.ellipse([-440, -300, 680, 820], fill=255), AURORA[0], AURORA_OPACITY * strength, 150)
    wash(bg, lambda d: d.ellipse([w - 650, h - 740, w + 470, h + 380], fill=255), AURORA[1], AURORA_OPACITY * strength, 150)


def add_grain(bg, grain, seed=2026):
    """`bg` plus a little seeded noise of std. dev. `grain` (8-bit levels), as the site's film grain: a steep wash bands once it
    is encoded. Seeded, so a rebuild gives the same bytes."""
    w, h = bg.size
    width = grain * 12 ** 0.5   # a uniform spread of this width has std. dev. `grain`
    noise = Image.frombytes("L", (w, h), random.Random(seed).randbytes(w * h)).point(lambda v: 128 + round((v / 255 - 0.5) * width))
    return ImageChops.add(bg, noise.convert("RGB"), 1.0, -128)


def logo_mark(size):
    """website/public/logo.svg drawn at `size` px: three level bars and two rising strokes (its shapes, in
    the viewBox of components/motion/logo.tsx). Drawn at 4x and scaled down for smooth edges."""
    ss = 4
    px = size * ss
    k = px / 57.29
    ox, oy = 3.854, 3.354

    def P(x, y):
        return ((x - ox) * k, (y - oy) * k)

    im = Image.new("RGBA", (px, px), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    for x, y, h in ((5, 26, 12), (14, 20, 24), (23, 25, 14)):
        d.rounded_rectangle([*P(x, y), *P(x + 6, y + h)], radius=3 * k, fill=rgba(LOGO_BARS))
    for pts, col in (([(34, 26), (41, 18), (48, 23), (57, 12)], LOGO_BARS), ([(34, 38), (41, 46), (48, 41), (57, 52)], LOGO_LINE)):
        xy = [P(*p) for p in pts]
        d.line(xy, fill=rgba(col), width=round(6 * k), joint="curve")
        for x, y in xy:
            d.ellipse([x - 3 * k, y - 3 * k, x + 3 * k, y + 3 * k], fill=rgba(col))
    return im.resize((size, size), Image.LANCZOS)


CONTRASTS = check()
