"""ElevenLabs narration for the reels and demos.

The API key is read from the ELEVENLABS_API_KEY environment variable only; it is never
printed, logged or written. Load it per command:   set -a; . <env file>; set +a; python3 narrate.py ...

  narrate.py status               character_count / limit from the subscription endpoint
  narrate.py test                 the two first test calls (default settings, then speed=1.15)
  narrate.py gen <timeline> ...   generate every missing line of the timelines, in order
  narrate.py sfx                  generate the sound effects
  narrate.py prepare <timeline>   build out/audio/<timeline>/<cue-id>.(mp3|wav) + timing json (no API calls)

Lines are cached by (text, model, voice settings, language): a line shared by several reels or
a demo is generated once.
"""
import base64, hashlib, json, os, subprocess, sys, time
import requests
from spec import *
import cues as C

API = "https://api.elevenlabs.io"
VOICE_ID = "CwhRBWXzGAHq8TQ4Fs17"         # Roger - Laid-Back, Casual, Resonant
MODEL_ID = "eleven_v4"
OUTPUT_FORMAT = "mp3_44100_128"           # the highest MP3 the free tier serves
# Eleven v4 exposes Stability and Similarity only (GET /v1/models: can_use_style = false and
# can_use_speaker_boost = false), so those two are not sent.
SETTINGS = dict(stability=0.5, similarity_boost=0.75)
BUDGET = 8000                              # hard cap for everything, retries and sound effects included
TTS = f"{ROOT}/work/tts"
OUT = f"{ROOT}/out"
USAGE_LOG = f"{OUT}/usage.log"
FIT_MARGIN = 0.12                          # speech must end this long before the voice window does
MAX_TEMPO = 1.10


def key():
    k = os.environ.get("ELEVENLABS_API_KEY")
    if not k:
        sys.exit("ELEVENLABS_API_KEY is not set (load it with: set -a; . <env file>; set +a)")
    return k


def hdr(extra=None):
    h = {"xi-api-key": key()}
    h.update(extra or {})
    return h


def log(msg):
    os.makedirs(OUT, exist_ok=True)
    line = f"{time.strftime('%Y-%m-%d %H:%M:%S')}  {msg}"
    print(line, flush=True)
    with open(USAGE_LOG, "a") as f:
        f.write(line + "\n")


def subscription():
    r = requests.get(f"{API}/v1/user/subscription", headers=hdr(), timeout=30)
    r.raise_for_status()
    j = r.json()
    return j["character_count"], j["character_limit"]


LEDGER = f"{ROOT}/work/ledger.json"


def ledger_total():
    return json.load(open(LEDGER))["spent"] if os.path.exists(LEDGER) else 0


def ledger_add(n, what):
    d = json.load(open(LEDGER)) if os.path.exists(LEDGER) else dict(spent=0, entries=[])
    d["spent"] += n
    d["entries"].append([time.strftime("%H:%M:%S"), what, n])
    json.dump(d, open(LEDGER, "w"))


def spent():
    """The subscription counter lags behind the calls, so trust the larger of it and the local ledger."""
    return max(subscription()[0], ledger_total())


def checkpoint(label, before=None):
    used, limit = subscription()
    delta = "" if before is None else f" (+{used - before})"
    log(f"{label}: character_count={used}/{limit}{delta}")
    return used


def ffprobe_duration(path):
    p = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", path], capture_output=True, text=True)
    return float(p.stdout.strip())


# ------------------------------------------------------------------- TTS
def line_hash(text, lang=None, settings=None):
    blob = json.dumps(dict(t=text, m=MODEL_ID, v=VOICE_ID, s=settings or SETTINGS, l=lang, f=OUTPUT_FORMAT), sort_keys=True)
    return hashlib.sha1(blob.encode()).hexdigest()[:16]


def tts_call(text, lang=None, settings=None, previous_ids=None, next_text=None):
    """One with-timestamps call. -> (mp3 bytes, alignment dict, request_id)"""
    body = dict(text=text, model_id=MODEL_ID, voice_settings=settings or SETTINGS)
    if lang:
        body["language_code"] = lang
    if previous_ids:
        body["previous_request_ids"] = previous_ids[-3:]
    if next_text:
        body["next_text"] = next_text
    r = requests.post(f"{API}/v1/text-to-speech/{VOICE_ID}/with-timestamps", params=dict(output_format=OUTPUT_FORMAT),
                      headers=hdr({"Content-Type": "application/json"}), json=body, timeout=180)
    if r.status_code != 200:
        raise RuntimeError(f"TTS {r.status_code}: {r.text[:600]}")
    j = r.json()
    return base64.b64decode(j["audio_base64"]), j.get("alignment"), j.get("normalized_alignment"), r.headers.get("request-id", "")


def cached(text, lang=None):
    h = line_hash(text, lang)
    p = f"{TTS}/{h}.json"
    return json.load(open(p)) if os.path.exists(p) else None


def generate_line(text, lang=None, previous_ids=None, next_text=None, settings=None):
    """Generate (or fetch from cache) one line; returns its meta dict."""
    os.makedirs(TTS, exist_ok=True)
    m = cached(text, lang)
    if m:
        return m
    used = ledger_total()
    if used + len(text) > BUDGET:
        raise RuntimeError(f"budget: {used} used + {len(text)} would pass {BUDGET}")
    audio, al, nal, rid = tts_call(text, lang, settings, previous_ids, next_text)
    ledger_add(len(text), "tts " + text[:30])
    h = line_hash(text, lang, settings)
    mp3 = f"{TTS}/{h}.mp3"
    open(mp3, "wb").write(audio)
    meta = dict(hash=h, text=text, lang=lang, request_id=rid, made=time.time(), mp3=mp3, alignment=al, normalized_alignment=nal,
                duration=ffprobe_duration(mp3), chars=len(text), used_previous_ids=bool(previous_ids), used_next_text=bool(next_text),
                settings=settings or SETTINGS)
    json.dump(meta, open(f"{TTS}/{h}.json", "w"))
    return meta


def word_times(markup, meta):
    """Per displayed unit start/end from the character alignment of the text that was sent."""
    sent = meta["text"]
    al = meta["alignment"]
    chars, st, en = al["characters"], al["character_start_times_seconds"], al["character_end_times_seconds"]
    if "".join(chars) != sent:
        # the service may normalise whitespace; map by skipping characters that differ
        j, idx = 0, []
        for i, ch in enumerate(sent):
            while j < len(chars) and chars[j] != ch:
                j += 1
            idx.append(j if j < len(chars) else len(chars) - 1)
            j += 1
    else:
        idx = list(range(len(sent)))
    units = parse_markup(markup)
    out, pos = [], 0
    for u in units:
        s = u["spoken"]
        a = sent.index(s, pos)
        b = a + len(s) - 1
        pos = b + 1
        if u["tag"]:
            continue
        out.append(dict(text=u["display"], start=round(st[idx[a]], 3), end=round(en[idx[b]], 3)))
    return out


def write_speech():
    """work/speech.json: spoken text -> end of the last word, for every cached English or Spanish line."""
    d = {}
    for f in os.listdir(TTS):
        if f.endswith(".json") and f != "speech.json":
            m = json.load(open(f"{TTS}/{f}"))
            if m.get("alignment"):
                d[m["text"]] = round(m["alignment"]["character_end_times_seconds"][-1], 3)
    json.dump(d, open(f"{ROOT}/work/speech.json", "w"), indent=0)
    return d


def speech_end(words):
    return words[-1]["end"] if words else 0.0


# ----------------------------------------------------- timelines to generate
def timelines():
    t = {}
    for name in REEL_ORDER:
        tl = C.build_reel(name)
        t[tl["file"]] = tl
    for code in DEMOS:
        tl = C.build_demo(code)
        t[tl["name"]] = tl
    es = C.build_reel("highlights")
    es["file"] = "reel-highlights-es"
    es["lang"] = "es"
    for c in es["cues"]:
        if c["id"] in ES_LINES:
            line = ES_LINES[c["id"]]
            c["narration"] = line
            c["narration_spoken"] = spoken_text(line)
            c["narration_display"] = display_text(line)
            c["words"] = spoken_word_count(line)
            c.pop("silent", None)
    t["reel-highlights-es"] = es
    return t


def voiced(tl):
    return [c for c in tl["cues"] if c.get("narration") and not c.get("silent")]


def generate_timeline(tl, previous_ids=None):
    lang = tl.get("lang")
    cs = voiced(tl)
    ids = list(previous_ids or [])
    for i, c in enumerate(cs):
        text = c["narration_spoken"]
        nxt = cs[i + 1]["narration_spoken"] if i + 1 < len(cs) else None
        had = cached(text, lang) is not None
        m = generate_line(text, lang, ids[-3:], nxt)
        if not had:
            log(f"{tl['file']} {c['id']} {text!r} ({len(text)} chars) audio={m['duration']:.2f}s  ledger={ledger_total()}/{BUDGET}")
            time.sleep(0.5)
        if m.get("request_id") and time.time() - m["made"] < 7000:
            ids.append(m["request_id"])
    return ids


# --------------------------------------------------------- prepare per-cue clips
def prepare(tl):
    """Copy/tempo-fit each cue's clip into out/audio/<file>/<cue-id>.(mp3|wav) and write the timing json."""
    lang = tl.get("lang")
    adir = f"{OUT}/audio/{tl['file']}"
    os.makedirs(adir, exist_ok=True)
    timing, report = {}, []
    for c in voiced(tl):
        m = cached(c["narration_spoken"], lang)
        if not m:
            report.append((c["id"], "MISSING"))
            continue
        words = word_times(c["narration"], m)
        end = speech_end(words)
        window = c.get("voice_end", c["end"]) - c["start"]
        limit = window - FIT_MARGIN
        room = window - BREATHING_ROOM
        tempo = 1.0
        if end > room:                      # keep the brief's 0.3 s of room if a gentle speed-up (<= 1.10x) can do it
            tempo = min(MAX_TEMPO, end / room)
            if tempo < 1.02:
                tempo = 1.0
        for pat in ("mp3", "wav"):
            p = f"{adir}/{c['id']}.{pat}"
            if os.path.exists(p):
                os.remove(p)
        if tempo > 1.0001:
            dst = f"{adir}/{c['id']}.wav"
            subprocess.run(["ffmpeg", "-nostdin", "-y", "-v", "error", "-i", m["mp3"], "-af", f"atempo={tempo:.4f}", "-ar", "44100", dst], check=True)
            for w in words:
                w["start"] = round(w["start"] / tempo, 3)
                w["end"] = round(w["end"] / tempo, 3)
            end /= tempo
        else:
            dst = f"{adir}/{c['id']}.mp3"
            subprocess.run(["cp", m["mp3"], dst], check=True)
        status = "ok" if end <= window - BREATHING_ROOM + 1e-6 else ("tight" if end <= limit + 1e-6 else "OVER")
        timing[c["id"]] = dict(words=words, speech_end=round(end, 3), tempo=round(tempo, 4), clip=os.path.basename(dst),
                               window=round(window, 3), status=status, request_id=m.get("request_id", ""))
        report.append((c["id"], status, round(end, 2), round(window, 2), round(tempo, 3)))
    json.dump(timing, open(f"{OUT}/{tl['file']}-narration-timing.json", "w"), indent=1)
    return timing, report


# ------------------------------------------------------------------- SFX
SFX = {
    "whoosh": dict(text="soft airy whoosh transition, short, subtle and clean", duration_seconds=0.6),
    "hit": dict(text="subtle deep low sub hit, short and soft, cinematic", duration_seconds=0.8),
    "ding": dict(text="single soft pleasant chime ding, clean, short", duration_seconds=1.0),
}


def gen_sfx():
    os.makedirs(f"{OUT}/sfx", exist_ok=True)
    for name, spec in SFX.items():
        p = f"{OUT}/sfx/{name}.mp3"
        if os.path.exists(p):
            continue
        est = int(spec["duration_seconds"] * 40) + 5
        used = ledger_total()
        if used + est > BUDGET:
            raise RuntimeError("budget")
        r = requests.post(f"{API}/v1/sound-generation", params=dict(output_format=OUTPUT_FORMAT), headers=hdr({"Content-Type": "application/json"}),
                          json=dict(text=spec["text"], duration_seconds=spec["duration_seconds"], prompt_influence=0.5), timeout=180)
        if r.status_code != 200:
            raise RuntimeError(f"SFX {r.status_code}: {r.text[:400]}")
        open(p, "wb").write(r.content)
        ledger_add(est, "sfx " + name)
        log(f"sfx {name} ({spec['duration_seconds']}s requested) audio={ffprobe_duration(p):.2f}s  ledger={ledger_total()}/{BUDGET} (estimate 40 credits/s)")


# -------------------------------------------------------------------- CLI
def main():
    cmd = sys.argv[1]
    if cmd == "status":
        checkpoint("status")
        log(f"local ledger: {ledger_total()} chars sent / budget {BUDGET}")
    elif cmd == "test":
        text = spoken_text(C.build_reel("highlights")["cues"][0]["narration"])
        nxt = spoken_text(LINES["BT05"])
        before = checkpoint("before test")
        m = generate_line(text, None, None, nxt)
        after = checkpoint(f"test 1: default settings, next_text sent; text={text!r} ({len(text)} chars) audio={m['duration']:.2f}s request-id {'present' if m['request_id'] else 'missing'}", before)
        al = m["alignment"]
        log(f"alignment: {len(al['characters'])} chars for {len(text)} sent; first/last char times {al['character_start_times_seconds'][0]:.2f}/{al['character_end_times_seconds'][-1]:.2f}; normalized_alignment present={m['normalized_alignment'] is not None}")
        log("alignment chars: " + "".join(al["characters"]))
    elif cmd == "test-speed":
        text = spoken_text(C.build_reel("highlights")["cues"][0]["narration"])
        before = subscription()[0]
        s = dict(SETTINGS, speed=1.15)
        audio, al, nal, rid = tts_call(text, None, s)
        ledger_add(len(text), "tts speed test")
        p = f"{TTS}/speedtest.mp3"
        open(p, "wb").write(audio)
        checkpoint(f"test 2: speed=1.15, audio={ffprobe_duration(p):.2f}s", before)
    elif cmd == "gen":
        prev = []
        tls = timelines()
        for name in sys.argv[2:]:
            prev = generate_timeline(tls[name], [])
        checkpoint("batch done")
        write_speech()
    elif cmd == "sfx":
        gen_sfx()
    elif cmd == "prepare":
        tls = timelines()
        for name in sys.argv[2:]:
            timing, rep = prepare(tls[name])
            print(name)
            for r in rep:
                print("  ", r)


if __name__ == "__main__":
    main()
