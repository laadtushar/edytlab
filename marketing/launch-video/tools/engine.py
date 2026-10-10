#!/usr/bin/env python3
"""Code-driven motion engine for the edytlab launch videos.

PIL draws every frame (immediate mode: a frame is a pure function of time), the
real demo recordings are decoded by ffmpeg and composed inside a virtual camera,
and a second ffmpeg process encodes the result. Nothing here needs more than
Python 3 + Pillow + ffmpeg, so it runs offline.

Motion vocabulary (see research.md, section M):
  * easing  : Material 3 cubic-béziers (emphasized decelerate to enter, emphasized
              accelerate to exit), easeInOutCubic for the camera (Motion Canvas'
              default), a lightly damped spring for chips (Remotion's spring model)
  * camera  : crop rectangle + frame rectangle interpolated together, geometric
              in size (constant perceived zoom speed), plus a 1.2 % slow drift
  * type    : per-word kinetic reveal (rise + fade + blur-in, staggered)
  * callouts: ring that draws itself on, leader line, chip that springs in
"""
import io
import math
import os
import re
import subprocess
import sys

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

FPS = 30
SRC_W, SRC_H = 1280, 776
# The three demo recordings live in the repo: <repo>/website/public/demos. This file is <repo>/marketing/launch-video/tools/engine.py.
# Set EDYTLAB_DEMO_DIR to read them from somewhere else.
DEMO_DIR = os.environ.get("EDYTLAB_DEMO_DIR") or os.path.join(
    os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))), "website", "public", "demos")
FD ="/usr/share/fonts/truetype/liberation/"
F_SANS_B = FD + "LiberationSans-Bold.ttf"
F_SANS = FD + "LiberationSans-Regular.ttf"
F_SERIF_I = FD + "LiberationSerif-Italic.ttf"
F_SERIF = FD + "LiberationSerif-Regular.ttf"
F_MONO_B = FD + "LiberationMono-Bold.ttf"

ACCENT = (242, 140, 60)
WHITE = (255, 255, 255)
MUTED = (172, 178, 192)
PANEL = (13, 14, 20)


# ----------------------------------------------------------------- easing
def cubic_bezier(x1, y1, x2, y2):
    def bez(t, a, b):
        return 3 * a * (1 - t) ** 2 * t + 3 * b * (1 - t) * t ** 2 + t ** 3

    def f(x):
        if x <= 0:
            return 0.0
        if x >= 1:
            return 1.0
        lo, hi = 0.0, 1.0
        for _ in range(26):
            mid = (lo + hi) / 2
            if bez(mid, x1, x2) < x:
                lo = mid
            else:
                hi = mid
        return bez((lo + hi) / 2, y1, y2)

    return f


EASE_IN = cubic_bezier(0.05, 0.7, 0.1, 1.0)      # Material emphasized decelerate: things entering
EASE_OUT = cubic_bezier(0.3, 0.0, 0.8, 0.15)     # Material emphasized accelerate: things leaving
EASE_STD = cubic_bezier(0.2, 0.0, 0.0, 1.0)      # Material standard
EASE_CAM = cubic_bezier(0.645, 0.045, 0.355, 1)  # easeInOutCubic (Motion Canvas' default tween)


def spring(t, zeta=0.78, omega=16.0):
    """Damped spring step response, 0 -> 1 with ~2 % overshoot (Hooke's law, as in Remotion/Motion Canvas)."""
    if t <= 0:
        return 0.0
    wd = omega * math.sqrt(1 - zeta * zeta)
    return 1 - math.exp(-zeta * omega * t) * (math.cos(wd * t) + zeta * omega / wd * math.sin(wd * t))


def clamp(x, a=0.0, b=1.0):
    return a if x < a else b if x > b else x


def lerp(a, b, t):
    return a + (b - a) * t


def prog(T, t0, dur, ease=None):
    p = clamp((T - t0) / dur) if dur > 0 else (1.0 if T >= t0 else 0.0)
    return ease(p) if ease else p


# ------------------------------------------------------------------- fonts
_fonts = {}


def font(path, size):
    key = (path, int(round(size)))
    if key not in _fonts:
        _fonts[key] = ImageFont.truetype(path, key[1])
    return _fonts[key]


def text_w(f, s):
    return f.getlength(s)


# ----------------------------------------------------------------- layouts
LAYOUTS = {
    # 1:1 LinkedIn feed
    "sq": dict(W=1080, H=1080, title_box=(56, 44, 1024, 236), title_max=66, title_min=42,
               area=(44, 262, 1036, 948), max_scale=2.15, step_size=54, chips=(40, 244, 1040, 1010),
               chip_size=30, ring_w=4, radius=20, note_y=962, note_size=24,
               bar=(56, 1050, 1024), safe=(40, 40, 1040, 1040), list_size=42, end_wordmark=170),
    # 16:9 Reddit (and LinkedIn desktop)
    "wide": dict(W=1920, H=1080, title_box=(120, 40, 1800, 190), title_max=72, title_min=44,
                 area=(120, 214, 1800, 1010), max_scale=2.4, step_size=66, chips=(30, 200, 1890, 1050),
                 chip_size=34, ring_w=4, radius=22, note_y=1034, note_size=24,
                 bar=(120, 1066, 1800), safe=(40, 30, 1880, 1050), list_size=48, end_wordmark=210),
    # 9:16 Instagram Reels: Meta's own safe zone is 14 % top, 35 % bottom, 6 % each side
    "tall": dict(W=1080, H=1920, title_box=(65, 270, 1015, 490), title_max=78, title_min=48,
                 area=(65, 506, 1015, 1240), max_scale=2.3, step_size=62,
                 chips=(65, 272, 1015, 1246),
                 chip_size=33, ring_w=4, radius=20, note_y=1256, note_size=24,
                 bar=(65, 1330, 1015), safe=(65, 269, 1015, 1248), list_size=46, end_wordmark=230),
}


class Layout:
    def __init__(self, name):
        self.name = name
        for k, v in LAYOUTS[name].items():
            setattr(self, k, v)
        ax0, ay0, ax1, ay1 = self.area
        self.aw, self.ah = ax1 - ax0, ay1 - ay0


# ------------------------------------------------------------ compositing
def _clip_paste(canvas, im, x, y):
    """alpha_composite im onto canvas at integer (x, y), clipped to the canvas."""
    W, H = canvas.size
    sx0, sy0 = max(0, -x), max(0, -y)
    sx1, sy1 = min(im.width, W - x), min(im.height, H - y)
    if sx1 <= sx0 or sy1 <= sy0:
        return
    sub = im.crop((sx0, sy0, sx1, sy1)) if (sx0 or sy0 or sx1 < im.width or sy1 < im.height) else im
    canvas.alpha_composite(sub, (x + sx0, y + sy0))


def paste_layer(canvas, layer, x, y, alpha=1.0, blur=0.0, scale=1.0):
    """Composite an RGBA layer at float (x, y) (sub-pixel), with optional opacity, blur and scale about its centre."""
    if alpha <= 0.004:
        return
    im = layer
    if abs(scale - 1.0) > 0.002:
        nw, nh = max(1, round(im.width * scale)), max(1, round(im.height * scale))
        im = im.resize((nw, nh), Image.BICUBIC)
        x -= (nw - layer.width) / 2
        y -= (nh - layer.height) / 2
    if blur > 0.4:
        im = im.filter(ImageFilter.GaussianBlur(blur))
    if alpha < 0.996:
        a = im.getchannel("A").point(lambda v, k=alpha: int(v * k))
        im = im.copy()
        im.putalpha(a)
    ix, iy = math.floor(x), math.floor(y)
    fx, fy = x - ix, y - iy
    if fx > 0.03 or fy > 0.03:
        im = im.transform((im.width + 1, im.height + 1), Image.AFFINE, (1, 0, -fx, 0, 1, -fy), resample=Image.BILINEAR)
    _clip_paste(canvas, im, ix, iy)


def rounded_mask(w, h, r):
    """Anti-aliased rounded-rectangle alpha mask (corners drawn 4x supersampled, then pasted)."""
    w, h = int(w), int(h)
    r = int(min(r, w // 2, h // 2))
    m = Image.new("L", (w, h), 255)
    if r < 2:
        return m
    # top-left quarter of a filled circle (255 inside), 4x supersampled; the corner is cut where it is 0
    q = Image.new("L", (r * 4, r * 4), 0)
    ImageDraw.Draw(q).ellipse([0, 0, r * 8 - 1, r * 8 - 1], fill=255)
    inside = q.resize((r, r), Image.LANCZOS)  # top-left quarter of the circle: 255 inside
    outside = ImageChops.invert(inside)       # 255 where the corner must be cut away
    for fx, fy, ox, oy in ((0, 0, 0, 0), (1, 0, w - r, 0), (0, 1, 0, h - r), (1, 1, w - r, h - r)):
        c = outside
        if fx:
            c = c.transpose(Image.FLIP_LEFT_RIGHT)
        if fy:
            c = c.transpose(Image.FLIP_TOP_BOTTOM)
        m.paste(0, (ox, oy), c)
    return m


_bg_cache = {}


def make_bg(W, H):
    key = (W, H)
    if key in _bg_cache:
        return _bg_cache[key].copy()
    sw, sh = max(8, W // 8), max(8, H // 8)
    small = Image.new("RGB", (sw, sh))
    px = small.load()
    cx, cy = sw * 0.5, sh * 0.30
    for y in range(sh):
        for x in range(sw):
            d = math.hypot((x - cx) / sw, (y - cy) / sh * 0.9)
            k = clamp(1 - d * 1.35)
            k = k * k
            px[x, y] = (round(lerp(7, 24, k)), round(lerp(8, 27, k)), round(lerp(12, 40, k)))
    bg = small.resize((W, H), Image.BICUBIC)
    noise = Image.effect_noise((W, H), 40).point(lambda v: int((v - 128) * 0.09) + 8)
    bg = ImageChops.add(bg, Image.merge("RGB", (noise, noise, noise)), 1.0, -8)
    _bg_cache[key] = bg
    return bg.copy()


# ------------------------------------------------------------- text bits
def text_layer(s, f, fill=WHITE, pad=10, stroke=0):
    w = int(math.ceil(text_w(f, s))) + pad * 2 + 2
    asc, desc = f.getmetrics()
    h = asc + desc + pad * 2
    im = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    d.text((pad, pad), s, font=f, fill=fill + (255,), stroke_width=stroke)
    return im


TOKEN = re.compile(r"\*|\||[^\s*|]+")


def parse_markup(markup):
    """'Describe the *blend.* | Hear it' -> list of lines, each a list of (word, accent)."""
    lines, cur, acc = [], [], False
    for tok in TOKEN.findall(markup):
        if tok == "*":
            acc = not acc
        elif tok == "|":
            lines.append(cur)
            cur = []
        else:
            cur.append((tok, acc))
    lines.append(cur)
    return [l for l in lines if l]


def layout_words(markup, path, max_w, size_hi, size_lo, max_lines=2, line_gap=1.12):
    """Pick the largest font size whose greedy wrap fits (max_w, max_lines). -> (size, lines[[ (word, accent, x_rel, w) ]], widths)"""
    forced = parse_markup(markup)
    for size in range(int(size_hi), int(size_lo) - 1, -2):
        f = font(path, size)
        sp = text_w(f, " ")
        out = []
        for line in forced:
            cur, cur_w = [], 0.0
            for word, acc in line:
                ww = text_w(f, word)
                if cur and cur_w + sp + ww > max_w:
                    out.append(cur)
                    cur, cur_w = [], 0.0
                x = cur_w + (sp if cur else 0)
                cur.append((word, acc, x, ww))
                cur_w = x + ww
            out.append(cur)
        if len(out) <= max_lines and all(l and (l[-1][2] + l[-1][3]) <= max_w + 1 for l in out):
            return size, out
    return size_lo, out


# -------------------------------------------------------------- items
class Item:
    z = 10
    t0 = 0.0
    t1 = 1e9

    def active(self, T):
        return self.t0 - 0.001 <= T <= self.t1 + 0.001

    def draw(self, ctx, canvas, T):
        raise NotImplementedError


class BarsBG(Item):
    """Brand motion: a row of thin accent bars that breathe like a waveform (the app's own logo motif)."""
    z = 1

    def __init__(self, t0, t1, rect, alpha=0.30, fade=0.5, n=None, seed=0.0, mirror=True, color=ACCENT):
        self.t0, self.t1, self.rect, self.alpha, self.fade = t0, t1, rect, alpha, fade
        self.n, self.seed, self.mirror, self.color = n, seed, mirror, color

    def draw(self, ctx, canvas, T):
        a = min(prog(T, self.t0, self.fade, EASE_STD), 1 - prog(T, self.t1 - self.fade, self.fade, EASE_STD)) * self.alpha
        if a <= 0.004:
            return
        x0, y0, x1, y1 = self.rect
        w, h = x1 - x0, y1 - y0
        n = self.n or max(18, int(w / 22))
        step = w / n
        bw = max(3, int(step * 0.42))
        lay = Image.new("RGBA", (int(w), int(h)), (0, 0, 0, 0))
        d = ImageDraw.Draw(lay)
        for i in range(n):
            u = i / (n - 1)
            env = 0.35 + 0.65 * math.sin(math.pi * u) ** 0.8              # taller in the middle
            m = 0.5 + 0.5 * math.sin(i * 0.83 + T * 2.1 + self.seed) * math.sin(i * 0.31 - T * 1.3 + self.seed * 1.7)
            hh = max(4.0, h * env * (0.18 + 0.82 * m * m))
            cx = i * step + step / 2
            if self.mirror:
                d.rounded_rectangle([cx - bw / 2, h / 2 - hh / 2, cx + bw / 2, h / 2 + hh / 2], radius=bw / 2, fill=self.color + (255,))
            else:
                d.rounded_rectangle([cx - bw / 2, h - hh, cx + bw / 2, h], radius=bw / 2, fill=self.color + (255,))
        paste_layer(canvas, lay, x0, y0, alpha=a)


class Title(Item):
    """Kinetic typography: words rise, fade and de-blur in a stagger (emphasized decelerate), leave on emphasized accelerate."""
    z = 20

    def __init__(self, ctx, markup, t0, t1, box=None, align="center", size=None, path=F_SANS_B, delay=0.0,
                 stagger=0.07, valign="mid", color=WHITE, max_lines=2, enter=0.55, exit_=0.24, min_size=None, fixed=False):
        L = ctx.L
        self.t0, self.t1 = t0, t1
        self.markup = markup
        box = box or L.title_box
        self.box = box
        bw, bh = box[2] - box[0], box[3] - box[1]
        size_hi = size or L.title_max
        size_lo = size if (fixed and size) else (min_size or (L.title_min if not size else max(24, int(size * 0.7))))
        self.size, lines = layout_words(markup, path, bw, size_hi, size_lo, max_lines)
        f = font(path, self.size)
        asc, desc = f.getmetrics()
        lh = (asc + desc) * 1.0
        total_h = lh * len(lines) * 1.02
        y0 = box[1] + (bh - total_h) / 2 if valign == "mid" else (box[1] if valign == "top" else box[3] - total_h)
        self.words = []
        k = 0
        pad = 12
        for li, line in enumerate(lines):
            lw = line[-1][2] + line[-1][3]
            xo = box[0] + (bw - lw) / 2 if align == "center" else box[0]
            for word, acc, x, ww in line:
                lay = text_layer(word, f, ACCENT if acc else color, pad=pad)
                self.words.append((lay, xo + x - pad, y0 + li * lh * 1.02 - pad + 0, k))
                k += 1
        self.stagger, self.delay, self.enter, self.exit_ = stagger, delay, enter, exit_

    def draw(self, ctx, canvas, T):
        q = prog(T, self.t1 - self.exit_, self.exit_, EASE_OUT)
        n = len(self.words)
        stag = min(self.stagger, 0.5 / max(1, n - 1)) if n > 1 else 0
        for lay, x, y, k in self.words:
            p = prog(T, self.t0 + self.delay + k * stag, self.enter, EASE_IN)
            if p <= 0 or q >= 1:
                continue
            a = clamp(p * 1.7) * (1 - q)
            dy = (1 - p) * self.size * 0.42 - q * self.size * 0.16
            paste_layer(canvas, lay, x, y + dy, alpha=a, blur=(1 - p) * 9 + q * 3)


class Stack(Item):
    """A staggered list of rows (the chores, the honest limits)."""
    z = 20

    def __init__(self, ctx, items, t0, t1, box, size=None, heading=None, stagger=0.2, marker="dot", heading_color=ACCENT,
                 row_gap=1.45, delay=0.0, exit_=0.24, center=False):
        L = ctx.L
        self.t0, self.t1, self.box, self.stagger, self.exit_, self.delay = t0, t1, box, stagger, exit_, delay
        size = size or L.list_size
        bw = box[2] - box[0]
        # shrink until the widest row fits
        while size > 24:
            f = font(F_SANS_B, size)
            if max(text_w(f, s) for s in items) + size * 1.1 <= bw:
                break
            size -= 2
        self.size = size
        f = font(F_SANS_B, size)
        pad = 8
        self.rows = []
        y = box[1]
        if heading:
            fh = font(F_SANS_B, max(22, int(size * 0.62)))
            self.heading = (text_layer(heading.upper(), fh, heading_color, pad=pad), box[0] - pad, y - pad)
            y += fh.getmetrics()[0] * 1.9
        else:
            self.heading = None
        for s in items:
            lay = Image.new("RGBA", (int(bw) + pad * 2, int(size * 1.4) + pad * 2), (0, 0, 0, 0))
            d = ImageDraw.Draw(lay)
            r = size * 0.13
            cy = pad + size * 0.72
            if marker == "dot":
                d.ellipse([pad, cy - r, pad + 2 * r, cy + r], fill=ACCENT + (255,))
            elif marker == "dash":
                d.rounded_rectangle([pad, cy - r * 0.5, pad + 2 * r * 1.6, cy + r * 0.5], radius=r * 0.5, fill=ACCENT + (255,))
            d.text((pad + size * 0.85, pad + size * 0.12), s, font=f, fill=WHITE + (255,))
            self.rows.append((lay, box[0] - pad, y - pad))
            y += size * row_gap
        self.end_y = y
        if center:
            used = y - box[1]
            shift = max(0.0, ((box[3] - box[1]) - used) / 2)
            self.rows = [(lay, x, yy + shift) for lay, x, yy in self.rows]
            if self.heading:
                h = self.heading
                self.heading = (h[0], h[1], h[2] + shift)

    def draw(self, ctx, canvas, T):
        q = prog(T, self.t1 - self.exit_, self.exit_, EASE_OUT)
        if q >= 1:
            return
        if self.heading:
            lay, x, y = self.heading
            p = prog(T, self.t0 + self.delay, 0.45, EASE_IN)
            paste_layer(canvas, lay, x, y + (1 - p) * 14, alpha=p * (1 - q))
        for i, (lay, x, y) in enumerate(self.rows):
            p = prog(T, self.t0 + self.delay + 0.15 + i * self.stagger, 0.5, EASE_IN)
            if p <= 0:
                continue
            paste_layer(canvas, lay, x - (1 - p) * 46 - q * 18, y, alpha=clamp(p * 1.6) * (1 - q), blur=(1 - p) * 6)


class Note(Item):
    """Small honest-footage note under the frame, and the thin progress line (brand motion)."""
    z = 30

    def __init__(self, ctx, text, t0, t1):
        L = ctx.L
        self.t0, self.t1 = t0, t1
        f = font(F_SANS, L.note_size)
        self.lay = text_layer(text, f, MUTED, pad=6)
        self.x = (L.W - self.lay.width) / 2
        self.y = L.note_y - 6

    def draw(self, ctx, canvas, T):
        a = min(prog(T, self.t0, 0.5, EASE_STD), 1 - prog(T, self.t1 - 0.3, 0.3, EASE_STD))
        paste_layer(canvas, self.lay, self.x, self.y, alpha=a * 0.85)


class Progress(Item):
    z = 30

    def __init__(self, ctx, total, t0=0.0, t1=None):
        self.total, self.t0, self.t1 = total, t0, t1 if t1 is not None else total

    def draw(self, ctx, canvas, T):
        L = ctx.L
        x0, y, x1 = L.bar
        a = min(prog(T, self.t0, 0.6, EASE_STD), 1 - prog(T, self.t1 - 0.5, 0.5, EASE_STD))
        if a <= 0.01:
            return
        w = x1 - x0
        lay = Image.new("RGBA", (int(w) + 4, 10), (0, 0, 0, 0))
        d = ImageDraw.Draw(lay)
        d.rounded_rectangle([2, 2, w + 2, 8], radius=3, fill=(255, 255, 255, 38))
        fw = max(6, w * clamp(T / self.total))
        d.rounded_rectangle([2, 2, 2 + fw, 8], radius=3, fill=ACCENT + (255,))
        paste_layer(canvas, lay, x0 - 2, y - 5, alpha=a)


# ------------------------------------------------------------- footage
class Decoder:
    def __init__(self, path, ss, dur):
        cmd = ["nice", "-n", "10", "ffmpeg", "-nostdin", "-v", "error", "-ss", f"{ss:.3f}", "-t", f"{dur + 0.5:.3f}", "-i", path,
               "-vf", f"fps={FPS}", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]
        self.p = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, bufsize=SRC_W * SRC_H * 3 * 2)
        self.n = SRC_W * SRC_H * 3
        self.last = None
        self.idx = -1

    def frame(self, idx):
        while self.idx < idx:
            d = self.p.stdout.read(self.n)
            if len(d) < self.n:
                break
            self.last = Image.frombytes("RGB", (SRC_W, SRC_H), d)
            self.idx += 1
        return self.last

    def close(self):
        try:
            self.p.stdout.close()
            self.p.kill()
            self.p.wait(timeout=5)
        except Exception:
            pass


def fit_frame(L, crop, align="c", scale_cap=None):
    """Frame rectangle (stage) for a crop (source) = contain in the layout's area with a scale cap."""
    cx, cy, cw, ch = crop
    ax0, ay0, ax1, ay1 = L.area
    cap = scale_cap or L.max_scale
    s = min(L.aw / cw, L.ah / ch, cap)
    fw, fh = cw * s, ch * s
    if align == "r":
        fx = ax1 - fw
    elif align == "l":
        fx = ax0
    else:
        fx = ax0 + (L.aw - fw) / 2
    fy = ay0 + (L.ah - fh) / 2
    return (fx, fy, fw, fh)


class FootageClip(Item):
    """A stretch of a real demo recording shown in a virtual camera.

    shots: list of dicts {t, crop|name, move, align}. The first shot is the starting frame; later shots arrive
    at t (clip-local seconds) after a camera move of `move` seconds (default 0.9).
    """
    z = 5

    def __init__(self, ctx, demo, src0, dur, t0, shots, regions, fade_in=0.5, fade_out=0.3, rise=True, drift=0.012,
                 audio=True, audio_gain_db=0.0):
        self.ctx = ctx
        L = ctx.L
        self.demo, self.src0, self.dur, self.t0, self.t1 = demo, src0, dur, t0, t0 + dur
        self.path = f"{DEMO_DIR}/{demo}.mp4"
        self.fade_in, self.fade_out, self.rise, self.drift = fade_in, fade_out, rise, drift
        self.audio, self.audio_gain_db = audio, audio_gain_db
        self.dec = None
        kfs = []
        prev = None
        for i, sh in enumerate(shots):
            crop = regions[sh["crop"]] if isinstance(sh["crop"], str) else sh["crop"]
            if isinstance(crop, dict):
                crop = crop[L.name]
            frame = fit_frame(L, crop, sh.get("align", "c"), sh.get("cap"))
            kf = (crop, frame)
            t = sh.get("t", 0.0)
            if prev is not None:
                kfs.append((max(kfs[-1][0], t - sh.get("move", 0.9)), prev[0], prev[1]))
            kfs.append((t, kf[0], kf[1]))
            prev = kf
        kfs.append((max(dur, kfs[-1][0]) + 5, prev[0], prev[1]))
        self.kfs = kfs

    def camera(self, lt):
        kfs = self.kfs
        if lt <= kfs[0][0]:
            crop, frame = kfs[0][1], kfs[0][2]
        else:
            crop, frame = kfs[-1][1], kfs[-1][2]
            for (ta, ca, fa), (tb, cb, fb) in zip(kfs, kfs[1:]):
                if ta <= lt < tb:
                    if ca == cb and fa == fb:
                        crop, frame = ca, fa
                    else:
                        e = EASE_CAM((lt - ta) / (tb - ta))
                        crop = _interp_rect(ca, cb, e)
                        frame = _interp_rect(fa, fb, e)
                    break
        if self.drift:
            k = 1 - self.drift * clamp(lt / max(1.0, self.dur))
            cx, cy, cw, ch = crop
            crop = (cx + cw * (1 - k) / 2, cy + ch * (1 - k) / 2, cw * k, ch * k)
        return crop, frame

    def mapper(self, T):
        lt = T - self.t0
        crop, frame = self.camera(lt)
        rise = self._rise(lt)
        cx, cy, cw, ch = crop
        fx, fy, fw, fh = frame
        fy += rise

        def m(sx, sy):
            return fx + (sx - cx) * fw / cw, fy + (sy - cy) * fh / ch

        return m, (fx, fy, fw, fh), fw / cw

    def alpha(self, lt):
        return min(prog(lt, 0, self.fade_in, EASE_STD), 1 - prog(lt, self.dur - self.fade_out, self.fade_out, EASE_STD))

    def _rise(self, lt):
        if not self.rise:
            return 0.0
        return (1 - prog(lt, 0, self.fade_in + 0.15, EASE_IN)) * 36

    def _decoder(self, lt):
        idx = int(round(lt * FPS))
        if self.dec is not None and idx - self._base < self.dec.idx:
            self.dec.close()
            self.dec = None
        if self.dec is None:
            start = max(0.0, lt)
            self.dec = Decoder(self.path, self.src0 + start, max(0.5, self.dur - start))
            self._base = idx
        return self.dec.frame(idx - self._base)

    def close(self):
        if self.dec:
            self.dec.close()
            self.dec = None

    def draw(self, ctx, canvas, T):
        L = ctx.L
        lt = T - self.t0
        a = self.alpha(lt)
        if a <= 0.004:
            return
        src = self._decoder(lt)
        if src is None:
            return
        crop, frame = self.camera(lt)
        fx, fy, fw, fh = frame
        fy += self._rise(lt)
        W, H = canvas.size
        ifw, ifh = max(2, int(round(fw))), max(2, int(round(fh)))
        ifx, ify = int(round(fx)), int(round(fy))
        cx, cy, cw, ch = crop
        box = (cx, cy, cx + cw, cy + ch)
        # shadow: a small blurred rectangle scaled up (cheap, smooth), only over the frame's neighbourhood
        k = 8
        bx0, by0 = max(0, (ifx - 80) // k * k), max(0, (ify - 60) // k * k)
        bx1, by1 = min(W, (ifx + ifw + 80) // k * k + k), min(H, (ify + ifh + 110) // k * k + k)
        sw_, sh_ = (bx1 - bx0) // k, (by1 - by0) // k
        if sw_ > 2 and sh_ > 2:
            sh = Image.new("L", (sw_, sh_), 0)
            ImageDraw.Draw(sh).rounded_rectangle([(ifx - bx0) / k, (ify - by0 + 10) / k, (ifx + ifw - bx0) / k, (ify + ifh - by0 + 10) / k],
                                                 radius=L.radius / k, fill=170)
            sh = sh.filter(ImageFilter.GaussianBlur(3.2)).resize((bx1 - bx0, by1 - by0), Image.BILINEAR)
            sh = sh.point(lambda v, kk=a: int(v * kk))
            shadow = Image.new("RGBA", sh.size, (0, 0, 0, 0))
            shadow.putalpha(sh)
            _clip_paste(canvas, shadow, bx0, by0)
        region = src.resize((ifw, ifh), Image.LANCZOS, box=box)
        if ifw / cw > 1.3:
            region = region.filter(ImageFilter.UnsharpMask(radius=1.6, percent=70, threshold=2))
        mask = rounded_mask(ifw, ifh, L.radius)
        if a < 0.996:
            mask = mask.point(lambda v, kk=a: int(v * kk))
        # paste clipped to canvas
        rgba = region.convert("RGBA")
        rgba.putalpha(mask)
        _clip_paste(canvas, rgba, ifx, ify)
        # hairline border
        bd = Image.new("RGBA", (ifw, ifh), (0, 0, 0, 0))
        ImageDraw.Draw(bd).rounded_rectangle([0, 0, ifw - 1, ifh - 1], radius=L.radius, outline=(255, 255, 255, int(34 * a)), width=2)
        _clip_paste(canvas, bd, ifx, ify)


def _interp_rect(a, b, e):
    """Interpolate rectangles: centre linearly, size geometrically (constant perceived zoom speed)."""
    ax, ay, aw, ah = a
    bx, by, bw, bh = b
    cx = lerp(ax + aw / 2, bx + bw / 2, e)
    cy = lerp(ay + ah / 2, by + bh / 2, e)
    w = aw * (bw / aw) ** e
    h = ah * (bh / ah) ** e
    return (cx - w / 2, cy - h / 2, w, h)


# -------------------------------------------------------------- callouts
def rr_path(x0, y0, x1, y1, r, n=7):
    """Closed rounded-rectangle outline as a point list starting at the top-middle, clockwise."""
    pts = [((x0 + x1) / 2, y0), (x1 - r, y0)]
    for a in range(1, n + 1):
        t = -math.pi / 2 + (math.pi / 2) * a / n
        pts.append((x1 - r + r * math.cos(t), y0 + r + r * math.sin(t)))
    pts.append((x1, y1 - r))
    for a in range(1, n + 1):
        t = (math.pi / 2) * a / n
        pts.append((x1 - r + r * math.cos(t), y1 - r + r * math.sin(t)))
    pts.append((x0 + r, y1))
    for a in range(1, n + 1):
        t = math.pi / 2 + (math.pi / 2) * a / n
        pts.append((x0 + r + r * math.cos(t), y1 - r + r * math.sin(t)))
    pts.append((x0, y0 + r))
    for a in range(1, n + 1):
        t = math.pi + (math.pi / 2) * a / n
        pts.append((x0 + r + r * math.cos(t), y0 + r + r * math.sin(t)))
    pts.append(((x0 + x1) / 2, y0))
    return pts


def trim_path(pts, p):
    if p >= 0.999:
        return pts
    segs = [math.hypot(b[0] - a[0], b[1] - a[1]) for a, b in zip(pts, pts[1:])]
    target = sum(segs) * p
    out, acc = [pts[0]], 0.0
    for (a, b), sl in zip(zip(pts, pts[1:]), segs):
        if acc + sl >= target:
            t = (target - acc) / sl if sl else 0
            out.append((lerp(a[0], b[0], t), lerp(a[1], b[1], t)))
            return out
        out.append(b)
        acc += sl
    return out


class Callout(Item):
    """Animated callout bound to a region of the recording: ring draws on, leader line, chip springs in.

    region is in SOURCE pixels so it follows the camera; t_in/t_out are clip-local seconds.
    """
    z = 25

    def __init__(self, ctx, clip, region, label, t_in, t_out, sides=("t", "b", "l", "r"), pad=7, icon=None, ring=True,
                 chip_dx=0, chip_dy=0, number=None):
        self.ctx, self.clip, self.region, self.label = ctx, clip, region, label
        self.t0, self.t1 = clip.t0 + t_in, clip.t0 + t_out
        self.sides, self.pad, self.ring, self.number = sides, pad, ring, number
        self.chip_dx, self.chip_dy = chip_dx, chip_dy
        L = ctx.L
        f = font(F_SANS_B, L.chip_size)
        self.f = f
        tw = text_w(f, label)
        self.pad_x, self.pad_y = int(L.chip_size * 0.62), int(L.chip_size * 0.36)
        asc, desc = f.getmetrics()
        self.num_w = int(L.chip_size * 1.25) if number else 0
        self.cw = int(tw + self.pad_x * 2 + self.num_w)
        self.ch = int(asc + desc + self.pad_y * 2)
        self.chip = self._render_chip()

    def _render_chip(self):
        s = 2  # supersample
        w, h = self.cw * s + 12, self.ch * s + 12
        im = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        d = ImageDraw.Draw(im)
        d.rounded_rectangle([6, 6, w - 7, h - 7], radius=(h - 12) / 2, fill=(12, 13, 19, 238), outline=ACCENT + (255,), width=3 * s // 2 + 1)
        L = self.ctx.L
        f2 = font(F_SANS_B, L.chip_size * s)
        asc, desc = f2.getmetrics()
        tx = 6 + self.pad_x * s + self.num_w * s
        if self.number:
            fn = font(F_SANS_B, int(L.chip_size * s * 0.82))
            nw = text_w(fn, str(self.number))
            r = (h - 12) / 2 - 8 * s
            cx = 6 + self.pad_x * s * 0.55 + r
            d.ellipse([cx - r, h / 2 - r, cx + r, h / 2 + r], fill=ACCENT + (255,))
            d.text((cx - nw / 2, h / 2 - fn.getmetrics()[0] * 0.56), str(self.number), font=fn, fill=(10, 10, 14, 255))
        d.text((tx, 6 + self.pad_y * s - 2 * s), self.label, font=f2, fill=WHITE + (255,))
        return im.resize((self.cw + 6, self.ch + 6), Image.LANCZOS)

    def _region(self, lt):
        """A rect, or [(clip_time, rect), ...] when the recorded UI scrolls under the callout (linear in between)."""
        r = self.region
        if not isinstance(r, list):
            return r
        if lt <= r[0][0]:
            return r[0][1]
        for (ta, ra), (tb, rb) in zip(r, r[1:]):
            if ta <= lt <= tb:
                k = (lt - ta) / (tb - ta) if tb > ta else 0
                return tuple(lerp(a, b, k) for a, b in zip(ra, rb))
        return r[-1][1]

    def draw(self, ctx, canvas, T):
        L = ctx.L
        if T < self.t0 or T > self.t1:
            return
        mapper, frame, scale = self.clip.mapper(T)
        x, y, w, h = self._region(T - self.clip.t0)
        X0, Y0 = mapper(x - self.pad, y - self.pad)
        X1, Y1 = mapper(x + w + self.pad, y + h + self.pad)
        lt = T - self.t0
        exit_q = prog(T, self.t1 - 0.25, 0.25, EASE_OUT)
        fa = self.clip.alpha(T - self.clip.t0)
        a_all = (1 - exit_q) * fa
        if a_all <= 0.01:
            return
        fx, fy, fw, fh = frame
        # --- ring (draws itself on over 0.5 s, then breathes)
        ring_p = prog(lt, 0.0, 0.55, EASE_IN)
        rw = L.ring_w
        pulse = 0.88 + 0.12 * math.sin(lt * 2 * math.pi * 0.9)
        # clip ring region to the frame
        cX0, cY0, cX1, cY1 = max(X0, fx + 2), max(Y0, fy + 2), min(X1, fx + fw - 2), min(Y1, fy + fh - 2)
        if self.ring and cX1 - cX0 > 6 and cY1 - cY0 > 6:
            ss = 3
            bx0, by0 = int(cX0 - rw * 2 - 6), int(cY0 - rw * 2 - 6)
            bw_, bh_ = int(cX1 - cX0 + rw * 4 + 12), int(cY1 - cY0 + rw * 4 + 12)
            lay = Image.new("RGBA", (bw_ * ss, bh_ * ss), (0, 0, 0, 0))
            d = ImageDraw.Draw(lay)
            r = min(14, (cY1 - cY0) / 2, (cX1 - cX0) / 2)
            pts = rr_path((cX0 - bx0) * ss, (cY0 - by0) * ss, (cX1 - bx0) * ss, (cY1 - by0) * ss, r * ss)
            pts = trim_path(pts, ring_p)
            if len(pts) > 1:
                # soft glow underneath, crisp stroke on top
                glow = Image.new("RGBA", lay.size, (0, 0, 0, 0))
                ImageDraw.Draw(glow).line(pts, fill=ACCENT + (120,), width=int(rw * ss * 2.6), joint="curve")
                glow = glow.filter(ImageFilter.GaussianBlur(rw * ss * 1.5))
                lay = Image.alpha_composite(lay, glow)
                ImageDraw.Draw(lay).line(pts, fill=ACCENT + (255,), width=int(rw * ss), joint="curve")
            lay = lay.resize((bw_, bh_), Image.LANCZOS)
            paste_layer(canvas, lay, bx0, by0, alpha=a_all * pulse)
            # ripple once the ring closes
            rp = prog(lt, 0.55, 0.7, EASE_STD)
            if 0 < rp < 1:
                g = rw * 3 * rp
                rl = Image.new("RGBA", (bw_, bh_), (0, 0, 0, 0))
                ImageDraw.Draw(rl).rounded_rectangle([cX0 - bx0 - g, cY0 - by0 - g, cX1 - bx0 + g, cY1 - by0 + g], radius=r + g,
                                                      outline=ACCENT + (255,), width=2)
                paste_layer(canvas, rl, bx0, by0, alpha=a_all * (1 - rp) * 0.7)
        # --- chip placement
        gap = 34
        rcx, rcy = (cX0 + cX1) / 2, (cY0 + cY1) / 2
        sx0, sy0, sx1, sy1 = L.chips
        cands = {
            "t": (rcx - self.cw / 2, cY0 - gap - self.ch),
            "b": (rcx - self.cw / 2, cY1 + gap),
            "l": (cX0 - gap - self.cw, rcy - self.ch / 2),
            "r": (cX1 + gap, rcy - self.ch / 2),
        }
        pos = None
        order = self.sides
        if L.name == "wide":
            first = "l" if rcx < fx + fw / 2 else "r"
            order = (first, "r" if first == "l" else "l") + tuple(self.sides)
        for s in order:
            px, py = cands[s]
            px, py = px + self.chip_dx, py + self.chip_dy
            if s in "tb":
                px = clamp(px, sx0, sx1 - self.cw)
            else:
                py = clamp(py, sy0, sy1 - self.ch)
            if px >= sx0 and px + self.cw <= sx1 and py >= sy0 and py + self.ch <= sy1:
                pos, side = (px, py), s
                break
        if pos is None:
            s = order[0]
            px = clamp(cands[s][0], sx0, sx1 - self.cw)
            py = clamp(cands[s][1], sy0, sy1 - self.ch)
            pos, side = (px, py), s
        px, py = pos
        t_chip = 0.35
        sp = spring(lt - t_chip)
        # leader line from the ring edge to the chip edge
        lp = prog(lt, 0.3, 0.3, EASE_IN)
        if side == "t":
            a_pt, b_pt = (clamp(px + self.cw / 2, cX0 + 8, cX1 - 8), cY0), (clamp(px + self.cw / 2, cX0 + 8, cX1 - 8), py + self.ch)
        elif side == "b":
            a_pt, b_pt = (clamp(px + self.cw / 2, cX0 + 8, cX1 - 8), cY1), (clamp(px + self.cw / 2, cX0 + 8, cX1 - 8), py)
        elif side == "l":
            a_pt, b_pt = (cX0, clamp(py + self.ch / 2, cY0 + 8, cY1 - 8)), (px + self.cw, py + self.ch / 2)
        else:
            a_pt, b_pt = (cX1, clamp(py + self.ch / 2, cY0 + 8, cY1 - 8)), (px, py + self.ch / 2)
        if self.ring and lp > 0:
            e = (lerp(a_pt[0], b_pt[0], lp), lerp(a_pt[1], b_pt[1], lp))
            x0_, y0_ = min(a_pt[0], e[0]) - 6, min(a_pt[1], e[1]) - 6
            lw_, lh_ = int(abs(e[0] - a_pt[0]) + 12), int(abs(e[1] - a_pt[1]) + 12)
            if lw_ > 2 and lh_ > 2:
                ll = Image.new("RGBA", (lw_ * 3, lh_ * 3), (0, 0, 0, 0))
                ImageDraw.Draw(ll).line([((a_pt[0] - x0_) * 3, (a_pt[1] - y0_) * 3), ((e[0] - x0_) * 3, (e[1] - y0_) * 3)],
                                        fill=ACCENT + (255,), width=int(L.ring_w * 0.75 * 3))
                ll = ll.resize((lw_, lh_), Image.LANCZOS)
                paste_layer(canvas, ll, x0_, y0_, alpha=a_all)
        if sp > 0:
            sc = 0.86 + 0.14 * sp
            paste_layer(canvas, self.chip, px - 3, py - 3, alpha=clamp(sp * 1.4) * a_all, scale=sc)


class Flow(Item):
    """'How it works' diagram: two boxes and one arrow, drawn on. Not app UI: a labelled diagram."""
    z = 20

    def __init__(self, ctx, t0, t1, left, right, arrow_label, chips, vertical=False, box=None):
        L = ctx.L
        self.t0, self.t1 = t0, t1
        self.ctx = ctx
        bx0, by0, bx1, by1 = box or (L.safe[0] + 20, L.title_box[3] + 20, L.safe[2] - 20, L.safe[3] - 70)
        self.box = (bx0, by0, bx1, by1)
        self.vertical = vertical
        self.left, self.right, self.arrow_label, self.chips = left, right, arrow_label, chips
        self.pieces = []
        self._build(L)

    def _panel(self, w, h, title, lines, accent=False, chips=None, size=28):
        s = 2
        while size > 20 and max([text_w(font(F_SANS_B, size * 1.12), title)] + [text_w(font(F_SANS, size), ln) for ln in lines]) > w - 56:
            size -= 1
        im = Image.new("RGBA", (int(w) * s + 8, int(h) * s + 8), (0, 0, 0, 0))
        d = ImageDraw.Draw(im)
        d.rounded_rectangle([4, 4, w * s + 4, h * s + 4], radius=22 * s, fill=(14, 15, 22, 235),
                            outline=(ACCENT if accent else (90, 96, 112)) + (255,), width=3 * s)
        ft = font(F_SANS_B, size * s * 1.12)
        d.text((24 * s, 18 * s), title, font=ft, fill=(ACCENT if accent else WHITE) + (255,))
        y = 18 * s + ft.getmetrics()[0] * 1.5
        fl = font(F_SANS, size * s)
        for ln in lines:
            d.text((24 * s, y), ln, font=fl, fill=(220, 224, 232, 255))
            y += size * s * 1.38
        if chips:
            fc = font(F_SANS_B, size * s * 0.82)
            x, y2 = 24 * s, y + 8 * s
            for c in chips:
                cw_ = text_w(fc, c) + 28 * s
                if x + cw_ > w * s - 20 * s:
                    x = 24 * s
                    y2 += size * s * 1.45
                d.rounded_rectangle([x, y2, x + cw_, y2 + size * s * 1.2], radius=size * s * 0.6, outline=(120, 126, 142, 255), width=2 * s,
                                    fill=(24, 26, 36, 255))
                d.text((x + 14 * s, y2 + size * s * 0.12), c, font=fc, fill=WHITE + (255,))
                x += cw_ + 12 * s
        return im.resize((int(w) + 4, int(h) + 4), Image.LANCZOS)

    def _build(self, L):
        x0, y0, x1, y1 = self.box
        W, H = x1 - x0, y1 - y0
        size = 52 if L.name == "wide" else 40
        if self.vertical:
            ph = (H - 110) / 2
            self.pl = (x0, y0, W, ph)
            self.pr = (x0, y1 - ph, W, ph)
        else:
            gap = 240 if L.name == "wide" else 120
            pw = (W - gap) / 2
            ph = min(H, 430 if L.name == "wide" else 400)
            py = y0 + (H - ph) / 2
            self.pl = (x0, py, pw, ph)
            self.pr = (x0 + pw + gap, py, pw, ph)
        self.img_l = self._panel(self.pl[2], self.pl[3], self.left[0], self.left[1], True, None, size)
        self.img_r = self._panel(self.pr[2], self.pr[3], self.right[0], self.right[1], False, self.chips, size)

    def draw(self, ctx, canvas, T):
        q = prog(T, self.t1 - 0.25, 0.25, EASE_OUT)
        if q >= 1:
            return
        L = ctx.L
        p1 = prog(T, self.t0, 0.6, EASE_IN)
        p2 = prog(T, self.t0 + 0.7, 0.6, EASE_IN)
        pa = prog(T, self.t0 + 1.2, 0.7, EASE_IN)
        paste_layer(canvas, self.img_l, self.pl[0] - 2, self.pl[1] - 2 + (1 - p1) * 40, alpha=p1 * (1 - q))
        paste_layer(canvas, self.img_r, self.pr[0] - 2, self.pr[1] - 2 + (1 - p2) * 40, alpha=p2 * (1 - q))
        # arrow between panels, labelled
        if pa > 0:
            if self.vertical:
                ax = L.W / 2
                ay0, ay1 = self.pl[1] + self.pl[3] + 8, self.pr[1] - 8
                pts = [(ax, ay0), (ax, lerp(ay0, ay1, pa))]
                lab_pos = (ax + 26, (ay0 + ay1) / 2)
            else:
                ay = self.pl[1] + self.pl[3] / 2
                ax0, ax1 = self.pl[0] + self.pl[2] + 10, self.pr[0] - 10
                pts = [(ax0, ay), (lerp(ax0, ax1, pa), ay)]
                lab_pos = ((ax0 + ax1) / 2, ay + 16)
            ss = 3
            bx, by = int(min(p[0] for p in pts) - 20), int(min(p[1] for p in pts) - 20)
            bw_, bh_ = int(abs(pts[1][0] - pts[0][0]) + 40), int(abs(pts[1][1] - pts[0][1]) + 40)
            lay = Image.new("RGBA", (bw_ * ss, bh_ * ss), (0, 0, 0, 0))
            d = ImageDraw.Draw(lay)
            d.line([((px - bx) * ss, (py - by) * ss) for px, py in pts], fill=ACCENT + (255,), width=5 * ss)
            ex, ey = pts[1]
            if pa > 0.95:
                if self.vertical:
                    tri = [((ex - 14 - bx) * ss, (ey - 6 - by) * ss), ((ex + 14 - bx) * ss, (ey - 6 - by) * ss), ((ex - bx) * ss, (ey + 14 - by) * ss)]
                else:
                    tri = [((ex - 6 - bx) * ss, (ey - 14 - by) * ss), ((ex - 6 - bx) * ss, (ey + 14 - by) * ss), ((ex + 14 - bx) * ss, (ey - by) * ss)]
                d.polygon(tri, fill=ACCENT + (255,))
            lay = lay.resize((bw_, bh_), Image.LANCZOS)
            paste_layer(canvas, lay, bx, by, alpha=1 - q)
            f = font(F_SANS_B, 26 if L.name != "wide" else 30)
            lt = text_layer(self.arrow_label, f, ACCENT, pad=6)
            lx = lab_pos[0] - (0 if self.vertical else lt.width / 2)
            paste_layer(canvas, lt, lx, lab_pos[1] - (lt.height / 2 if self.vertical else 0), alpha=prog(T, self.t0 + 1.7, 0.4, EASE_IN) * (1 - q))


class EndCard(Item):
    """Brand sign-off: wordmark (serif italic 'edyt' + 'lab', as in the app) over breathing bars, then the facts."""
    z = 20

    def __init__(self, ctx, t0, t1, lines, url_pill=None, small=None):
        L = ctx.L
        self.t0, self.t1 = t0, t1
        S = L.end_wordmark
        a = text_layer("edyt", font(F_SERIF_I, S), ACCENT, pad=14)
        b = text_layer("lab", font(F_SERIF, S), WHITE, pad=14)
        self.a, self.b = a, b
        tw = a.width + b.width - 28
        cx = L.W / 2
        self.ax = cx - tw / 2 - 14
        self.bx = self.ax + a.width - 14
        self.wy = (L.safe[1] + (L.safe[3] - L.safe[1]) * (0.30 if L.name != "wide" else 0.27)) - a.height / 2
        self.lines = []
        y = self.wy + a.height + 6
        f1 = font(F_SANS_B, int(L.title_max * 0.62))
        for ln in lines:
            lay = text_layer(ln[0], font(F_SANS_B if ln[1] else F_SANS, int(L.title_max * (0.62 if ln[1] else 0.5))), WHITE if ln[1] else MUTED, pad=8)
            self.lines.append((lay, (L.W - lay.width) / 2, y))
            y += lay.height * 1.0
        self.pill = None
        if url_pill:
            fp = font(F_SANS_B, int(L.title_max * 0.56))
            tw_ = text_w(fp, url_pill)
            pw_, ph_ = int(tw_ + 70), int(fp.getmetrics()[0] * 1.9)
            s = 2
            im = Image.new("RGBA", ((pw_ + 12) * s, (ph_ + 12) * s), (0, 0, 0, 0))
            d = ImageDraw.Draw(im)
            d.rounded_rectangle([6 * s, 6 * s, (pw_ + 6) * s, (ph_ + 6) * s], radius=ph_ * s / 2, fill=(14, 15, 22, 240), outline=ACCENT + (255,), width=4 * s)
            d.text((6 * s + 35 * s, 6 * s + ph_ * s * 0.2), url_pill, font=font(F_SANS_B, int(L.title_max * 0.56) * s), fill=WHITE + (255,))
            self.pill = (im.resize((pw_ + 12, ph_ + 12), Image.LANCZOS), (L.W - pw_ - 12) / 2, y + 18)
            y += ph_ + 36
        self.small = []
        for sm in (small or []):
            lay = text_layer(sm, font(F_SANS, int(L.title_max * 0.48)), MUTED, pad=6)
            self.small.append((lay, (L.W - lay.width) / 2, y + 10))
            y += lay.height + 4

    def draw(self, ctx, canvas, T):
        q = prog(T, self.t1 - 0.3, 0.3, EASE_STD)
        if q >= 1:
            return
        pa = prog(T, self.t0 + 0.15, 0.7, EASE_IN)
        pb = prog(T, self.t0 + 0.30, 0.7, EASE_IN)
        paste_layer(canvas, self.a, self.ax, self.wy + (1 - pa) * 60, alpha=clamp(pa * 1.5) * (1 - q), blur=(1 - pa) * 14)
        paste_layer(canvas, self.b, self.bx, self.wy + (1 - pb) * 60, alpha=clamp(pb * 1.5) * (1 - q), blur=(1 - pb) * 14)
        k = 0
        for lay, x, y in self.lines:
            p = prog(T, self.t0 + 0.8 + k * 0.18, 0.55, EASE_IN)
            paste_layer(canvas, lay, x, y + (1 - p) * 26, alpha=clamp(p * 1.6) * (1 - q), blur=(1 - p) * 6)
            k += 1
        if self.pill:
            lay, x, y = self.pill
            p = prog(T, self.t0 + 0.8 + k * 0.18, 0.6, EASE_IN)
            sp = spring(T - (self.t0 + 0.8 + k * 0.18))
            paste_layer(canvas, lay, x, y, alpha=clamp(p * 1.6) * (1 - q), scale=0.9 + 0.1 * sp)
            k += 1
        for lay, x, y in self.small:
            p = prog(T, self.t0 + 0.8 + k * 0.18, 0.55, EASE_IN)
            paste_layer(canvas, lay, x, y + (1 - p) * 14, alpha=clamp(p * 1.6) * (1 - q))
            k += 1


# ---------------------------------------------------------------- piece
class Ctx:
    def __init__(self, layout_name):
        self.L = Layout(layout_name)


class Piece:
    def __init__(self, layout_name, duration):
        self.ctx = Ctx(layout_name)
        self.L = self.ctx.L
        self.duration = duration
        self.items = []

    def add(self, item):
        self.items.append(item)
        return item

    def footage(self):
        return [i for i in self.items if isinstance(i, FootageClip)]

    def still(self, T):
        """One frame as a PIL RGB image (for contact sheets and QC)."""
        canvas = make_bg(self.L.W, self.L.H).convert("RGBA")
        for it in sorted(self.items, key=lambda i: i.z):
            if it.active(T):
                it.draw(self.ctx, canvas, T)
        for c in self.footage():
            c.close()
        return canvas.convert("RGB")

    def render(self, out_path, t_from=0.0, t_to=None, crf=19, preset="medium", scale_out=None, quiet=False):
        L = self.L
        t_to = t_to if t_to is not None else self.duration
        W, H = L.W, L.H
        n0, n1 = int(round(t_from * FPS)), int(round(t_to * FPS))
        items = sorted(self.items, key=lambda i: i.z)
        bg = make_bg(W, H)
        ow, oh = (scale_out or (W, H))
        vf = [] if (ow, oh) == (W, H) else ["-vf", f"scale={ow}:{oh}:flags=lanczos"]
        cmd = ["nice", "-n", "10", "ffmpeg", "-nostdin", "-y", "-v", "error", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{W}x{H}", "-r", str(FPS),
               "-i", "-"] + vf + ["-an", "-c:v", "libx264", "-preset", preset, "-crf", str(crf), "-pix_fmt", "yuv420p", "-profile:v", "high",
                                  "-movflags", "+faststart", "-threads", "3", out_path]
        enc = subprocess.Popen(cmd, stdin=subprocess.PIPE)
        try:
            for n in range(n0, n1):
                T = n / FPS
                canvas = bg.convert("RGBA")
                for it in items:
                    if it.active(T):
                        it.draw(self.ctx, canvas, T)
                    elif isinstance(it, FootageClip):
                        it.close()
                enc.stdin.write(canvas.convert("RGB").tobytes())
                if not quiet and (n - n0) % 60 == 0:
                    print(f"  frame {n - n0}/{n1 - n0}", flush=True)
        finally:
            for c in self.footage():
                c.close()
            try:
                enc.stdin.close()
            except Exception:
                pass
            enc.wait()
        return out_path
