#!/usr/bin/env python3
"""The three edytlab launch cuts as code: scene scripts -> Piece (video) + audio events + caption cues.

  python3 launch.py <li|rd|ig> still T1 T2 ...      contact-sheet stills (QC)
  python3 launch.py <li|rd|ig> render [--from a --to b] out.mp4
  python3 launch.py <li|rd|ig> cues out.json         timing of every scene, sfx and caption (for audio + narration)

li = LinkedIn 1:1 1080x1080, rd = Reddit 16:9 1920x1080, ig = Instagram Reels 9:16 1080x1920.
Every shot is a stretch of one of the three real demo recordings in website/public/demos/. Times are demo-video
seconds (the same clock the reel tools measured). Nothing on screen is invented UI; the one diagram is labelled.
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from engine import *  # noqa

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BT, EI, MM = "dj-beatmatched-transition", "dj-extended-club-intro", "dj-mini-mix"
MINUS = "−"

# Regions of the recorded app window, in source pixels (1280x776). Measured on native frames with a 50 px grid.
REG = {
    "WIDE": (0, 0, 1280, 700),
    "TYPE": (905, 440, 375, 275),        # chat composer + the last bubble
    "CHAT_TOP": (920, 62, 360, 330),
    "CHAT_MID": (920, 200, 360, 320),
    "CHAT_LOW": (920, 330, 360, 330),
    "CHAT_TALL": (920, 62, 360, 440),
    "WAVES": (0, 70, 920, 540),
    "WAVES3": (0, 70, 920, 630),
    "PLAYHEAD": (300, 92, 440, 340),
}
LAYOUT_OF = {"li": "sq", "rd": "wide", "ig": "tall"}


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

    def step(self, markup, t0, t1, cue=None, **kw):
        """Step titles share one size on every cut (consistent brand motion)."""
        self.title(markup, t0, t1, cue=cue, size=self.L.step_size, fixed=True, max_lines=2, **kw)

    def title(self, markup, t0, t1, cue=None, **kw):
        self.add(Title(self.ctx, markup, t0, t1, **kw))
        plain = markup.replace("*", "").replace("|", " ").replace("  ", " ")
        if cue != "":
            self.cues.append((t0 + 0.2, t1 - 0.1, cue or plain))

    def clip(self, demo, src0, dur, t0, shots, **kw):
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
def demo_blend(P, t, style):
    """The beatmatch-and-blend demo, step by step. Returns (t_end, last_clip, nxt). Cuts overlap by 0.3 s (crossfade)."""
    ig = style == "ig"
    calm = style == "rd"
    chat_cap = "CHAT_TOP"
    prev = None

    def nxt(c, tt, dur):
        nonlocal prev
        if prev is not None and prev is not c:
            prev.fade_out = 0.3
        prev = c
        return tt + dur - 0.3

    # 1. describe it (typed request: real typing, real send)
    d = P.clip(BT, 38.2 if ig else 37.9, 4.4 if ig else 5.0, t, [dict(t=0, crop="WIDE"), dict(t=0.9 if ig else 1.0, crop="TYPE", move=0.8)])
    P.step("*1* · Describe it in plain English" if not ig else "*1* · Say it in plain English", t + 0.15, t + d.dur - 0.12)
    P.call(d, (930, 620, 340, 82), "You type the edit", 1.1 if ig else 1.4, d.dur - 0.9, sides=("t",))
    t = nxt(d, t, d.dur)
    # 2. the agent calls a tool
    d = P.clip(BT, 45.6, 3.7 if ig else 4.4, t, [dict(t=0, crop="TYPE"), dict(t=0.6, crop=chat_cap, move=0.8)], fade_in=0.3, rise=False)
    P.step("*2* · The agent calls a tool", t + 0.15, t + d.dur - 0.12)
    P.call(d, [(0.9, (934, 94, 104, 28)), (2.9, (934, 103, 104, 28)), (3.4, (934, 110, 104, 28))], "A real tool call", 1.3, 3.2, sides=("r", "b"))
    if not ig:
        P.call(d, [(0.9, (948, 168, 252, 54)), (2.9, (948, 177, 252, 54)), (3.4, (948, 184, 252, 54))], "128 to 120 BPM, pitch kept", 2.0, 4.2 if not calm else 4.0, sides=("t", "b"))
    t = nxt(d, t, d.dur)
    if not ig:
        # 3. it flags what it noticed
        d = P.clip(BT, 62.0, 4.6, t, [dict(t=0, crop="CHAT_TOP"), dict(t=0.6, crop="CHAT_MID", move=0.8)], fade_in=0.3, rise=False)
        P.step("*3* · It flags what it noticed", t + 0.15, t + d.dur - 0.12)
        P.call(d, (948, 246, 250, 70), "It flags its own edge case", 1.5, 4.3, sides=("b", "t"))
        t = nxt(d, t, d.dur)
    # 4. describe the blend -> chips -> 2 kHz low-pass -> waveforms
    d4dur = 5.9 if ig else 7.0
    k_top, k_waves = (1.9, 3.6) if ig else (2.1, 4.2)
    shots = [dict(t=0, crop="CHAT_MID"), dict(t=k_top, crop="CHAT_TOP", move=0.8), dict(t=k_waves, crop="WAVES", move=1.0)]
    d = P.clip(BT, 82.4, d4dur, t, shots, fade_in=0.3, rise=False)
    P.step("*4* · Describe the blend" if not ig else "*3* · Describe the blend", t + 0.15, t + d.dur - 0.12)
    P.call(d, (932, 404, 128, 94), "Three tool calls", 0.3, k_top - 0.7, sides=("t", "b"))
    P.call(d, (965, 100, 240, 46), "A 2 kHz low-pass", k_top + 0.2, k_waves - 0.8, sides=("b", "t"))
    P.call(d, (384, 100, 252, 420), "Crossfade, 16 to 32 s", k_waves + 0.5, d.dur - 0.3, sides=("t", "b", "l"))
    t = nxt(d, t, d.dur)
    return t, prev, nxt


def build(key):
    ig, rd, li = key == "ig", key == "rd", key == "li"
    T = {"li": 69.0, "rd": 63.4, "ig": 31.4}[key]
    P = Plan(key, T)
    L = P.L
    P.add(Progress(P.ctx, T, 0.0, T))
    t = 0.0
    note = "Real screen recording  |  waits for the agent are sped up"
    if li:
        P.add(BarsBG(0.0, 3.7, (0, 640, L.W, 960), alpha=0.28, fade=0.5, seed=0.0))
        P.title("I built an *audio editor* | you talk to.", 0.15, 3.5, box=(70, 200, 1010, 600), size=104, max_lines=2, min_size=64,
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
        P.title("*edytlab:* an open-source audio editor | you drive in plain English", 0.2, 4.4, box=(160, 250, 1760, 640), size=96, max_lines=2,
                min_size=60, cue="edytlab: an open-source audio editor you drive in plain English")
        P.sfx.append((0.15, "whoosh", -14))
        P.scenes += [("intro", 0.0, 4.2)]
        t = 4.0
    else:
        P.add(BarsBG(0.0, 2.4, (0, 760, L.W, 1060), alpha=0.30, fade=0.4, seed=0.0))
        P.title("Describe the blend. | *Hear the mix.*", 0.1, 2.5, box=(65, 380, 1015, 740), size=92, max_lines=2, min_size=60,
                cue="Describe the blend. Hear the mix.", enter=0.45)
        P.sfx.append((0.08, "whoosh", -12))
        P.scenes += [("hook", 0.0, 2.3)]
        t = 2.2

    first_clip_t = t
    t, last, nxt = demo_blend(P, t, key)
    P.scenes += [("demo", first_clip_t, t)]
    # 5. hear it
    ldur = 9.2 if not ig else 7.0
    s0 = 90.3 if not ig else 91.0
    d = P.clip(BT, s0, ldur, t, [dict(t=0, crop="WAVES"), dict(t=2.0 if not ig else 1.6, crop="PLAYHEAD", move=1.1)], fade_in=0.3, rise=False)
    P.step("*5* · Hear it" if not ig else "*4* · Hear it", t + 0.15, t + d.dur - 0.12)
    P.call(d, (422, 62, 34, 32), "Press play: real audio", 0.3, 1.9 if not ig else 1.5, sides=("b", "r"))
    # the playhead moves 16 source px per second (measured: x=339 at 93.0 s, x=387 at 96.0 s)
    ph = lambda src: 339 + (src - 93.0) * 16.0
    P.call(d, [(2.6 - (0 if not ig else 0.0), (ph(s0 + 2.6) - 7, 100, 14, 400)), (d.dur - 0.6, (ph(s0 + d.dur - 0.6) - 7, 100, 14, 400))],
           "Heard in real time", 3.0 if not ig else 2.4, d.dur - 0.4, sides=("t", "b"))
    t_listen = t
    t = nxt(d, t, d.dur)
    # 6. master + export
    d = P.clip(BT, 163.2, 6.0 if not ig else 4.2, t, [dict(t=0, crop="CHAT_LOW")], fade_in=0.3, rise=False)
    P.step("*6* · Master and export" if not ig else "*5* · Master and export", t + 0.15, t + d.dur - 0.12)
    P.call(d, (948, 440, 250, 48), f"{MINUS}14 LUFS, peak {MINUS}1 dBFS", 0.8, d.dur - 0.4, sides=("b", "t"))
    if not ig:
        P.call(d, (937, 358, 72, 22), "Exports a 50 s WAV", 2.6, d.dur - 0.4, sides=("t", "r"))
    t_exp = t
    t = nxt(d, t, d.dur)
    P.scenes += [("listen", t_listen, t_exp + 0.3), ("export", t_exp, t)]
    if not ig:
        # two more jobs, same recordings
        d = P.clip(EI, 62.4, 4.0, t, [dict(t=0, crop="WIDE"), dict(t=1.1, crop="CHAT_TALL", move=0.9)], fade_in=0.3, rise=False)
        P.step("Also: *hold the low end until the drop*", t + 0.15, t + d.dur - 0.12)
        P.call(d, [(1.6, (948, 236, 250, 62)), (3.9, (948, 236, 250, 62))], "A 150 Hz high-pass, as asked", 2.0, d.dur - 0.3, sides=("b", "t"))
        t_a = t
        t = nxt(d, t, d.dur)
        d = P.clip(MM, 55.4, 4.0, t, [dict(t=0, crop="WAVES3")], fade_in=0.3, rise=False)
        P.step("And *a three-track mini-mix*", t + 0.15, t + d.dur - 0.12)
        P.call(d, (420, 100, 260, 620), "4-bar overlaps", 1.0, d.dur - 0.3, sides=("t", "b", "r"))
        d.fade_out = 0.4
        t = t + d.dur - 0.1
        P.scenes += [("also", t_a, t)]
        P.add(Note(P.ctx, note, first_clip_t + 0.2, t - 0.1))
    else:
        d.fade_out = 0.4
        P.add(Note(P.ctx, note, first_clip_t + 0.2, t + 0.3))
    t_flow = t
    if not ig:
        t_flow_end = t_flow + 6.6
        common_tail(P, t_flow, t_flow_end, t_flow_end + 5.3, T, rd)
    else:
        end_card(P, t_flow, T, ig=True)
    return P


def end_card(P, t_end, total, ig=False, rd=False):
    L = P.L
    if ig:
        P.add(BarsBG(t_end, total, (0, 1020, L.W, 1300), alpha=0.22, fade=0.5, seed=2.0))
        P.add(EndCard(P.ctx, t_end + 0.1, total,
                      [("Free and open source", True), ("Unsigned installers  ·  synthetic demo tracks", False)],
                      url_pill="edytlab.com", small=["macOS  ·  Windows  ·  Linux  ·  v0.4.0"]))
        P.cues.append((t_end + 0.8, total - 0.2, "edytlab. Free and open source. Unsigned installers, synthetic demo tracks. edytlab.com"))
    elif rd:
        P.add(BarsBG(t_end, total, (0, int(L.H * 0.66), L.W, int(L.H * 0.96)), alpha=0.18, fade=0.6, seed=2.0))
        P.add(EndCard(P.ctx, t_end + 0.1, total,
                      [("Free and open source (MIT)", True), ("macOS  ·  Windows  ·  Linux  ·  v0.4.0  ·  installers are unsigned", False)],
                      url_pill="github.com/laadtushar/edytlab", small=["edytlab.com"]))
        P.cues.append((t_end + 0.8, total - 0.2, "edytlab. Free and open source (MIT). github.com/laadtushar/edytlab. v0.4.0, installers unsigned."))
    else:
        P.add(BarsBG(t_end, total, (0, int(L.H * 0.70), L.W, int(L.H * 0.97)), alpha=0.18, fade=0.6, seed=2.0))
        P.add(EndCard(P.ctx, t_end + 0.1, total,
                      [("Free and open source (MIT)", True), ("macOS  ·  Windows  ·  Linux  ·  v0.4.0", False)],
                      url_pill="github.com/laadtushar/edytlab", small=["edytlab.com", "Which prep job would you hand off first?"]))
        P.cues.append((t_end + 0.8, total - 0.2, "edytlab. Free and open source (MIT). github.com/laadtushar/edytlab. Which prep job would you hand off first?"))
    P.sfx.append((t_end + 0.2, "chime", -11))
    P.scenes += [("end", t_end, total)]


def common_tail(P, t_flow, t_limits, t_end, total, rd):
    L = P.L
    P.step("*93 tools.* Run on *your machine.*", t_flow, t_limits + 0.1, cue="93 deterministic tools run on your machine. Only the chat goes to the provider you choose.")
    P.add(Flow(P.ctx, t_flow + 0.3, t_limits + 0.1,
               ("Your computer", ["Audio files stay here", "93 deterministic tools", "Rust DSP engine"]),
               ("The provider you choose", ["Only the chat is sent"]),
               "chat text only",
               ["Anthropic", "OpenAI", "OpenRouter", "Groq", "Gemini", "Ollama (local)"],
               vertical=(not rd), box=(None if not rd else (140, 230, 1780, 1000))))
    P.sfx.append((t_flow, "whoosh", -13))
    P.step("*Honest* limits", t_limits, t_end + 0.15, cue="Honest limits: synthetic demo tracks; it preps files, it is not a live DJ tool; no stem separation or transcription yet; unsigned installers.")
    box = (L.safe[0] + 28, L.title_box[3] + 20, L.safe[2] - 28, L.safe[3] - 60) if not rd else (260, 240, 1660, 1000)
    P.add(Stack(P.ctx, ["Demo tracks are short synthetic loops",
                        "It preps files. It is not a live DJ tool",
                        "No stem separation or transcription yet",
                        "Installers are unsigned: expect a warning"],
                t_limits + 0.2, t_end + 0.1, box=box, stagger=0.32, marker="dash", size=(48 if not rd else 66), row_gap=1.75, center=True))
    P.sfx.append((t_limits, "whoosh", -13))
    P.scenes += [("how", t_flow, t_limits), ("limits", t_limits, t_end)]
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
