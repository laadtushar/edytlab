"""Render the 1080x1920 reels: PIL draws the background, cards and caption stills, one ffmpeg
filter graph does the compositing. Everything runs under `nice -n 10`.

The look is the website's (brand.py): its dark tokens, Inter, the logo, and the cards of the demo
videos' own burned-in captions (an opaque card with a violet edge, so a caption's contrast never
depends on the frame behind it).

Where things sit on the 1080x1920 canvas. Everything this file adds (captions, pill, note, progress
bar, card text) stays inside SAFE: 120 px clear of both sides (the right side is where Reels,
Shorts and TikTok put their buttons) and above the bottom 20 % (their caption and sound bar). The
recording itself runs edge to edge. `assert_safe` checks each layer when it is drawn.
"""
import io, json, math, os, re, subprocess, sys
from PIL import Image, ImageChops, ImageDraw, ImageFilter
from spec import *
from cues import build_reel, FPS
import brand   # the site's colours and Inter, from website/app/globals.css and the `fonts-inter` package, as make-demo-videos.mjs uses them
from brand import font, BACKGROUND, CARD, FOREGROUND, PRIMARY, MUTED_FG, BORDER, GRADIENT, AURORA

W, H = 1080, 1920
CARD_WASH = 1.6     # the aurora on the hook and end cards, in multiples of the site's own opacity: a card is nothing but the wash, so it gets more (put_text keeps every line at 4.5:1 or better)
SAFE_L, SAFE_R, SAFE_TOP, SAFE_BOTTOM = 120, 960, 250, round(H * 0.8)
FRAME_W, FRAME_H, FRAME_Y = 1080, 654, 720      # the 1280x776 recording, scaled to the full width
CAP_Y, CAP_H = 430, 270                          # the reel's own caption: a band above the recording
PILL_Y = 366
NOTE_Y = 1402
BAR_Y = 1458
CARD_PAD_X, CARD_PAD_Y, EDGE_W = 32, 26, 8      # the caption card; the violet edge is EDGE_W wide
CARD_R = brand.RADIUS_PX * 2                    # the site's --radius, at the reels' 2x (a canvas is about twice a site section's px size)
WORK = f"{ROOT}/work"
THREADS = os.environ.get("REEL_THREADS", "3")
X264_PARAMS = os.environ.get("REEL_X264", "aq-mode=3")
GRAIN = float(os.environ.get("REEL_GRAIN", "0.5"))   # std. dev. (8-bit levels) of the noise on the cards' background, see background()


def run(cmd, **kw):
    cmd = ["nice", "-n", "10"] + cmd
    p = subprocess.run(cmd, capture_output=True, text=True, **kw)
    if p.returncode != 0:
        raise RuntimeError("command failed: " + " ".join(cmd[:6]) + "\n" + p.stderr[-2500:])
    return p


rgba = brand.rgba


def assert_safe(layer, what, dx=0, dy=0, left=SAFE_L, right=SAFE_R, top=0, bottom=SAFE_BOTTOM):
    """The layer's visible pixels, placed at (dx, dy) on the canvas, lie inside the safe area."""
    box = layer.getchannel("A").point(lambda v: 255 if v > 8 else 0).getbbox()
    if box is None:
        return
    x0, y0, x1, y1 = box[0] + dx, box[1] + dy, box[2] + dx, box[3] + dy
    if x0 < left or x1 > right or y0 < top or y1 > bottom:
        raise ValueError(f"{what} reaches {(x0, y0, x1, y1)}, outside the safe area {(left, top, right, bottom)}")


def check_contrast(bg, box, colour, what):
    """Text of `colour` over `bg` in `box`: 4.5:1 or better against the brightest the background gets there
    (the per-channel maximum in the box, which is at least as bright as any one pixel)."""
    ext = bg.convert("RGB").crop(tuple(round(v) for v in box)).getextrema()
    ratio = brand.contrast(colour, tuple(e[1] for e in ext))
    if ratio < brand.MIN_CONTRAST:
        raise ValueError(f"{what}: {ratio:.2f}:1 over the brightest part of the background behind it, under {brand.MIN_CONTRAST}:1")


def put_text(layer, bg, xy, text, f, colour, anchor, what):
    """Draw text on `layer` and check its contrast against `bg`, the picture the layer will sit on."""
    ImageDraw.Draw(layer).text(xy, text, font=f, fill=rgba(colour), anchor=anchor)
    check_contrast(bg, ImageDraw.Draw(layer).textbbox(xy, text, font=f, anchor=anchor), colour, what)


# ------------------------------------------------------------------ background
def background(strength=1.0, grain=0.0, glow=True):
    """The page's dark background with the site's `.aurora` (brand.aurora_wash) and, when `glow` is on (behind the
    recording), the violet of its `.glow`. `strength` 1 is the site's own opacity (0.16); the cards use CARD_WASH times
    that. `grain` is the std. dev. of a little noise (brand.add_grain): the cards' wash is steep enough to band once it is
    encoded. Only the cards get it, because the encoder keeps noise and it costs file size."""
    bg = Image.new("RGB", (W, H), BACKGROUND)
    brand.aurora_wash(bg, strength)
    if glow:
        brand.wash(bg, lambda d: d.rectangle([-60, FRAME_Y + 10, W + 60, FRAME_Y + FRAME_H - 10], fill=255), AURORA[0], 0.26, 70)
    if grain:
        bg = brand.add_grain(bg, grain)
    return bg


def shadowed(layer, blur=12, off=8, alpha=0.5):
    a = layer.getchannel("A").point(lambda v: int(v * alpha))
    sh = Image.new("RGBA", layer.size, (0, 0, 0, 0))
    sh.putalpha(a)
    sh = sh.filter(ImageFilter.GaussianBlur(blur))
    out = Image.new("RGBA", layer.size, (0, 0, 0, 0))
    out.alpha_composite(sh, (0, off))
    out.alpha_composite(layer)
    return out


# ------------------------------------------------------------------ the logo
logo_mark = brand.logo_mark   # website/public/logo.svg, drawn in brand.py (the launch video draws it too)


def wordmark(layer, bg, cx, top, mark, size):
    """The site header's lockup: the mark, then "edytlab" in Inter SemiBold."""
    f = font("SemiBold", size)
    tw = f.getlength("edytlab")
    gap = round(mark * 0.28)
    x = cx - (mark + gap + tw) / 2
    layer.alpha_composite(logo_mark(mark), (round(x), round(top)))
    put_text(layer, bg, (x + mark + gap, top + mark / 2), "edytlab", f, FOREGROUND, "lm", "wordmark")


def gradient_text(layer, bg, xy, text, f, anchor="ls"):
    """Text filled with the site's `.gradient-text` (light violet to magenta-violet, left to right)."""
    m = Image.new("L", layer.size, 0)
    ImageDraw.Draw(m).text(xy, text, font=f, fill=255, anchor=anchor)
    box = m.getbbox()
    if not box:
        return
    check_contrast(bg, box, GRADIENT[1], "gradient title")   # the darker end is the one that matters
    w = box[2] - box[0]
    ramp = Image.new("L", (w, 1))
    ramp.putdata([round(255 * x / max(1, w - 1)) for x in range(w)])
    ramp = ramp.resize((w, box[3] - box[1]))
    fill = Image.composite(Image.new("RGB", ramp.size, GRADIENT[1]), Image.new("RGB", ramp.size, GRADIENT[0]), ramp)
    full = Image.new("RGB", layer.size, GRADIENT[0])
    full.paste(fill, (box[0], box[1]))
    layer.paste(full, (0, 0), m)


def wrap(text, f, max_w, max_lines=2):
    """Greedy line breaks; with two lines, the split that leaves them most even."""
    words = text.split()
    if f.getlength(text) <= max_w:
        return [text]
    best = None
    for k in range(1, len(words)):
        a, b = " ".join(words[:k]), " ".join(words[k:])
        w = max(f.getlength(a), f.getlength(b))
        if w <= max_w and (best is None or w < best[0]):
            best = (w, [a, b])
    if best is None or max_lines < 2:
        raise ValueError(f"does not fit {max_lines} line(s) of {max_w}px: {text}")
    return best[1]


# ------------------------------------------------------------------ cards
def hook_card(path, title, sub):
    bg = background(CARD_WASH, GRAIN, glow=False).convert("RGBA")
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    wordmark(layer, bg, W / 2, 330, 84, 64)
    size = 132
    while size > 64 and max(font("Bold", size).getlength(ln) for ln in title) > SAFE_R - SAFE_L:
        size -= 4
    f = font("Bold", size)
    lh = round(size * 1.1)
    top = 760 - lh * len(title) / 2
    for i, ln in enumerate(title):
        xy = (W / 2 - f.getlength(ln) / 2, top + i * lh + size)
        if i == len(title) - 1:
            gradient_text(layer, bg, xy, ln, f)
        else:
            put_text(layer, bg, xy, ln, f, FOREGROUND, "ls", "hook title")
    if sub:
        fs = font("Medium", 38)
        y = top + lh * len(title) + 40
        for ln in wrap(sub, fs, SAFE_R - SAFE_L):
            put_text(layer, bg, (W / 2, y), ln, fs, MUTED_FG, "ma", "hook sub line")
            y += 52
    assert_safe(layer, "hook card", top=SAFE_TOP)
    bg.alpha_composite(shadowed(layer, blur=8, off=6, alpha=0.45))
    bg.convert("RGB").save(path)


def end_card(path, free="Free, open source"):
    bg = background(CARD_WASH, GRAIN, glow=False).convert("RGBA")
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    wordmark(layer, bg, W / 2, 470, 150, 104)
    fsz = 84
    while font("Bold", fsz).getlength(free) > SAFE_R - SAFE_L:
        fsz -= 4
    put_text(layer, bg, (W / 2, 760), free, font("Bold", fsz), FOREGROUND, "ma", "end title")
    # the url in a pill with the site's primary ring (`ring-hover`: primary at 45 %)
    fu = font("SemiBold", 88)
    tw = fu.getlength(SITE_URL)
    x0, x1 = W / 2 - tw / 2 - 60, W / 2 + tw / 2 + 60
    d.rounded_rectangle([x0, 930, x1, 1070], radius=70, fill=rgba(CARD, 235), outline=rgba(PRIMARY, 200), width=4)
    d.text((W / 2, 1000), SITE_URL, font=fu, fill=rgba(PRIMARY), anchor="mm")
    put_text(layer, bg, (W / 2, 1150), "macOS  ·  Windows  ·  Linux", font("Medium", 46), MUTED_FG, "ma", "end platforms")
    assert_safe(layer, "end card", top=SAFE_TOP)
    bg.alpha_composite(shadowed(layer, blur=8, off=6, alpha=0.45))
    bg.convert("RGB").save(path)


# ------------------------------------------------------------- caption stills
def explode(tokens):
    """tokens -> parts (text, token index, glued): a long hyphenated name, such as a file name, may break after a hyphen."""
    parts = []
    for i, tok in enumerate(tokens):
        pieces = re.findall(r"[^-]+-?", tok) if len(tok) > 16 and "-" in tok.strip("-") else [tok]
        for k, piece in enumerate(pieces):
            parts.append((piece, i, k > 0))
    return parts


MAX_TEXT_W = SAFE_R - SAFE_L - 2 * CARD_PAD_X - EDGE_W


def layout(tokens, max_w=MAX_TEXT_W):
    """Best 1-2 line layout at the largest Inter SemiBold size in 60..44 px. -> (font, parts, [(first, end)])"""
    parts = explode(tokens)
    for size in range(60, 43, -2):
        f = font("SemiBold", size)
        sp = f.getlength(" ")
        ws = [f.getlength(p[0]) for p in parts]

        def span(a, b):
            return sum(ws[a:b]) + sum(sp for k in range(a + 1, b) if not parts[k][2])

        if span(0, len(parts)) <= max_w:
            return f, parts, [(0, len(parts))]
        best = None
        for k in range(1, len(parts)):
            w1, w2 = span(0, k), span(k, len(parts))
            if max(w1, w2) <= max_w and (best is None or max(w1, w2) < best[0]):
                best = (max(w1, w2), k)
        if best:
            return f, parts, [(0, best[1]), (best[1], len(parts))]
    raise ValueError("caption does not fit 2 lines: " + " ".join(tokens))


def caption_image(tokens, accent_n, cur=None, alpha=1.0, mode="plain"):
    """The reel's own caption: Inter SemiBold on the site's card with a violet edge, as in the demos.
    tokens: display words. mode plain: the first accent_n words violet, the rest white.
    mode words: idx<cur white, idx==cur violet, idx>cur dim (cur=-1: all dim)."""
    f, parts, lines = layout(tokens)
    sp = f.getlength(" ")
    asc, desc = f.getmetrics()
    lh = round((asc + desc) * 1.08)

    def width(a, b):
        return sum(f.getlength(parts[k][0]) for k in range(a, b)) + sum(sp for k in range(a + 1, b) if not parts[k][2])

    block_w = max(width(a, b) for a, b in lines)
    block_h = asc + desc + (len(lines) - 1) * lh
    card_w, card_h = round(block_w) + 2 * CARD_PAD_X + EDGE_W, block_h + 2 * CARD_PAD_Y
    x0, y0 = round((W - card_w) / 2), round((CAP_H - card_h) / 2)
    layer = Image.new("RGBA", (W, CAP_H), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    box = [x0, y0, x0 + card_w, y0 + card_h]
    d.rounded_rectangle(box, radius=CARD_R, fill=rgba(CARD, 240), outline=rgba(BORDER), width=2)
    # the violet edge: a bar down the left side, cut to the card's rounded shape
    shape = Image.new("L", layer.size, 0)
    ImageDraw.Draw(shape).rounded_rectangle(box, radius=CARD_R, fill=255)
    edge = Image.new("L", layer.size, 0)
    ImageDraw.Draw(edge).rectangle([x0, y0, x0 + EDGE_W - 1, y0 + card_h], fill=255)
    layer.paste(Image.new("RGBA", layer.size, rgba(PRIMARY)), (0, 0), ImageChops.multiply(shape, edge))
    for li, (a, b) in enumerate(lines):
        x = x0 + EDGE_W + CARD_PAD_X + (block_w - width(a, b)) / 2
        base = y0 + CARD_PAD_Y + asc + li * lh
        for k in range(a, b):
            text, ti, glued = parts[k]
            if k > a and not glued:
                x += sp
            if mode == "words":
                col = FOREGROUND + (255,) if ti < cur else PRIMARY + (255,) if ti == cur else FOREGROUND + (120,)
            else:
                col = rgba(PRIMARY) if ti < accent_n else rgba(FOREGROUND)
            d.text((x, base), text, font=f, fill=col, anchor="ls")
            x += f.getlength(text)
    assert_safe(layer, "caption", dy=CAP_Y)
    layer = shadowed(layer, blur=14, off=8, alpha=0.5)
    if alpha < 1.0:
        layer.putalpha(layer.getchannel("A").point(lambda v: int(v * alpha)))
    return layer


ACCENT_RE = [re.compile(r"^\d+ ·"), re.compile(r"^(Listen|Done|Before):")]


def accent_count(text):
    for rx in ACCENT_RE:
        m = rx.match(text)
        if m:
            return len(m.group(0).split())
    return 0


def blank_caption(path):
    Image.new("RGBA", (W, CAP_H), (0, 0, 0, 0)).save(path)


# ------------------------------------------------------------ group layers
def group_layer(path, bg, pill_text, note=True, note_text="Real screen recording · waits sped up"):
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    if pill_text:
        f = font("SemiBold", 34)
        tw = f.getlength(pill_text)
        x0, x1 = (W - tw) / 2 - 44, (W + tw) / 2 + 44
        d.rounded_rectangle([x0, PILL_Y, x1, PILL_Y + 66], radius=33, fill=rgba(CARD, 240), outline=rgba(PRIMARY, 115), width=3)
        d.text((W / 2, PILL_Y + 33), pill_text, font=f, fill=rgba(FOREGROUND), anchor="mm")
    if note:
        put_text(layer, bg, (W / 2, NOTE_Y), note_text, font("Medium", 28), MUTED_FG, "ma", "note")
    # progress track
    d.rounded_rectangle([SAFE_L, BAR_Y, SAFE_R - 1, BAR_Y + 5], radius=3, fill=rgba(FOREGROUND, 36))
    assert_safe(layer, "pill, note and progress track")
    layer.save(path)


def bar_image(path):
    im = Image.new("RGBA", (SAFE_R - SAFE_L, 6), (0, 0, 0, 0))
    ImageDraw.Draw(im).rounded_rectangle([0, 0, SAFE_R - SAFE_L - 1, 5], radius=3, fill=rgba(PRIMARY))
    im.save(path)


# --------------------------------------------------------------- build graph
def fr(t):
    return int(round(t * FPS))


def caption_timeline(tl, work, mode, words):
    """-> list of (png, n_frames) covering [0, demo_duration + 1 s)."""
    segs = [c for c in tl["cues"] if c["kind"] in ("step", "listen")]
    t0 = tl["cues"][0]["end"] if tl["cues"][0]["kind"] == "hook" else 0.0
    t_end = [c for c in tl["cues"] if c["kind"] == "end"][0]["start"] if any(c["kind"] == "end" for c in tl["cues"]) else tl["duration"]
    os.makedirs(f"{work}/caps", exist_ok=True)
    blank = f"{work}/caps/blank.png"
    blank_caption(blank)
    out = []
    cursor = 0  # frames, demo-local
    n = 0
    for c in segs:
        a, b = fr(c["start"] - t0), fr(c["end"] - t0)
        if a > cursor:
            out.append((blank, a - cursor))
        use_words = mode == "words" and c["kind"] == "step" and c.get("id") in words
        if use_words:
            ws = words[c["id"]]
            tokens = [w["text"] for w in ws]
            states = [(-1, 0.0)] + [(i, w["start"]) for i, w in enumerate(ws)]
            last_end = ws[-1]["end"]
            states.append((len(ws), last_end + 0.25))
            times = [c["start"] - t0 + s for _, s in states]
            cur_list = [i for i, _ in states]
            acc = 0
        else:
            tokens = c["onscreen"].split()
            acc = accent_count(c["onscreen"])
            times = [c["start"] - t0]
            cur_list = [None]
        for si, cur in enumerate(cur_list):
            sa = max(a, fr(times[si]))
            sb = min(b, fr(times[si + 1])) if si + 1 < len(times) else b
            if si == 0:
                sa = a
            if sb <= sa:
                continue
            png = f"{work}/caps/c{n:04d}.png"; n += 1
            if si == 0:  # fade-in ramp over the first 3 frames
                for k, al in enumerate((0.34, 0.67)):
                    p2 = f"{work}/caps/c{n:04d}.png"; n += 1
                    caption_image(tokens, acc, cur if cur is not None else -1, al, "words" if use_words else "plain").save(p2)
                    out.append((p2, 1))
                sa += 2
                if sb <= sa:
                    continue
            caption_image(tokens, acc, cur if cur is not None else -1, 1.0, "words" if use_words else "plain").save(png)
            out.append((png, sb - sa))
        cursor = b
    total = fr(t_end - t0) + FPS
    if total > cursor:
        out.append((blank, total - cursor))
    return out


def write_concat(path, items):
    with open(path, "w") as f:
        f.write("ffconcat version 1.0\n")
        for png, n in items:
            f.write(f"file '{png}'\nduration {n / FPS:.6f}\n")
        f.write(f"file '{items[-1][0]}'\n")


def render_video(tl, mode="plain", words=None, out_path=None, crf=20):
    name = tl["name"]
    r = REELS[name]
    work = f"{WORK}/{tl['file']}-{mode}"
    os.makedirs(work, exist_ok=True)
    words = words or {}
    cues = tl["cues"]
    hook = next((c for c in cues if c["kind"] == "hook"), None)
    end = next((c for c in cues if c["kind"] == "end"), None)
    segs = [c for c in cues if c["kind"] in ("step", "listen")]
    t0 = hook["end"] if hook else 0.0
    demo_dur = (end["start"] if end else tl["duration"]) - t0
    demo_frames = fr(demo_dur)

    inputs, idx = [], 0
    if hook:
        hp = f"{work}/hook.png"
        hook_card(hp, hook["title"], hook.get("sub", ""))
        inputs += ["-loop", "1", "-framerate", str(FPS), "-t", f"{fr(hook['end']-hook['start'])/FPS:.4f}", "-i", hp]
        hidx = idx; idx += 1
    sidx = []
    for c in segs:
        s = c["source"]
        inputs += ["-ss", f"{s['start']:.3f}", "-t", f"{(c['end']-c['start'])+0.5:.3f}", "-i", f"{DEMO_DIR}/{s['demo']}.mp4"]
        sidx.append(idx); idx += 1
    if end:
        ep = f"{work}/end.png"
        end_card(ep, ES_TEXT["free"] if tl.get("lang") == "es" else "Free, open source")
        inputs += ["-loop", "1", "-framerate", str(FPS), "-t", f"{fr(end['end']-end['start'])/FPS:.4f}", "-i", ep]
        eidx = idx; idx += 1
    # the background the recording sits on
    bp = f"{work}/bg.png"
    bgimg = background()
    bgimg.save(bp)
    inputs += ["-loop", "1", "-framerate", str(FPS), "-t", f"{demo_dur+1:.3f}", "-i", bp]
    bgidx = idx; idx += 1
    bar = f"{work}/bar.png"
    bar_image(bar)
    # caption layer
    items = caption_timeline(tl, work, mode, words)
    cl = f"{work}/caps.txt"
    write_concat(cl, items)
    inputs += ["-f", "concat", "-safe", "0", "-i", cl]
    cidx = idx; idx += 1
    # group layers (pill per demo run, note everywhere)
    groups = []  # (start, end, pill)
    for c in segs:
        code = c["source"]["code"]
        pill = (ES_TEXT["pill"][code] if tl.get("lang") == "es" else DEMOS[code]["title"]) if tl["pills"] else ""
        a, b = c["start"] - t0, c["end"] - t0
        if groups and groups[-1][2] == pill:
            groups[-1][1] = b
        else:
            groups.append([a, b, pill])
    gidx = []
    for gi, (a, b, pill) in enumerate(groups):
        gp = f"{work}/group{gi}.png"
        group_layer(gp, bgimg, pill, True, ES_TEXT["note"] if tl.get("lang") == "es" else "Real screen recording · waits sped up")
        inputs += ["-loop", "1", "-framerate", str(FPS), "-t", f"{demo_dur+1:.3f}", "-i", gp]
        gidx.append(idx); idx += 1
    inputs += ["-loop", "1", "-framerate", str(FPS), "-t", f"{demo_dur+1:.3f}", "-i", bar]
    baridx = idx; idx += 1

    f = []
    # demo cuts -> exact frame counts
    labels = []
    for k, (c, i) in enumerate(zip(segs, sidx)):
        nf = fr(c["end"] - c["start"])
        f.append(f"[{i}:v]fps={FPS}:round=near,scale={FRAME_W}:{FRAME_H}:flags=lanczos,setsar=1,format=yuv420p,"
                 f"tpad=stop_mode=clone:stop=4,trim=end_frame={nf},setpts=N/({FPS}*TB)[v{k}]")
        labels.append(f"[v{k}]")
    f.append("".join(labels) + f"concat=n={len(labels)}:v=1:a=0[vc]")
    f.append(f"[{bgidx}:v]fps={FPS},format=yuv420p,setsar=1[bg]")
    f.append(f"[bg][vc]overlay=0:{FRAME_Y}:format=auto[b1]")
    f.append(f"[{cidx}:v]fps={FPS},format=rgba,setsar=1[cap]")
    f.append(f"[b1][cap]overlay=0:{CAP_Y}:format=auto:eof_action=pass[b2]")
    cur = "b2"
    for gi, ((a, b, pill), i) in enumerate(zip(groups, gidx)):
        f.append(f"[{i}:v]format=rgba,setsar=1[gl{gi}]")
        f.append(f"[{cur}][gl{gi}]overlay=0:0:format=auto:enable='between(t,{a:.3f},{b:.3f})':eof_action=pass[g{gi}]")
        cur = f"g{gi}"
    f.append(f"[{baridx}:v]format=rgba,setsar=1[bar]")
    f.append(f"color=c=0x00000000:s={SAFE_R-SAFE_L}x6:r={FPS},format=rgba[bcan]")
    f.append(f"[bcan][bar]overlay=x='-w+w*t/{demo_dur:.3f}':y=0:eval=frame:format=auto:shortest=1[bfill]")
    f.append(f"[{cur}][bfill]overlay={SAFE_L}:{BAR_Y}:format=auto:eof_action=pass,setsar=1,format=yuv420p,trim=end_frame={demo_frames},setpts=N/({FPS}*TB)[demo]")
    parts = []
    if hook:   # no fade in: the first frame is the card, which is what a feed shows before it plays
        hf = fr(hook["end"] - hook["start"])
        f.append(f"[{hidx}:v]fps={FPS},scale={W}:{H},setsar=1,format=yuv420p,trim=end_frame={hf},setpts=N/({FPS}*TB)[hk]")
        parts.append("[hk]")
    parts.append("[demo]")
    if end:
        ef = fr(end["end"] - end["start"])
        f.append(f"[{eidx}:v]fps={FPS},scale={W}:{H},setsar=1,format=yuv420p,"
                 f"fade=t=out:st={ef/FPS-0.25:.3f}:d=0.25,trim=end_frame={ef},setpts=N/({FPS}*TB),setsar=1,format=yuv420p[ed]")
        parts.append("[ed]")
    f.append("".join(parts) + f"concat=n={len(parts)}:v=1:a=0[outv]")
    script = f"{work}/graph.txt"
    open(script, "w").write(";\n".join(f))
    out_path = out_path or f"{work}/video.mp4"
    cmd = ["ffmpeg", "-nostdin", "-y", "-v", "error"] + inputs + ["-filter_complex_script", script, "-map", "[outv]", "-an",
           "-c:v", "libx264", "-preset", "medium", "-crf", str(crf), "-profile:v", "high", "-level", "4.2", "-pix_fmt", "yuv420p",
           "-x264-params", X264_PARAMS, "-r", str(FPS), "-g", "60", "-threads", THREADS, "-movflags", "+faststart", out_path]
    run(cmd)
    return out_path


# ----------------------------------------------------------------- app audio
def demo_wav(slug):
    """Whole-file 48 kHz stereo decode with gaps filled, so absolute times line up with the picture."""
    os.makedirs(f"{WORK}/audio", exist_ok=True)
    p = f"{WORK}/audio/{slug}.wav"
    if not os.path.exists(p):
        run(["ffmpeg", "-nostdin", "-y", "-v", "error", "-i", f"{DEMO_DIR}/{slug}.mp4", "-vn", "-af",
             "aresample=48000:async=1:first_pts=0", "-ac", "2", "-c:a", "pcm_s16le", p])
    return p


def render_app_audio(tl, out_wav):
    """Concatenate the demo audio of every cut (silence under the cards) to 48 kHz stereo wav."""
    name = tl["file"]
    inputs, f, labels = [], [], []
    for k, c in enumerate(tl["cues"]):
        d = fr(c["end"] - c["start"]) / FPS
        if c["kind"] in ("hook", "end"):
            f.append(f"anullsrc=r=48000:cl=stereo,atrim=duration={d:.4f},asetpts=PTS-STARTPTS[a{k}]")
        else:
            s = c["source"]
            i = len(inputs) // 6
            inputs += ["-ss", f"{s['start']:.3f}", "-t", f"{d + 0.5:.3f}", "-i", demo_wav(s["demo"])]
            fo = 0.35 if c["kind"] == "listen" else 0.02
            f.append(f"[{i}:a]aformat=sample_fmts=fltp:channel_layouts=stereo,asetpts=PTS-STARTPTS,apad=whole_dur={d:.4f},"
                     f"atrim=end={d:.4f},afade=t=in:d=0.04,afade=t=out:st={d-fo:.4f}:d={fo}[a{k}]")
        labels.append(f"[a{k}]")
    f.append("".join(labels) + f"concat=n={len(labels)}:v=0:a=1[aout]")
    script = f"{WORK}/{name}-audio-graph.txt"
    open(script, "w").write(";\n".join(f))
    run(["ffmpeg", "-nostdin", "-y", "-v", "error"] + inputs + ["-filter_complex_script", script, "-map", "[aout]",
        "-c:a", "pcm_s16le", "-ar", "48000", out_wav])
    return out_wav


def measure_loudness(path):
    p = subprocess.run(["nice", "-n", "10", "ffmpeg", "-nostdin", "-hide_banner", "-i", path, "-vn", "-af", "ebur128=peak=true", "-f", "null", "-"],
                       capture_output=True, text=True)
    tail = p.stderr[p.stderr.rindex("Summary:"):]
    g = lambda rx: float(re.search(rx, tail).group(1))
    return dict(integrated_lufs=g(r"I:\s+(-?[\d.]+) LUFS"), lra=g(r"LRA:\s+([\d.]+) LU"), true_peak_dbtp=g(r"Peak:\s+(-?[\d.]+) dBFS"))


def loudnorm_two_pass(in_wav, out_m4a, target=-14.0, tp=-1.0, lra=11, max_peak=-1.1, tol=0.25):
    """Bring in_wav to `target` LUFS integrated with the encoded AAC at or under `max_peak` dBTP.
    Plain loudnorm cannot raise mastered music (peaks near 0 dBFS) to -14 LUFS without touching
    the peaks, so: measure, apply gain, catch the peaks with a lookahead limiter, encode, then
    measure the AAC itself and iterate gain/ceiling until both numbers hold."""
    p = run(["ffmpeg", "-nostdin", "-hide_banner", "-i", in_wav, "-af", "ebur128=peak=true", "-f", "null", "-"])
    tail = p.stderr[p.stderr.rindex("Summary:"):]
    i0 = float(re.search(r"I:\s+(-?[\d.]+) LUFS", tail).group(1))
    tp0 = float(re.search(r"Peak:\s+(-?[\d.]+) dBFS", tail).group(1))
    gain, ceil_db = target - i0, max_peak - 0.6
    res = None
    for _ in range(8):
        lim = 10 ** (ceil_db / 20)
        flt = f"volume={gain:.3f}dB,alimiter=limit={lim:.4f}:attack=4:release=60:level=0,aresample=48000"
        run(["ffmpeg", "-nostdin", "-y", "-v", "error", "-i", in_wav, "-af", flt, "-c:a", "aac", "-b:a", "192k", "-ar", "48000", "-ac", "2", out_m4a])
        res = measure_loudness(out_m4a)
        ok_i = abs(res["integrated_lufs"] - target) <= tol
        ok_p = res["true_peak_dbtp"] <= max_peak
        if ok_i and ok_p:
            break
        if not ok_i:
            gain += (target - res["integrated_lufs"]) * 0.9
        if not ok_p:
            ceil_db -= (res["true_peak_dbtp"] - max_peak) + 0.1
    return dict(input_i=i0, input_tp=tp0, gain_db=round(gain, 2), ceiling_db=round(ceil_db, 2), result=res)


def mux(video, audio, out):
    run(["ffmpeg", "-nostdin", "-y", "-v", "error", "-i", video, "-i", audio, "-map", "0:v:0", "-map", "1:a:0", "-c", "copy",
         "-movflags", "+faststart", "-shortest", out])


def contact_sheet(video, tl, out_png, cols=4, rows=3, tw=270):
    """12 labelled frames: the middle of each cue (evenly thinned if more than 12)."""
    cues = tl["cues"]
    pts = [(c["id"], (c["start"] + c["end"]) / 2) for c in cues]
    if len(pts) > cols * rows:
        step = len(pts) / (cols * rows)
        pts = [pts[int(i * step)] for i in range(cols * rows)]
    elif len(pts) < cols * rows:  # fill with extra points in the longest cues
        extra = sorted(cues, key=lambda c: c["end"] - c["start"], reverse=True)
        i = 0
        while len(pts) < cols * rows:
            c = extra[i % len(extra)]
            pts.append((c["id"], c["start"] + (c["end"] - c["start"]) * 0.2)); i += 1
        pts.sort(key=lambda p: p[1])
    th = round(tw * 16 / 9)
    sheet = Image.new("RGB", (cols * tw, rows * th), (0, 0, 0))
    d = ImageDraw.Draw(sheet)
    f = font("SemiBold", 18)
    for i, (cid, t) in enumerate(pts):
        p = subprocess.run(["nice", "-n", "10", "ffmpeg", "-nostdin", "-v", "error", "-ss", f"{t:.3f}", "-i", video, "-frames:v", "1",
                            "-f", "image2pipe", "-vcodec", "png", "-"], capture_output=True)
        im = Image.open(io.BytesIO(p.stdout)).convert("RGB").resize((tw, th), Image.LANCZOS)
        x, y = (i % cols) * tw, (i // cols) * th
        sheet.paste(im, (x, y))
        d.rectangle([x, y, x + 120, y + 24], fill=(0, 0, 0))
        d.text((x + 4, y + 2), f"{cid} {t:5.1f}s", font=f, fill=(255, 255, 0))
    sheet.save(out_png)


