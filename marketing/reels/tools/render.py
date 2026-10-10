"""Render the 1080x1920 reels: PIL draws the cards and caption stills, one ffmpeg
filter graph does the compositing. Everything runs under `nice -n 10`."""
import io, json, math, os, re, subprocess, sys
from PIL import Image, ImageDraw, ImageEnhance, ImageFilter, ImageFont
from spec import *
from cues import build_reel, FPS

W, H = 1080, 1920
FRAME_W, FRAME_H, FRAME_Y = 1080, 654, 720
CAP_Y, CAP_H = 430, 270
PILL_Y = 366
NOTE_Y = 1402
BAR_Y = 1458
ACCENT = (242, 140, 60)
FONT_B = "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf"
FONT_SERIF_BI = "/usr/share/fonts/truetype/liberation/LiberationSerif-BoldItalic.ttf"
WORK = f"{ROOT}/work"
THREADS = os.environ.get("REEL_THREADS", "3")


def run(cmd, **kw):
    cmd = ["nice", "-n", "10"] + cmd
    p = subprocess.run(cmd, capture_output=True, text=True, **kw)
    if p.returncode != 0:
        raise RuntimeError("command failed: " + " ".join(cmd[:6]) + "\n" + p.stderr[-2500:])
    return p


def font(size, path=FONT_B):
    return ImageFont.truetype(path, size)


def grab(slug, t):
    p = subprocess.run(["nice", "-n", "10", "ffmpeg", "-nostdin", "-v", "error", "-ss", f"{t:.3f}", "-i", f"{DEMO_DIR}/{slug}.mp4",
                        "-frames:v", "1", "-f", "image2pipe", "-vcodec", "png", "-"], capture_output=True)
    return Image.open(io.BytesIO(p.stdout)).convert("RGB")


# ------------------------------------------------------------------ cards
def blurred_bg(im, dark=0.30):
    w, h = im.size
    cw = round(h * 9 / 16)
    x0 = (w - cw) // 2
    c = im.crop((x0, 0, x0 + cw, h)).resize((270, 480), Image.LANCZOS).filter(ImageFilter.GaussianBlur(5))
    c = c.resize((W, H), Image.BICUBIC)
    c = ImageEnhance.Brightness(c).enhance(dark)
    c = ImageEnhance.Color(c).enhance(0.8)
    # vertical vignette
    g = Image.new("L", (1, H))
    for y in range(H):
        d = abs(y - H / 2) / (H / 2)
        g.putpixel((0, y), int(255 * (0.55 + 0.45 * (1 - d ** 2))))
    g = g.resize((W, H))
    return Image.composite(c, Image.new("RGB", (W, H), (6, 8, 14)), g)


def text_w(f, s):
    return f.getlength(s)


def draw_center(d, y, s, f, fill, w=W):
    d.text(((w - text_w(f, s)) / 2, y), s, font=f, fill=fill)


def shadowed(layer, blur=4, off=4, alpha=0.55):
    a = layer.getchannel("A").point(lambda v: int(v * alpha))
    sh = Image.new("RGBA", layer.size, (0, 0, 0, 0))
    sh.putalpha(a)
    sh = sh.filter(ImageFilter.GaussianBlur(blur))
    out = Image.new("RGBA", layer.size, (0, 0, 0, 0))
    out.alpha_composite(sh, (0, off))
    out.alpha_composite(layer)
    return out


def wordmark(d, cx, y, size, fill_lab=(255, 255, 255)):
    fa, fb = font(size, FONT_SERIF_BI), font(size)
    wa, wb = text_w(fa, "edyt"), text_w(fb, "lab")
    x = cx - (wa + wb) / 2
    d.text((x, y), "edyt", font=fa, fill=ACCENT)
    d.text((x + wa, y), "lab", font=fb, fill=fill_lab)


def hook_card(path, title, sub, bg_im):
    bg = blurred_bg(bg_im, 0.62).convert("RGBA")
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    wordmark(d, W / 2, 330, 78)
    size = 132
    while size > 70:
        f = font(size)
        if max(text_w(f, ln) for ln in title) <= 940:
            break
        size -= 4
    f = font(size)
    lh = int(size * 1.16)
    top = 760 - lh * len(title) / 2
    for i, ln in enumerate(title):
        draw_center(d, top + i * lh, ln, f, ACCENT if i == len(title) - 1 else (255, 255, 255))
    y = top + lh * len(title) + 36
    if sub:
        fs = font(40)
        draw_center(d, y, sub, fs, (255, 255, 255, 190))
    layer = shadowed(layer, blur=6, off=5, alpha=0.5)
    bg.alpha_composite(layer)
    bg.convert("RGB").save(path)


def end_card(path, bg_im, free="Free, open source"):
    bg = blurred_bg(bg_im, 0.58).convert("RGBA")
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    wordmark(d, W / 2, 520, 168)
    fsz = 84
    while text_w(font(fsz), free) > 940:
        fsz -= 4
    draw_center(d, 760, free, font(fsz), (255, 255, 255))
    # url pill
    fu = font(92)
    s = SITE_URL
    tw = text_w(fu, s)
    px0, px1 = (W - tw) / 2 - 56, (W + tw) / 2 + 56
    d.rounded_rectangle([px0, 930, px1, 1066], radius=68, outline=ACCENT + (255,), width=5, fill=(0, 0, 0, 90))
    d.text(((W - tw) / 2, 930 + 18), s, font=fu, fill=ACCENT + (255,))
    draw_center(d, 1150, "macOS  ·  Windows  ·  Linux", font(50), (255, 255, 255, 220))
    layer = shadowed(layer, blur=6, off=5, alpha=0.5)
    bg.alpha_composite(layer)
    bg.convert("RGB").save(path)


# ------------------------------------------------------------- caption stills
def layout(tokens, max_w=960):
    """Best 1-2 line layout at the largest size in 64..50 px. -> (font, [[idx]])"""
    for size in range(64, 49, -2):
        f = font(size)
        sp = text_w(f, " ")
        ws = [text_w(f, t) for t in tokens]
        total = sum(ws) + sp * (len(ws) - 1)
        if total <= max_w:
            return f, [list(range(len(tokens)))]
        best = None
        for k in range(1, len(tokens)):
            w1 = sum(ws[:k]) + sp * (k - 1)
            w2 = sum(ws[k:]) + sp * (len(ws) - k - 1)
            if max(w1, w2) <= max_w and (best is None or max(w1, w2) < best[0]):
                best = (max(w1, w2), k)
        if best:
            k = best[1]
            return f, [list(range(k)), list(range(k, len(tokens)))]
    raise ValueError("caption does not fit 2 lines: " + " ".join(tokens))


def caption_image(tokens, accent_n, cur=None, alpha=1.0, mode="plain"):
    """tokens: display words. mode plain: all white (first accent_n accent).
    mode words: idx<cur white, idx==cur accent, idx>cur dim white (cur=-1: all dim)."""
    f, lines = layout(tokens)
    layer = Image.new("RGBA", (W, CAP_H), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    sp = text_w(f, " ")
    lh = int(f.size * 1.2)
    top = (CAP_H - lh * len(lines)) / 2 + 4
    for li, idxs in enumerate(lines):
        total = sum(text_w(f, tokens[i]) for i in idxs) + sp * (len(idxs) - 1)
        x = (W - total) / 2
        for i in idxs:
            if mode == "words":
                if i < cur:
                    col = (255, 255, 255, 255)
                elif i == cur:
                    col = ACCENT + (255,)
                else:
                    col = (255, 255, 255, 120)
            else:
                col = ACCENT + (255,) if i < accent_n else (255, 255, 255, 255)
            d.text((x, top + li * lh), tokens[i], font=f, fill=col)
            x += text_w(f, tokens[i]) + sp
    layer = shadowed(layer, blur=4, off=4, alpha=0.6)
    if alpha < 1.0:
        layer.putalpha(layer.getchannel("A").point(lambda v: int(v * alpha)))
    return layer


ACCENT_RE = [re.compile(r"^\d+ ·"), re.compile(r"^(Listen|Done|Before):")]


def accent_count(text):
    toks = text.split()
    for rx in ACCENT_RE:
        m = rx.match(text)
        if m:
            return len(m.group(0).split())
    return 0


def blank_caption(path):
    Image.new("RGBA", (W, CAP_H), (0, 0, 0, 0)).save(path)


# ------------------------------------------------------------ group layers
def group_layer(path, pill_text, note=True, note_text="Real screen recording · waits sped up"):
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    if pill_text:
        f = font(34)
        tw = text_w(f, pill_text)
        x0, x1 = (W - tw) / 2 - 44, (W + tw) / 2 + 44
        d.rounded_rectangle([x0, PILL_Y, x1, PILL_Y + 66], radius=33, fill=(0, 0, 0, 120), outline=ACCENT + (230,), width=3)
        d.text(((W - tw) / 2, PILL_Y + 12), pill_text, font=f, fill=(255, 255, 255, 255))
    if note:
        f = font(27)
        draw_center(d, NOTE_Y, note_text, f, (255, 255, 255, 150))
    # progress track
    d.rounded_rectangle([60, BAR_Y, W - 60, BAR_Y + 6], radius=3, fill=(255, 255, 255, 36))
    layer.save(path)


def bar_image(path):
    im = Image.new("RGBA", (W - 120, 6), (0, 0, 0, 0))
    ImageDraw.Draw(im).rounded_rectangle([0, 0, W - 121, 5], radius=3, fill=ACCENT + (255,))
    im.save(path)


def mask_image(path):
    m = Image.new("L", (FRAME_W, FRAME_H), 0)
    ImageDraw.Draw(m).rounded_rectangle([0, 0, FRAME_W - 1, FRAME_H - 1], radius=22, fill=255)
    m.save(path)


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

    # cards: background frame from the demo the reel is about
    demo_code = r.get("demo")
    bg_src = (DEMOS[demo_code]["slug"], 85.0) if demo_code == "BT" else (DEMOS[demo_code]["slug"], 58.0) if demo_code else ("dj-mini-mix", 58.0)
    if name == "highlights":
        bg_src = ("dj-beatmatched-transition", 85.0)
    if name == "teaser":
        bg_src = ("dj-beatmatched-transition", 85.0)
    if demo_code == "MM":
        bg_src = ("dj-mini-mix", 58.0)
    if demo_code == "EI":
        bg_src = ("dj-extended-club-intro", 66.0)
    bgim = grab(*bg_src)
    inputs, fparts = [], []
    idx = 0
    if hook:
        hp = f"{work}/hook.png"
        hook_card(hp, hook["title"], hook.get("sub", ""), bgim)
        inputs += ["-loop", "1", "-framerate", str(FPS), "-t", f"{fr(hook['end']-hook['start'])/FPS:.4f}", "-i", hp]
        hidx = idx; idx += 1
    sidx = []
    for c in segs:
        s = c["source"]
        inputs += ["-ss", f"{s['start']:.3f}", "-t", f"{(c['end']-c['start'])+0.5:.3f}", "-i", f"{DEMO_DIR}/{s['demo']}.mp4"]
        sidx.append(idx); idx += 1
    if end:
        ep = f"{work}/end.png"
        end_card(ep, bgim, ES_TEXT["free"] if tl.get("lang") == "es" else "Free, open source")
        inputs += ["-loop", "1", "-framerate", str(FPS), "-t", f"{fr(end['end']-end['start'])/FPS:.4f}", "-i", ep]
        eidx = idx; idx += 1
    mp, bp = f"{work}/mask.png", f"{work}/bar.png"
    mask_image(mp); bar_image(bp)
    inputs += ["-loop", "1", "-framerate", str(FPS), "-t", f"{demo_dur+1:.3f}", "-i", mp]
    midx = idx; idx += 1
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
        group_layer(gp, pill, True, ES_TEXT["note"] if tl.get("lang") == "es" else "Real screen recording · waits sped up")
        inputs += ["-loop", "1", "-framerate", str(FPS), "-t", f"{demo_dur+1:.3f}", "-i", gp]
        gidx.append(idx); idx += 1
    inputs += ["-loop", "1", "-framerate", str(FPS), "-t", f"{demo_dur+1:.3f}", "-i", bp]
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
    f.append("[vc]split=2[fgsrc][bgsrc]")
    f.append("[bgsrc]crop=1080:520:0:0,scale=-2:480,crop=270:480,gblur=sigma=5,scale=1080:1920:flags=bicubic,eq=brightness=0.02:contrast=1.25:saturation=1.1,setsar=1,format=yuv420p[bg]")
    f.append(f"[fgsrc]format=rgba[fgc];[{midx}:v]format=gray,setsar=1[mk];[fgc][mk]alphamerge[fg]")
    f.append(f"[bg][fg]overlay=0:{FRAME_Y}:format=auto[b1]")
    f.append(f"[{cidx}:v]fps={FPS},format=rgba,setsar=1[cap]")
    f.append(f"[b1][cap]overlay=0:{CAP_Y}:format=auto:eof_action=pass[b2]")
    cur = "b2"
    for gi, ((a, b, pill), i) in enumerate(zip(groups, gidx)):
        f.append(f"[{i}:v]format=rgba,setsar=1[gl{gi}]")
        f.append(f"[{cur}][gl{gi}]overlay=0:0:format=auto:enable='between(t,{a:.3f},{b:.3f})':eof_action=pass[g{gi}]")
        cur = f"g{gi}"
    f.append(f"[{baridx}:v]format=rgba,setsar=1[bar]")
    f.append(f"color=c=0x00000000:s={W-120}x6:r={FPS},format=rgba[bcan]")
    f.append(f"[bcan][bar]overlay=x='-w+w*t/{demo_dur:.3f}':y=0:eval=frame:format=auto:shortest=1[bfill]")
    f.append(f"[{cur}][bfill]overlay=60:{BAR_Y}:format=auto:eof_action=pass,setsar=1,format=yuv420p,trim=end_frame={demo_frames},setpts=N/({FPS}*TB)[demo]")
    parts = []
    if hook:
        hf = fr(hook["end"] - hook["start"])
        f.append(f"[{hidx}:v]fps={FPS},scale={W}:{H},setsar=1,format=yuv420p,"
                 f"zoompan=z='min(1+0.0006*on,1.05)':x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':d=1:s={W}x{H}:fps={FPS},"
                 f"fade=t=in:st=0:d=0.2,trim=end_frame={hf},setpts=N/({FPS}*TB),setsar=1,format=yuv420p[hk]")
        parts.append("[hk]")
    parts.append("[demo]")
    if end:
        ef = fr(end["end"] - end["start"])
        f.append(f"[{eidx}:v]fps={FPS},scale={W}:{H},setsar=1,format=yuv420p,"
                 f"zoompan=z='min(1+0.0006*on,1.05)':x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':d=1:s={W}x{H}:fps={FPS},"
                 f"fade=t=in:st=0:d=0.2,fade=t=out:st={ef/FPS-0.25:.3f}:d=0.25,trim=end_frame={ef},setpts=N/({FPS}*TB),setsar=1,format=yuv420p[ed]")
        parts.append("[ed]")
    f.append("".join(parts) + f"concat=n={len(parts)}:v=1:a=0[outv]")
    script = f"{work}/graph.txt"
    open(script, "w").write(";\n".join(f))
    out_path = out_path or f"{work}/video.mp4"
    cmd = ["ffmpeg", "-nostdin", "-y", "-v", "error"] + inputs + ["-filter_complex_script", script, "-map", "[outv]", "-an",
           "-c:v", "libx264", "-preset", "medium", "-crf", str(crf), "-profile:v", "high", "-level", "4.2", "-pix_fmt", "yuv420p",
           "-r", str(FPS), "-g", "60", "-threads", THREADS, "-movflags", "+faststart", out_path]
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
    f = font(18)
    for i, (cid, t) in enumerate(pts):
        p = subprocess.run(["nice", "-n", "10", "ffmpeg", "-nostdin", "-v", "error", "-ss", f"{t:.3f}", "-i", video, "-frames:v", "1",
                            "-f", "image2pipe", "-vcodec", "png", "-"], capture_output=True)
        im = Image.open(io.BytesIO(p.stdout)).convert("RGB").resize((tw, th), Image.LANCZOS)
        x, y = (i % cols) * tw, (i // cols) * th
        sheet.paste(im, (x, y))
        d.rectangle([x, y, x + 120, y + 24], fill=(0, 0, 0))
        d.text((x + 4, y + 2), f"{cid} {t:5.1f}s", font=f, fill=(255, 255, 0))
    sheet.save(out_png)


