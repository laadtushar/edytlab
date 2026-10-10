"""Single source of truth for the edytlab social reels.

Everything the renderer, the narration generator and the cue/transcript
writers need lives here: where each caption of each demo video starts on the
FINAL mp4 timeline, where the app's audio is audible (the "Listen" moments),
the narration lines, and the edit lists (EDLs) of every reel.

Times are seconds on the final (sped-up) demo mp4 timeline unless noted.
"""
import math
import re

ROOT = "/tmp/claude-0/-home-user-edytlab/f8840b24-655e-5052-a297-05770f1c9451/scratchpad/social/reel"
DEMO_DIR = "/home/user/edytlab/website/public/demos"
SITE_URL = "edytlab.com"
WORDS_PER_SECOND = 2.4          # the brief's conservative narration rate
BREATHING_ROOM = 0.3            # seconds kept free at the end of a cue

# ---------------------------------------------------------------------------
# Demo caption timing.
#
# make-demo-videos.mjs burns each caption in on the RAW recording timeline,
# then plays stretches at 1x/1.25x/1.75x/3.5x and concatenates the pieces.
# Mapping raw -> final analytically (re-implementing its segments()) lands
# within 0.1 s at the start but drifts to +3.4 s by the end: every concat
# segment boundary adds about 0.1 s of padding. So the starts below are
# MEASURED on the final mp4 (the frame where the lower-third text changes,
# found by differencing the caption strip, see work/detect.py) and spot-checked
# on extracted frames. A caption stays up until the next one starts.
# ---------------------------------------------------------------------------
DEMOS = {
    "BT": dict(
        slug="dj-beatmatched-transition", short="beatmatched-transition",
        title="Beatmatch and blend two tracks", duration=187.733,
        captions=[
            ("BT01", 0.00, "A DJ blends Midnight Drive (120 BPM) into Neon Rush (128 BPM)"),
            ("BT02", 2.40, "Open the tracks"),
            ("BT03", 10.20, "Before: both tracks from the top, 120 against 128 BPM"),
            ("BT04", 22.33, "1 · What am I working with?"),
            ("BT05", 35.73, "2 · Beatmatch"),
            ("BT06", 48.60, "3 · Line up the overlap"),
            ("BT07", 63.87, "4 · Blend: crossfade, low-pass on the outgoing track"),
            ("BT08", 87.73, "Listen: the transition, from 0:12"),
            ("BT09", 110.67, "5 · Master for streaming"),
            ("BT10", 155.93, "6 · Export"),
            ("BT11", 167.00, "The mastered mix, from 0:20"),
            ("BT12", 183.60, "Done: midnight-into-neon.wav, 0:50, -14 LUFS"),
        ],
        # app audio audible (silencedetect -55 dB), per caption id
        audio={"BT03": (15.06, 20.68), "BT08": (92.30, 108.90), "BT11": (171.31, 181.95)},
    ),
    "EI": dict(
        slug="dj-extended-club-intro", short="extended-club-intro",
        title="Extend an intro for mixing", duration=115.867,
        captions=[
            ("EI01", 0.00, "A DJ makes a mixable intro for Solar Flare (124 BPM)"),
            ("EI02", 2.40, "Open the tracks"),
            ("EI03", 9.40, "Before: the original intro"),
            ("EI04", 22.40, "1 · Tempo and bar length"),
            ("EI05", 34.73, "2 · Extend the intro"),
            ("EI06", 53.60, "3 · A filter that opens into the drop"),
            ("EI07", 67.20, "4 · Fade in and export"),
            ("EI08", 81.53, "Listen: the new intro fades in"),
            ("EI09", 95.27, "Listen: the low end arrives at the drop (0:31)"),
            ("EI10", 111.67, "Done: solar-flare-extended-intro.wav, a 16-bar intro"),
        ],
        audio={"EI03": (14.15, 20.75), "EI08": (86.03, 93.65), "EI09": (99.51, 110.10)},
    ),
    "MM": dict(
        slug="dj-mini-mix", short="mini-mix",
        title="A three-track mini-mix", duration=137.800,
        captions=[
            ("MM01", 0.00, "A DJ builds a three-track mini-mix at 124 BPM"),
            ("MM02", 2.47, "Open the tracks"),
            ("MM03", 10.33, "1 · Match every track to 124 BPM"),
            ("MM04", 23.40, "2 · Sequence with 4-bar overlaps"),
            ("MM05", 60.00, "3 · Match loudness, limit the master"),
            ("MM06", 86.80, "Listen: Midnight Drive into Solar Flare (0:23)"),
            ("MM07", 104.93, "Listen: Solar Flare into Neon Rush (0:47)"),
            ("MM08", 122.67, "4 · Export the mix"),
            ("MM09", 133.73, "Done: mini-mix-124bpm.wav, 1:21"),
        ],
        audio={"MM06": (91.75, 103.39), "MM07": (109.29, 120.88)},
    ),
}


def caption_table(code):
    """[(id, start, end, text, audio_or_None)] for one demo, ends from the next start."""
    d = DEMOS[code]
    caps = d["captions"]
    out = []
    for i, (cid, start, text) in enumerate(caps):
        end = caps[i + 1][1] if i + 1 < len(caps) else d["duration"]
        out.append((cid, start, end, text, d["audio"].get(cid)))
    return out


def caption_by_id(cid):
    for code in DEMOS:
        for row in caption_table(code):
            if row[0] == cid:
                return code, row
    raise KeyError(cid)


# ---------------------------------------------------------------------------
# Narration markup:  "{120|one twenty}" shows 120 and speaks "one twenty";
# plain tokens are shown and spoken as written; "[excited]" style tags are
# spoken-only (sent to the model, never shown). Punctuation attached to a
# token stays attached on both sides.
# ---------------------------------------------------------------------------
TOKEN_RE = re.compile(r"^(?P<pre>[^{]*)\{(?P<d>[^|]*)\|(?P<s>[^}]*)\}(?P<post>.*)$")
TAG_RE = re.compile(r"^\[[^\]]+\]$")


TOKEN_SPLIT = re.compile(r"\[[^\]]+\]|\S*\{[^}]*\}\S*|\S+")


def parse_markup(text):
    """-> list of dict(display, spoken, tag) per token (a {display|spoken} group may hold spaces)."""
    units = []
    for tok in TOKEN_SPLIT.findall(text):
        if TAG_RE.match(tok):
            units.append(dict(display="", spoken=tok, tag=True))
            continue
        m = TOKEN_RE.match(tok)
        if m:
            units.append(dict(display=m["pre"] + m["d"] + m["post"], spoken=m["pre"] + m["s"] + m["post"], tag=False))
        else:
            units.append(dict(display=tok, spoken=tok, tag=False))
    return units


def spoken_text(text):
    """The exact string sent to the TTS model (tags included)."""
    return " ".join(u["spoken"] for u in parse_markup(text))


def display_text(text):
    return " ".join(u["display"] for u in parse_markup(text) if not u["tag"])


def spoken_word_count(text):
    return sum(len(u["spoken"].split()) for u in parse_markup(text) if not u["tag"])


def max_words(max_seconds):
    return max(0, math.floor(max_seconds * WORDS_PER_SECOND))


# ---------------------------------------------------------------------------
# Narration lines for the three full demo videos: one per caption moment.
# Listen moments carry only a short lead-in line, spoken before the music
# starts; "" means the cue is silent. Numbers are written the way they are
# spoken, BPM as letters, LUFS as "loofs", the app's file names never read out.
# ---------------------------------------------------------------------------
LINES = {
    "BT01": "Blending two tracks.",
    "BT02": "First, open both tracks.",
    "BT03": "From the top: {120|one twenty} against {128|one twenty-eight}.",
    "BT04": "{1|One}: ask what you're working with: tempo and key.",
    "BT05": "{2|Two}: beatmatch. Neon Rush stretches down to {120|one twenty}.",
    "BT06": "{3|Three}: line up the overlap, under the last {8|eight} bars.",
    "BT07": "{4|Four}: crossfade, with a low-pass on the outgoing track.",
    "BT08": "Have a listen.",
    "BT09": "{5|Five}: master it for streaming.",
    "BT10": "{6|Six}: export the file.",
    "BT11": "Now, the mastered mix.",
    "BT12": "Done: {50|fifty} seconds, {-14|minus fourteen} {LUFS|loofs}.",
    "EI01": "A mixable intro.",
    "EI02": "First, open the track.",
    "EI03": "First, the original intro.",
    "EI04": "{1|One}: check the tempo and bar length.",
    "EI05": "{2|Two}: extend the intro. Copy the first {8|eight} bars.",
    "EI06": "{3|Three}: a filter that opens into the drop.",
    "EI07": "{4|Four}: fade it in, and export.",
    "EI08": "Listen to the intro fade in.",
    "EI09": "Wait for the drop.",
    "EI10": "Done: a {16|sixteen}-bar intro, exported.",
    "MM01": "A three-track mini-mix.",
    "MM02": "First, open the three tracks.",
    "MM03": "First, match every track to {124|one twenty-four} {BPM|B P M}.",
    "MM04": "Second, sequence them with {4|four}-bar overlaps.",
    "MM05": "Third, match the loudness, and limit the master.",
    "MM06": "Here's the first blend.",
    "MM07": "And the second one.",
    "MM08": "Fourth, export the mix.",
    "MM09": "Done: {1:21|one minute twenty-one}, exported.",
}


# ---------------------------------------------------------------------------
# Reel edit lists.
#
# A reel is a list of items:
#   hook  : title card            (id, line, title lines)
#   step  : cut from a demo       (caption id, src start, optional min length)
#   listen: cut with real audio   (caption id, src start = onset - lead, music seconds)
#   end   : end card
# Cut lengths come from the narration (words / 2.4 + 0.7, never under the
# floor), so every line fits its cue with the brief's 0.3 s of room.
# ---------------------------------------------------------------------------
STEP_FLOOR = 4.2
LISTEN_LEAD = 2.0
HOOK_SECONDS = 3.0
END_SECONDS = 3.0

END_LINE = "{edytlab|edit lab}. Free and open source."

REELS = {
    "highlights": dict(
        file="reel-highlights", label="Highlights", prefix="R", pills=True,
        items=[
            dict(kind="hook", line="[excited] Describe the blend. Hear the mix.",
                 title=["Describe the blend.", "Hear the mix."], sub="A real screen recording of edytlab"),
            dict(kind="step", cap="BT05", src=37.9), dict(kind="step", cap="BT07", src=82.6),
            dict(kind="listen", cap="BT08", music=6.7),
            dict(kind="step", cap="EI05", src=40.0), dict(kind="step", cap="EI06", src=61.5),
            dict(kind="listen", cap="EI09", music=8.5),
            dict(kind="step", cap="MM03", src=13.6), dict(kind="step", cap="MM04", src=54.0),
            dict(kind="listen", cap="MM06", music=6.5),
            dict(kind="end"),
        ]),
    "beatmatched-transition": dict(
        file="reel-beatmatched-transition", label="Beatmatch", prefix="B", pills=False, demo="BT",
        items=[
            dict(kind="hook", line="Beatmatch two tracks. Just ask.",
                 title=["Beatmatch", "two tracks.", "Just ask."], sub="Midnight Drive 120 BPM into Neon Rush 128 BPM"),
            dict(kind="step", cap="BT04", src=25.0), dict(kind="step", cap="BT05", src=37.9),
            dict(kind="step", cap="BT06", src=57.0), dict(kind="step", cap="BT07", src=82.6),
            dict(kind="listen", cap="BT08", music=10.0),
            dict(kind="step", cap="BT09", src=141.0),
            dict(kind="listen", cap="BT11", music=6.0),
            dict(kind="step", cap="BT12", src=183.6, floor=4.1),
            dict(kind="end"),
        ]),
    "extended-club-intro": dict(
        file="reel-extended-club-intro", label="Extended intro", prefix="E", pills=False, demo="EI",
        items=[
            dict(kind="hook", line="Extend an intro. Just ask.",
                 title=["Extend an intro.", "Just ask."], sub="Solar Flare, 124 BPM"),
            dict(kind="step", cap="EI04", src=25.0), dict(kind="step", cap="EI05", src=40.0),
            dict(kind="step", cap="EI06", src=59.5), dict(kind="step", cap="EI07", src=69.5),
            dict(kind="listen", cap="EI09", music=10.5),
            dict(kind="step", cap="EI10", src=111.67, floor=4.2),
            dict(kind="end"),
        ]),
    "mini-mix": dict(
        file="reel-mini-mix", label="Mini-mix", prefix="M", pills=False, demo="MM",
        items=[
            dict(kind="hook", line="Build a mini-mix. Just ask.",
                 title=["Build a mini-mix.", "Just ask."], sub="Three tracks, matched to 124 BPM"),
            dict(kind="step", cap="MM03", src=13.6), dict(kind="step", cap="MM04", src=54.0),
            dict(kind="step", cap="MM05", src=66.5),
            dict(kind="listen", cap="MM06", music=6.5), dict(kind="listen", cap="MM07", music=6.5),
            dict(kind="step", cap="MM08", src=125.0),
            dict(kind="step", cap="MM09", src=133.73, floor=4.0),
            dict(kind="end"),
        ]),
    "teaser": dict(
        file="reel-teaser", label="Teaser", prefix="T", pills=False,
        items=[
            dict(kind="hook", line="Ask for the blend.", title=["Ask for", "the blend."], sub="", seconds=2.5),
            dict(kind="step", cap="BT05", src=38.6, line="Beatmatch the two tracks.", seconds=2.7),
            dict(kind="step", cap="BT07", src=83.4, line="Blend with a crossfade.", seconds=2.7),
            dict(kind="listen", cap="BT08", src=94.8, seconds=4.5, silent=True),
            dict(kind="end", line="{edytlab|edit lab}. Free, open source.", seconds=2.5),
        ]),
}
REEL_ORDER = ["highlights", "beatmatched-transition", "extended-club-intro", "mini-mix", "teaser"]

# Spanish narration for the highlights reel (one line per cue id). Keeps the
# on-screen English caption of the app untouched; the big reel caption shows
# the Spanish words.
ES_LINES = {
    "R01": "[excited] Describe la mezcla. Escúchala.",
    "R02": "{2|Dos}: beatmatch. Neon Rush baja a {120|ciento veinte}.",
    "R03": "{4|Cuatro}: crossfade, con un pasa-bajos en la pista que sale.",
    "R04": "Escucha.",
    "R05": "{2|Dos}: alarga la intro. Copia los primeros {8|ocho} compases.",
    "R06": "{3|Tres}: un filtro que se abre en el drop.",
    "R07": "Espera el drop.",
    "R08": "{1|Uno}: iguala cada pista a {124|ciento veinticuatro} {BPM|B P M}.",
    "R09": "{2|Dos}: secuencia con solapes de {4|cuatro} compases.",
    "R10": "Así suena.",
    "R11": "Gratis y de código abierto.",
}


# On-screen text of the Spanish reel (cards, pills, the Listen captions, the note under the frame).
ES_TEXT = dict(
    hook_title=["Describe la mezcla.", "Escúchala."],
    hook_sub="Una grabación real de pantalla de edytlab",
    listen={"BT08": "Escucha: la transición, desde el 0:12",
            "EI09": "Escucha: el grave llega con el drop (0:31)",
            "MM06": "Escucha: Midnight Drive entra en Solar Flare (0:23)"},
    pill={"BT": "Beatmatch y mezcla de dos pistas", "EI": "Alargar una intro para mezclar", "MM": "Un mini-mix de tres pistas"},
    note="Grabación real de pantalla · esperas aceleradas",
    free="Gratis y de código abierto",
)
