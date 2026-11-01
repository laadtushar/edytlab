#!/usr/bin/env python3
"""The three edytlab launch cuts as code: scene scripts -> Piece (video) + audio events + caption cues.

  python3 launch.py <li|rd|ig> still T1 T2 ...      contact-sheet stills (QC)
  python3 launch.py <li|rd|ig> render [--from a --to b] out.mp4
  python3 launch.py <li|rd|ig> cues out.json         timing of every scene, sfx and caption (for audio + narration)

li = LinkedIn 1:1 1080x1080, rd = Reddit 16:9 1920x1080, ig = Instagram Reels 9:16 1080x1920.
Every shot is a stretch of one of the three real demo recordings in website/public/demos/ (re-filmed in the app's violet look,
#581). Times are demo-video seconds, the clock the reels measured: where each burned-in step caption starts (`detect_captions.py`)
and where the app's sound is audible are in marketing/reels/tools/spec.py, which this file reads, and `Plan.clip` asserts that
every shot lies inside the caption it is cut from, that a shot with no sound never overlaps the app's sound, and that a Listen
shot starts at most 0.3 s before the sound and ends inside it. Nothing on screen is invented UI; the one diagram is labelled.
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from engine import *  # noqa

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
# The reels' spec.py holds the caption windows and the audible windows measured on the re-filmed demos. It wants a scratch
# root for the narration work it does not do here; nothing of it is used.
os.environ.setdefault("EDYTLAB_REEL_ROOT", os.path.join(HERE, "work"))
import spec  # noqa: E402

BT, EI, MM = "dj-beatmatched-transition", "dj-extended-club-intro", "dj-mini-mix"
CODE = {BT: "BT", EI: "EI", MM: "MM"}
MINUS = "\u2212"
NOTE = "Real screen recording  \u00b7  waits sped up"

# Regions of the recorded app window, in source pixels (1280x776). Measured on native frames with a 50 px grid. The recording
# burns its own step caption into a card across the bottom (y 684 to 754), which this cut replaces with its own titles, so no
# crop reaches below y 684, except the composer's (TYPE), which a short caption's card never touches.
REG = {
    "WIDE": (0, 0, 1280, 684),
    "TYPE": (905, 440, 375, 266),        # chat composer + the last bubble
    "CHAT_TOP": (920, 60, 360, 330),
    "CHAT_MID": (920, 160, 360, 330),
    "CHAT_LOW": (920, 330, 360, 330),
    "CHAT_TALL": (920, 180, 360, 440),
    "WAVES": (0, 70, 920, 540),
    "WAVES3": (0, 70, 920, 610),
    "PLAYHEAD": (300, 92, 440, 340),
}
LAYOUT_OF = {"li": "sq", "rd": "wide", "ig": "tall"}
LISTEN_LEAD_MAX = 0.3     # a Listen shot starts at most this long before the app's sound (as in the reels)


def caption_window(code, ids):
    """(start, end) on the demo video of the captions `ids` (consecutive), from the reels' spec."""
    rows = {r[0]: r for r in spec.caption_table(code)}
    return rows[ids[0]][1], rows[ids[-1]][2]


def check_shot(demo, src0, dur, caps, listen=False):
    """The shot lies inside its caption(s); a shot without sound keeps clear of the app's sound; a Listen shot starts on it."""
    code = CODE[demo]
    lo, hi = caption_window(code, caps)
    end = src0 + dur
    assert lo - 1e-6 <= src0 and end <= hi + 0.05, (demo, caps, "outside its caption", src0, end, lo, hi)
    windows = list(spec.DEMOS[code]["audio"].values())
    if listen:
        w = [w for w in windows if w[0] - LISTEN_LEAD_MAX <= src0 <= w[1]]
        assert w, (demo, caps, "a Listen shot must start at most 0.3 s before the app's sound", src0, windows)
        onset, offset = w[0]
        assert src0 >= onset - LISTEN_LEAD_MAX - 1e-6 and end <= offset + 0.02, (demo, caps, "must end inside the sound", src0, end, onset, offset)
    else:
        for onset, offset in windows:
            assert end <= onset + 1e-6 or src0 >= offset - 1e-6, (demo, caps, "the app's sound would leak into a silent shot", src0, end, onset, offset)


class Plan:
    """Collects the items plus the audio and caption side-cars while a cut is being scripted."""

    def __init__(self, key, total):
        self.key = key
        self.piece = Piece(LAYOUT_OF[key], total)
        self.ctx = self.piece.ctx
        self.L = self.piece.L
        self.total = total
        self.sfx = []          # (t, name, gain_db)
        self.cues = []         # (t0, t1, text) on-screen text cues (become the .srt)
        self.clips = []
        self.scenes = []       # (name, t0, t1) for narration timing

    def add(self, it):
        return self.piece.add(it)

    def set_total(self, total):
        total = round(total * FPS) / FPS        # a whole number of frames
        self.total = total
        self.piece.duration = total

    def step(self, markup, t0, t1, cue=None, **kw):
        """Step titles share one size on every cut (consistent brand motion)."""
        self.title(markup, t0, t1, cue=cue, size=self.L.step_size, fixed=True, max_lines=2, **kw)

    def title(self, markup, t0, t1, cue=None, **kw):
        self.add(Title(self.ctx, markup, t0, t1, **kw))
        plain = markup.replace("*", "").replace("|", " ").replace("  ", " ")
        if cue != "":
            self.cues.append((t0 + 0.2, t1 - 0.1, cue or plain))

    def clip(self, demo, src0, dur, t0, shots, caps=None, listen=False, **kw):
        check_shot(demo, src0, dur, caps, listen)
        c = FootageClip(self.ctx, demo, src0, dur, t0, shots, REG, **kw)
        self.add(c)
        self.clips.append(c)
        return c

    def call(self, clip, region, label, t_in, t_out, **kw):
        c = Callout(self.ctx, clip, region, label, t_in, t_out, **kw)
        self.add(c)
        self.sfx.append((clip.t0 + t_in + 0.5, "tick", -10))
        return c


# ----------------------------------------------------------------- shared scenes
END_SECONDS = {"li": 5.2, "rd": 5.5, "ig": 5.5}    # how long the end card holds


def demo_blend(P, t, style):
    """The beatmatch-and-blend demo, step by step. Returns (t_end, last_clip, nxt). Cuts overlap by 0.3 s (crossfade)."""
    ig = style == "ig"
    chat_low = "CHAT_LOW"
    prev = None

    def nxt(c, tt, dur):
        nonlocal prev
        if prev is not None and prev is not c:
            prev.fade_out = 0.3
        prev = c
        return tt + dur - 0.3

    # 1. describe it: a question typed into the composer (real typing, real send)
    d = P.clip(BT, 23.6 if ig else 23.4, 4.4 if ig else 5.0, t, [dict(t=0, crop="WIDE"), dict(t=0.9, crop="TYPE", move=0.8)], caps=("BT04",))
    P.step("*1* · Describe it in plain English" if not ig else "*1* · Say it in plain English", t + 0.15, t + d.dur - 0.12)
    P.call(d, (930, 578, 345, 124), "You type the edit", 1.2, d.dur - 0.9, sides=("t",))
    t = nxt(d, t, d.dur)
    # 2. the agent calls a tool: "Time stretch" runs, the chip turns green
    d = P.clip(BT, 40.0, 3.5, t, [dict(t=0, crop="TYPE"), dict(t=0.4, crop="CHAT_MID", move=0.6)], caps=("BT05",), fade_in=0.3, rise=False)
    P.step("*2* · The agent calls a tool", t + 0.15, t + d.dur - 0.12)
    P.call(d, (937, 181, 108, 22), "A real tool call", 0.9, 3.0, sides=("r", "b"))
    if not ig:
        P.call(d, (945, 255, 255, 132), "Tempo matched, pitch kept", 1.3, 3.0, sides=("t", "b"))
    t = nxt(d, t, d.dur)
    if not ig:
        # 3. it reports what it did: the reply lands (src 52 to 54.5)
        d = P.clip(BT, 50.6, 3.9, t, [dict(t=0, crop="CHAT_MID")], caps=("BT06",), fade_in=0.3, rise=False)
        P.step("*3* · It reports what changed", t + 0.15, t + d.dur - 0.12)
        P.call(d, (945, 160, 250, 92), "It says what it did", 2.2, 3.7, sides=("b",))
        t = nxt(d, t, d.dur)
    # 4. describe the blend: the request is sent (src 65), the reply lists three edits (from 68), then the fades on the waveforms
    s0, d4, k_w = (64.0, 7.0, 5.2) if ig else (63.0, 8.0, 6.2)
    shots = [dict(t=0, crop="CHAT_TALL"), dict(t=k_w, crop="WAVES", move=0.9)]
    d = P.clip(BT, s0, d4, t, shots, caps=("BT07",), fade_in=0.3, rise=False)
    P.step("*4* · Describe the blend" if not ig else "*3* · Describe the blend", t + 0.15, t + d.dur - 0.12)
    if not ig:
        P.call(d, (995, 395, 255, 150), "Fades + low-pass, in plain English", 1.9, 3.1, sides=("t", "b"))
        P.call(d, (940, 110, 260, 300), "Three edits, one request", 3.7, k_w - 0.1, sides=("r", "l"))
    else:
        P.call(d, (940, 110, 260, 300), "Three edits, one request", 2.7, k_w - 0.1, sides=("r", "l"))
    P.call(d, (495, 110, 385, 400), "The crossfade", k_w + 0.5, d.dur - 0.3, sides=("t", "b", "l"))
    t = nxt(d, t, d.dur)
    return t, prev, nxt


def build(key):
    ig, rd, li = key == "ig", key == "rd", key == "li"
    P = Plan(key, 1000.0)       # the total is known once the last scene is placed: set below
    L = P.L
    t = 0.0
    if li:
        P.add(BarsBG(0.0, 3.7, (0, 640, L.W, 960), alpha=0.28, fade=0.5, seed=0.0))
        P.add(Lockup(P.ctx, 0.15, 3.5, top=150, mark=72))
        P.title("I built an *audio editor* | you talk to.", 0.3, 3.5, box=(70, 290, 1010, 640), size=104, max_lines=2, min_size=64,
                cue="I built an audio editor you talk to.")
        P.sfx.append((0.12, "whoosh", -13))
        P.title("One blend. *Seven chores.*", 3.5, 8.2, cue="One blend. Seven chores.")
        P.add(Stack(P.ctx, ["Find both tempos", "Stretch one to match", "Line up the downbeats", "Draw the fades",
                            "Filter the outgoing track", "Check the loudness", "Export a file"],
                    3.6, 8.3, box=(170, 260, 940, 960), stagger=0.3, heading="Prepping a blend by hand", marker="dot", size=54, center=True))
        P.add(BarsBG(8.3, 10.4, (0, 640, L.W, 960), alpha=0.24, fade=0.4, seed=3.0))
        P.title("Or just | *describe it.*", 8.4, 10.4, box=(70, 220, 1010, 640), size=116, max_lines=2, min_size=70, cue="Or just describe it.")
        P.sfx += [(3.5, "whoosh", -15), (8.35, "whoosh", -13)]
        t = 9.9
        P.scenes += [("hook", 0.0, 3.4), ("problem", 3.4, 9.9)]
    elif rd:
        P.add(BarsBG(0.0, 4.2, (0, 700, L.W, 980), alpha=0.18, fade=0.6, seed=0.0))
        P.add(Lockup(P.ctx, 0.2, 4.4, top=170, mark=80))
        P.title("An open-source audio editor | you drive in plain English", 0.35, 4.4, box=(160, 330, 1760, 700), size=96, max_lines=2,
                min_size=60, cue="edytlab: an open-source audio editor you drive in plain English")
        P.sfx.append((0.15, "whoosh", -14))
        P.scenes += [("intro", 0.0, 4.2)]
        t = 4.0
    else:
        P.add(BarsBG(0.0, 2.4, (0, 760, L.W, 1060), alpha=0.30, fade=0.4, seed=0.0))
        P.add(Lockup(P.ctx, 0.1, 2.5, top=300, mark=72))
        P.title("Describe the blend. | *Hear the mix.*", 0.2, 2.5, box=(120, 420, 960, 780), size=92, max_lines=2, min_size=60,
                cue="Describe the blend. Hear the mix.", enter=0.45)
        P.sfx.append((0.08, "whoosh", -12))
        P.scenes += [("hook", 0.0, 2.3)]
        t = 2.2

    first_clip_t = t
    t, last, nxt = demo_blend(P, t, key)
    P.scenes += [("demo", first_clip_t, t)]
    # 5. hear it: the cut starts as the sound does and ends inside it
    ldur = 9.2 if not ig else 7.0
    s0 = 1120 / 15      # 74.667 s: the demo is 15 fps, and this is the last frame at least 0.2 s before the sound (audible from 74.87 s)
    d = P.clip(BT, s0, ldur, t, [dict(t=0, crop="WAVES"), dict(t=2.0 if not ig else 1.6, crop="PLAYHEAD", move=1.1)], caps=("BT08",), listen=True,
               fade_in=0.3, rise=False)
    P.step("*5* · Hear it" if not ig else "*4* · Hear it", t + 0.15, t + d.dur - 0.12)
    P.call(d, (424, 58, 32, 30), "Press play: real audio", 0.3, 1.9 if not ig else 1.5, sides=("b", "r"))
    # the playhead moves 15.5 source px per second (measured: x=348 at 76.0 s, 410 at 80.0 s, 472 at 84.0 s, 534 at 88.0 s)
    ph = lambda src: 348 + (src - 76.0) * 15.5
    # on the square and tall cuts the ring covers the top track only: a full-height ring leaves its chip no room but the title's
    # or the note's (on 16:9 the chips go in the gutters, so the ring may span both tracks)
    tall_ring = not rd
    rh = 240 if tall_ring else 420
    P.call(d, [(2.6, (ph(s0 + 2.6) - 7, 96, 14, rh)), (d.dur - 0.6, (ph(s0 + d.dur - 0.6) - 7, 96, 14, rh))],
           "Heard in real time", 3.0 if not ig else 2.4, d.dur - 0.4, sides=("b", "t") if tall_ring else ("t", "b"))
    t_listen = t
    t = nxt(d, t, d.dur)
    # 6. master + export: the export reply lands at about 119 to 121 s (the app's sound is silent here)
    d = P.clip(BT, 117.0, 6.0 if not ig else 4.2, t, [dict(t=0, crop="CHAT_LOW")], caps=("BT10",), fade_in=0.3, rise=False)
    P.step("*6* · Master and export" if not ig else "*5* · Master and export", t + 0.15, t + d.dur - 0.12)
    P.call(d, (945, 395, 240, 150), "It reports what it exported", 3.8 if not ig else 2.9, d.dur - 0.4, sides=("b", "t"), pad=4)
    t_exp = t
    t = nxt(d, t, d.dur)
    P.scenes += [("listen", t_listen, t_exp + 0.3), ("export", t_exp, t)]
    if not ig:
        # two more jobs, same recordings
        d = P.clip(EI, 49.0, 4.0, t, [dict(t=0, crop="CHAT_MID")], caps=("EI05",), fade_in=0.3, rise=False)
        P.step("Also: *hold the low end until the drop*", t + 0.15, t + d.dur - 0.12)
        P.call(d, (960, 340, 235, 115), "A high-pass until the drop", 1.8, d.dur - 0.3, sides=("t",))
        t_a = t
        t = nxt(d, t, d.dur)
        d = P.clip(MM, 33.5, 4.0, t, [dict(t=0, crop="WAVES3")], caps=("MM04",), fade_in=0.3, rise=False)
        P.step("And *a three-track mini-mix*", t + 0.15, t + d.dur - 0.12)
        P.call(d, (355, 100, 330, 580), "4-bar overlaps", 1.0, d.dur - 0.3, sides=("t", "b", "r"))
        d.fade_out = 0.4
        t = t + d.dur - 0.1
        P.scenes += [("also", t_a, t)]
        P.add(Note(P.ctx, NOTE, first_clip_t + 0.2, t - 0.1))
    else:
        d.fade_out = 0.4
        P.add(Note(P.ctx, NOTE, first_clip_t + 0.2, t + 0.3))
    t_flow = t
    if not ig:
        t_flow_end = t_flow + 6.6
        T = t_flow_end + END_SECONDS[key]
        P.set_total(T)
        common_tail(P, t_flow, t_flow_end, P.total, rd)
    else:
        T = t_flow + END_SECONDS[key]
        P.set_total(T)
        end_card(P, t_flow, P.total, ig=True)
    P.add(Progress(P.ctx, P.total, 0.0, P.total))
    return P


def end_card(P, t_end, total, ig=False, rd=False):
    """The sign-off: the site's lockup, "Free, open source", the url. It names no version, on purpose (marketing/README.md, "Evergreen media rules")."""
    L = P.L
    if ig:
        ec = EndCard(P.ctx, t_end + 0.1, total, [("Free, open source", True), ("Synthetic demo tracks", False)],
                     url_pill="edytlab.com", small=["macOS  \u00b7  Windows  \u00b7  Linux"])
        cue = "edytlab. Free, open source. Synthetic demo tracks. edytlab.com"
    elif rd:
        ec = EndCard(P.ctx, t_end + 0.1, total, [("Free, open source", True), ("MIT licence  \u00b7  macOS  \u00b7  Windows  \u00b7  Linux", False)],
                     url_pill="github.com/laadtushar/edytlab", small=["edytlab.com"])
        cue = "edytlab. Free, open source (MIT). github.com/laadtushar/edytlab. edytlab.com."
    else:
        ec = EndCard(P.ctx, t_end + 0.1, total, [("Free, open source", True), ("MIT licence  \u00b7  macOS  \u00b7  Windows  \u00b7  Linux", False)],
                     url_pill="edytlab.com", small=["github.com/laadtushar/edytlab", "Which prep job would you hand off first?"])
        cue = "edytlab. Free, open source (MIT). edytlab.com. github.com/laadtushar/edytlab. Which prep job would you hand off first?"
    P.add(ec)
    # the breathing bars under the block, never behind a word
    y0 = min(L.H - 60, ec.bottom + 30)
    P.add(BarsBG(t_end, total, (0, y0, L.W, min(L.H - 20, y0 + 230)), alpha=0.22, fade=0.5, seed=2.0))
    P.cues.append((t_end + 0.8, total - 0.2, cue))
    P.sfx.append((t_end + 0.2, "chime", -11))
    P.scenes += [("end", t_end, total)]


def common_tail(P, t_flow, t_end, total, rd):
    # No tool count, no provider list: both change with a release (see marketing/README.md, "Evergreen media rules").
    P.step("*Deterministic tools.* Run on *your machine.*", t_flow, t_end + 0.1, cue="Deterministic tools run on your machine. Only the chat goes to the provider you choose.")
    P.add(Flow(P.ctx, t_flow + 0.3, t_end + 0.1,
               ("Your computer", ["Audio files stay here", "Deterministic tools", "Rust DSP engine"]),
               ("The provider you choose", ["Only the chat is sent"]),
               "chat text only",
               ["Hosted AI providers", "Local models (Ollama)"],
               vertical=(not rd), box=(None if not rd else (140, 230, 1780, 1000))))
    P.sfx.append((t_flow, "whoosh", -13))
    # No "Honest limits" slide (removed at the owner's request, 2026-10-24): the how-it-works card is followed by the end card.
    # No whoosh into the end card: like the Instagram cut's, it has the chime alone (a whoosh 0.2 s ahead of it would stack on it).
    P.scenes += [("how", t_flow, t_end)]
    end_card(P, t_end, total, rd=rd)


BUILDERS = {k: (lambda k=k: build(k)) for k in LAYOUT_OF}


def contact(P, times, out, cols=4, w=480):
    L = P.L
    h = round(w * L.H / L.W)
    rows = (len(times) + cols - 1) // cols
    sheet = Image.new("RGB", (cols * w, rows * h), (0, 0, 0))
    d = ImageDraw.Draw(sheet)
    f = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", 16)
    for i, t in enumerate(times):
        im = P.piece.still(t).resize((w, h), Image.LANCZOS)
        x, y = (i % cols) * w, (i // cols) * h
        sheet.paste(im, (x, y))
        d.rectangle([x, y, x + 62, y + 20], fill=(0, 0, 0))
        d.text((x + 4, y + 2), f"{t:.1f}", fill=(255, 255, 0), font=f)
    sheet.save(out)


def cues_json(P):
    return dict(key=P.key, total=P.total, scenes=P.scenes, sfx=sorted(P.sfx), captions=sorted(P.cues),
                clips=[dict(demo=c.demo, src0=c.src0, dur=c.dur, t0=c.t0, fade_in=c.fade_in, fade_out=c.fade_out) for c in P.clips])


if __name__ == "__main__":
    plat, cmd = sys.argv[1], sys.argv[2]
    P = BUILDERS[plat]()
    if cmd == "still":
        times = [float(x) for x in sys.argv[3:]]
        os.makedirs(f"{HERE}/work", exist_ok=True)   # work/ is a scratch directory, not committed
        out = f"{HERE}/work/qc_{plat}.png"
        if len(times) == 1:
            P.piece.still(times[0]).save(out)
        else:
            contact(P, times, out, cols=4 if plat != "rd" else 3, w=480 if plat != "rd" else 640)
        print(out)
    elif cmd == "cues":
        json.dump(cues_json(P), open(sys.argv[3], "w"), indent=1)
        print(sys.argv[3], "total", P.total)
    elif cmd == "render":
        a = sys.argv[3:]
        t_from = float(a[a.index("--from") + 1]) if "--from" in a else 0.0
        t_to = float(a[a.index("--to") + 1]) if "--to" in a else None
        P.piece.render(a[-1], t_from, t_to)
        print(a[-1])
