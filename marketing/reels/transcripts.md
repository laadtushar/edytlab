# edytlab reels: narration transcripts and ElevenLabs settings

Voice **Roger - Laid-Back, Casual, Resonant** (`voice_id CwhRBWXzGAHq8TQ4Fs17`), model **Eleven v4** (`eleven_v4`).
Every line below was generated with exactly these settings, so the narrated videos in this folder and anything you
re-record in the ElevenLabs web app will match.

## Read this before posting: the free-plan licence

The audio in `*-narrated.mp4` and in `audio/` was generated on the account's **free plan** (10,000 credits a month).
ElevenLabs' own terms ("Can I publish the content I generate on the platform?",
https://elevenlabs.io/docs/help-center/legal/can-i-publish-the-content-i-generate-on-the-platform), verbatim:

- "The free plan does not include a commercial license and cannot be used for any commercial purpose."
- "you must attribute it to ElevenLabs by including "elevenlabs.io" or "11.ai" in the title"
- "All paid plans include a commercial license, provided you're not using Beta Services."
- "Content generated during a paid subscription can be used commercially, and indefinitely"
- "Content created outside of a paid subscription (before or after) cannot be used commercially"
- "Attribution requirement is subject to the agreement you have with ElevenLabs."

The pricing page (https://elevenlabs.io/pricing) lists "Commercial License" on the Starter card as an addition to "everything
in free", and does not list it for Free. Whether promoting a free, open-source project counts as a "commercial purpose" is a
call for the owner or ElevenLabs (it is not something this folder can settle). The safe routes are to post only the
un-narrated reels, to put "elevenlabs.io" in the title of any post that uses the free-plan voice, or to regenerate the audio
on a paid plan (`tools/narrate.py gen ...`, roughly 2,000 characters for everything) before posting commercially.

## ElevenLabs settings

Docs read on 2026-10-10; "API test" means I called the live API and looked at what came back.

| Setting | Value used / recommended | Why | Status |
|---|---|---|---|
| Model | **Eleven v4** (`eleven_v4`) | The model you asked for: "Our most emotionally rich, expressive speech synthesis model", 90+ languages, 10,000 characters per request. | Confirmed: https://elevenlabs.io/docs/overview/models and `GET /v1/models` (`can_do_text_to_speech: true`, 1 credit per character; `eleven_v4_turbo` is half price and also takes audio tags) |
| Stability slider (Creative to Robust) | **Middle ("Natural")**; API `stability: 0.5` | One file per cue means separate takes have to sound like one person: Creative drifts between takes, Robust flattens delivery and (per the v3 guide) answers tags less. With almost no tags, the middle is the right trade. | The API accepted 0.5. v4 page: "Lower values allow more expressive, varied delivery; higher values keep the performance closer to a fixed baseline" (https://elevenlabs.io/docs/overview/capabilities/text-to-speech/eleven-v4.md). The labels Creative / Natural / Robust and their numbers are documented for **v3** (https://elevenlabs.io/docs/best-practices/prompting); for v4 the page gives neither the labels nor numbers: **unconfirmed for v4** |
| Similarity | **75% (default)**; API `similarity_boost: 0.75` | Closest to the reference voice without the stiffness the v4 page warns about: "Higher values enforce closer adherence to the reference, which can come at the cost of some naturalness". | Default confirmed in the API reference (https://elevenlabs.io/docs/api-reference/text-to-speech/convert). Audible effect on v4 not measured |
| Style | **Not available; leave at 0** | Eleven v4 does not expose it. | Confirmed: "Style and Speed sliders are not available in Eleven v4" (v4 page above) and `GET /v1/models` says `can_use_style: false` for `eleven_v4` |
| Speaker boost | **Not available** | Same. | Confirmed: `can_use_speaker_boost: false` for `eleven_v4` in `GET /v1/models` |
| Speed | **Not available; do not rely on it** | The v4 page says there is no Speed slider. The API still accepts a `speed` field (documented range 0.7 to 1.2, default 1.0), but sending 1.15 for the same line gave a *longer* clip (2.69 s) than the default (2.51 s). Pacing is fixed after the fact instead: a clip that does not fit gets a gentle speed-up of at most 1.10x (`atempo`), see the "Note" column. | API test (one line, two settings). Documented range: https://elevenlabs.io/docs/overview/capabilities/text-to-speech/best-practices.md |
| Audio tags | **Sparse: only `[excited]` on the hook line of the highlights reel** (and its Spanish twin). Nothing else. | Roger is a laid-back voice; every tag is a chance of a laugh or a sigh nobody asked for. Tags also cost characters (the 43-character hook line, `[excited] ` included, was billed as 43). | Confirmed: v4 and v3 take audio tags such as `[excited]`, `[whispers]`, `[laughs]`, `[sighs]`, `[curious]` (https://elevenlabs.io/docs/help-center/product/core-capabilities/text-to-speech/how-do-audio-tags-work-with-eleven-v3-and-v4.md; the v4 page adds "tag-following is imperfect and still improving"). `[pause]` is **not** in the documented list (the guide's examples use `[Brief pause]`, `[short pause]`), so it is not used; SSML `<break>` is not supported on v4 ("Eleven v4 and Eleven v3 do not support SSML break tags"), pauses come from punctuation |
| Text | Numbers, BPM, times and file names written the way they are said ("one twenty-four B P M", "minus fourteen loofs"); brand said as "edit lab" | "Normalization is enabled by default for all TTS models", but the best-practices page still advises expanding numbers and abbreviations. Never read out `.wav`. | Documented (best-practices page). "edit lab" is my guess at how the name is said: change it in the text if the owner says it differently |
| One file per cue | **Yes.** Name each file after its cue: `R01.mp3`, `BT03.mp3`, `M05.mp3` | Each line lands exactly on its cue (`adelay`), a bad take is replaced alone, and a line can be retimed without touching the rest. | add-narration.sh looks for `<audio-dir>/<cue-id>.mp3` or `.wav` |
| Output format | **`mp3_44100_128`** (what the free plan serves). On a paid plan take `mp3_44100_192` (Creator and up) or **WAV/PCM 44.1 kHz** (Pro and up) | Highest quality offered per tier; the clips get mixed and re-encoded to AAC anyway, so the free format is already enough. | "MP3 with 192kbps bitrate requires you to be subscribed to Creator tier or above", PCM/WAV 44.1 kHz "require Pro tier or above" (API reference above) |
| Prosody between cues | Sequential generation with `previous_request_ids` (up to 3) and `next_text` | Keeps the delivery of neighbouring lines close. | The API accepted both on `eleven_v4` (HTTP 200). The stitching guide uses `eleven_v4` in its examples and says "Request stitching is not available for the `eleven_v3` model" (https://elevenlabs.io/docs/eleven-api/guides/how-to/text-to-speech/request-stitching.md). **Whether it audibly helps on v4 is unconfirmed** (nobody could listen here) |
| Word timings | `POST /v1/text-to-speech/{voice_id}/with-timestamps` | Gives start/end seconds per character of the text sent; that drives the word-synced captions and the `.srt` files. | API test: works on `eleven_v4`; tag characters are in the alignment (they are skipped when captions are built) |
| Language | Spanish version used `language_code: "es"` on `eleven_v4` | One voice, a second language. | API accepted it (HTTP 200); the docs say the field is "Ignored if the model doesn't support it", so whether v4 uses it or detects the language from the text is **unconfirmed** |
| Sound effects | `POST /v1/sound-generation`, duration set per effect | "40 credits per second when duration is specified" (https://elevenlabs.io/docs/overview/capabilities/sound-effects.md); three effects of 0.6 to 1.0 s: `sfx/whoosh.mp3`, `sfx/hit.mp3`, `sfx/ding.mp3` | Documented price is 40 credits per second, about 100 credits for the three. The account counter, though, moved by only about 24 credits beyond the text-to-speech characters, so what the free plan really charges for sound effects is **unconfirmed** (either way it is tiny) |

Speaking pace measured on Roger at these settings: about 2.0 to 2.4 words per second, slower after a colon ("Two:" takes
half a second). The brief's 2.4 words/s sizes `max_words`; where a real line needed more room the cut was lengthened
(reels) or the clip sped up by at most 1.10x.

### Using the ElevenLabs web app instead

Open Text to Speech, pick **Roger - Laid-Back, Casual, Resonant**, model **Eleven v4**, move Stability to the middle,
Similarity to about 75%. For each cue, paste the line from `transcripts/<cue-id>.txt` (the text is already in the form to be
spoken, tags included), generate once, download as MP3 44.1 kHz 128 kbps (or WAV 44.1 kHz on Pro), and save it as
`<cue-id>.mp3` in a folder. Then: `./add-narration.sh reel-highlights.mp4 reel-highlights-cues.json <folder> out.mp4`.
Lines with a Listen moment are lead-ins only (the cue says `voice_end`): the app's music plays after them, never talk over it.

### Facts the narration keeps to

Only what the recordings show: the DJ asks, the agent runs the tools, the timeline changes, the mix is exported. Stem
separation and transcription are never mentioned (neither is shipped), nor is any AI model name or version.

### Usage on the free plan

Everything below, retries and sound effects included, stayed under the 8,000-character budget:
- text-to-speech: 1842 characters in 55 calls (every distinct line of the reels, the three demos and the Spanish reel; a line that appears in several files was generated once). Included: one line regenerated because its first take had unparsed markup, and one line replaced by a shorter wording to fit its window
- one speed experiment: 43 characters (the Speed setting test described above)
- three sound effects: 111 credits counted in my ledger (40 credits per second of requested duration, plus a margin)
- total in my local ledger: **1996 of the 8,000-character budget** (the account's free allowance is 10,000 a month)
- the account's own counter (`GET /v1/user/subscription`, which lags the calls by a few seconds): `status: character_count=1909/10000`
- full per-call log: `usage.log`

## Cue ids

`R` highlights reel, `B` beatmatch reel, `E` intro reel, `M` mini-mix reel, `T` teaser (all in `reel-*-cues.json`);
`BT`, `EI`, `MM` are the three full website videos (`demo-<slug>-cues.json`). The Spanish reel reuses `R01` to `R11`
(`audio/reel-highlights-es/`, `transcripts/es/`). A line that is word for word the same in two files was generated once
and the clip is shared (same text, same audio). Table columns: **Words / max** is the spoken word count against the brief's
`floor(max_seconds x 2.4)` for the cue; **Spoken** is the measured length of the line from the character alignment
(`max_seconds` is the cue window minus 0.3 s). "(silent)" cues are the "Listen" moments, where the app's music plays.


## Reels


### Highlights reel (reel-highlights)

Video: `reel-highlights.mp4` / `reel-highlights-narrated.mp4`, 62.1 s. Cue file: `reel-highlights-cues.json`. Audio: `audio/reel-highlights/<cue-id>.mp3`.

| Cue | Window (s) | On screen | Narration (text sent to ElevenLabs) | Words / max | Spoken | Note |
|---|---|---|---|---|---|---|
| R01 | 0.00 to 3.00 | Describe the blend. Hear the mix. | [excited] Describe the blend. Hear the mix. | 6 / 6 | 2.56 s | title card |
| R02 | 3.00 to 8.40 | 2 · Beatmatch | Two: beatmatch. Neon Rush stretches down to one twenty. | 9 / 12 | 4.80 s |  |
| R03 | 8.40 to 13.40 | 4 · Blend: crossfade, low-pass on the outgoing track | Four: crossfade, with a low-pass on the outgoing track. | 9 / 11 | 4.32 s |  |
| R04 | 13.40 to 22.10 (voice to 15.40) | Listen: the transition, from 0:12 | Have a listen. | 3 / 4 | 0.88 s | lead-in only, silent over the music |
| R05 | 22.10 to 27.10 | 2 · Extend the intro | Two: extend the intro. Copy the first eight bars. | 9 / 11 | 4.32 s |  |
| R06 | 27.10 to 31.30 | 3 · A filter that opens into the drop | Three: a filter that opens into the drop. | 8 / 9 | 3.20 s |  |
| R07 | 31.30 to 41.80 (voice to 33.30) | Listen: the low end arrives at the drop (0:31) | Wait for the drop. | 4 / 4 | 1.28 s | lead-in only, silent over the music |
| R08 | 41.80 to 46.40 | 1 · Match every track to 124 BPM | First, match every track to one twenty-four B P M. | 10 / 10 | 4.00 s |  |
| R09 | 46.40 to 50.60 | 2 · Sequence with 4-bar overlaps | Second, sequence them with four-bar overlaps. | 6 / 9 | 3.52 s |  |
| R10 | 50.60 to 59.10 (voice to 52.60) | Listen: Midnight Drive into Solar Flare (0:23) | Here's the first blend. | 4 / 4 | 1.44 s | lead-in only, silent over the music |
| R11 | 59.10 to 62.10 | edytlab · Free, open source · edytlab.com · macOS · Windows · Linux | edit lab. Free and open source. | 6 / 6 | 2.70 s | end card; clip sped up 1.10x to keep 0.3 s of room |

### Reel: beatmatch and blend two tracks (reel-beatmatched-transition)

Video: `reel-beatmatched-transition.mp4` / `reel-beatmatched-transition-narrated.mp4`, 55.0 s. Cue file: `reel-beatmatched-transition-cues.json`. Audio: `audio/reel-beatmatched-transition/<cue-id>.mp3`.

| Cue | Window (s) | On screen | Narration (text sent to ElevenLabs) | Words / max | Spoken | Note |
|---|---|---|---|---|---|---|
| B01 | 0.00 to 3.00 | Beatmatch two tracks. Just ask. | Beatmatch two tracks. Just ask. | 5 / 6 | 2.72 s | title card |
| B02 | 3.00 to 7.90 | 1 · What am I working with? | One: ask what you're working with: tempo and key. | 9 / 11 | 4.24 s |  |
| B03 | 7.90 to 13.30 | 2 · Beatmatch | Two: beatmatch. Neon Rush stretches down to one twenty. | 9 / 12 | 4.80 s |  |
| B04 | 13.30 to 18.30 | 3 · Line up the overlap | Three: line up the overlap, under the last eight bars. | 10 / 11 | 4.32 s |  |
| B05 | 18.30 to 23.30 | 4 · Blend: crossfade, low-pass on the outgoing track | Four: crossfade, with a low-pass on the outgoing track. | 9 / 11 | 4.32 s |  |
| B06 | 23.30 to 35.30 (voice to 25.30) | Listen: the transition, from 0:12 | Have a listen. | 3 / 4 | 0.88 s | lead-in only, silent over the music |
| B07 | 35.30 to 39.50 | 5 · Master for streaming | Five: master it for streaming. | 5 / 9 | 2.64 s |  |
| B08 | 39.50 to 47.90 (voice to 41.90) | The mastered mix, from 0:20 | Now, the mastered mix. | 4 / 5 | 1.84 s | lead-in only, silent over the music |
| B09 | 47.90 to 52.00 | Done: midnight-into-neon.wav, 0:50, -14 LUFS | Done: fifty seconds, minus fourteen loofs. | 6 / 9 | 3.84 s |  |
| B10 | 52.00 to 55.00 | edytlab · Free, open source · edytlab.com · macOS · Windows · Linux | edit lab. Free and open source. | 6 / 6 | 2.70 s | end card; clip sped up 1.10x to keep 0.3 s of room |

### Reel: extend an intro for mixing (reel-extended-club-intro)

Video: `reel-extended-club-intro.mp4` / `reel-extended-club-intro-narrated.mp4`, 40.3 s. Cue file: `reel-extended-club-intro-cues.json`. Audio: `audio/reel-extended-club-intro/<cue-id>.mp3`.

| Cue | Window (s) | On screen | Narration (text sent to ElevenLabs) | Words / max | Spoken | Note |
|---|---|---|---|---|---|---|
| E01 | 0.00 to 3.00 | Extend an intro. Just ask. | Extend an intro. Just ask. | 5 / 6 | 2.64 s | title card |
| E02 | 3.00 to 7.20 | 1 · Tempo and bar length | One: check the tempo and bar length. | 7 / 9 | 3.28 s |  |
| E03 | 7.20 to 12.20 | 2 · Extend the intro | Two: extend the intro. Copy the first eight bars. | 9 / 11 | 4.32 s |  |
| E04 | 12.20 to 16.40 | 3 · A filter that opens into the drop | Three: a filter that opens into the drop. | 8 / 9 | 3.20 s |  |
| E05 | 16.40 to 20.60 | 4 · Fade in and export | Four: fade it in, and export. | 6 / 9 | 2.96 s |  |
| E06 | 20.60 to 33.10 (voice to 22.60) | Listen: the low end arrives at the drop (0:31) | Wait for the drop. | 4 / 4 | 1.28 s | lead-in only, silent over the music |
| E07 | 33.10 to 37.27 | Done: solar-flare-extended-intro.wav, a 16-bar intro | Done: a sixteen-bar intro, exported. | 5 / 9 | 3.52 s |  |
| E08 | 37.27 to 40.27 | edytlab · Free, open source · edytlab.com · macOS · Windows · Linux | edit lab. Free and open source. | 6 / 6 | 2.70 s | end card; clip sped up 1.10x to keep 0.3 s of room |

### Reel: a three-track mini-mix (reel-mini-mix)

Video: `reel-mini-mix.mp4` / `reel-mini-mix-narrated.mp4`, 44.2 s. Cue file: `reel-mini-mix-cues.json`. Audio: `audio/reel-mini-mix/<cue-id>.mp3`.

| Cue | Window (s) | On screen | Narration (text sent to ElevenLabs) | Words / max | Spoken | Note |
|---|---|---|---|---|---|---|
| M01 | 0.00 to 3.00 | Build a mini-mix. Just ask. | Build a mini-mix. Just ask. | 5 / 6 | 2.56 s | title card |
| M02 | 3.00 to 7.60 | 1 · Match every track to 124 BPM | First, match every track to one twenty-four B P M. | 10 / 10 | 4.00 s |  |
| M03 | 7.60 to 11.80 | 2 · Sequence with 4-bar overlaps | Second, sequence them with four-bar overlaps. | 6 / 9 | 3.52 s |  |
| M04 | 11.80 to 16.00 | 3 · Match loudness, limit the master | Third, match the loudness, and limit the master. | 8 / 9 | 3.60 s |  |
| M05 | 16.00 to 24.50 (voice to 18.00) | Listen: Midnight Drive into Solar Flare (0:23) | Here's the first blend. | 4 / 4 | 1.44 s | lead-in only, silent over the music |
| M06 | 24.50 to 33.00 (voice to 26.50) | Listen: Solar Flare into Neon Rush (0:47) | And the second one. | 4 / 4 | 1.28 s | lead-in only, silent over the music |
| M07 | 33.00 to 37.20 | 4 · Export the mix | Fourth, export the mix. | 4 / 9 | 2.24 s |  |
| M08 | 37.20 to 41.20 | Done: mini-mix-124bpm.wav, 1:21 | Done: one minute twenty-one, exported. | 5 / 8 | 3.28 s |  |
| M09 | 41.20 to 44.20 | edytlab · Free, open source · edytlab.com · macOS · Windows · Linux | edit lab. Free and open source. | 6 / 6 | 2.70 s | end card; clip sped up 1.10x to keep 0.3 s of room |

### Teaser (reel-teaser)

Video: `reel-teaser.mp4` / `reel-teaser-narrated.mp4`, 14.0 s. Cue file: `reel-teaser-cues.json`. Audio: `audio/reel-teaser/<cue-id>.mp3`.

| Cue | Window (s) | On screen | Narration (text sent to ElevenLabs) | Words / max | Spoken | Note |
|---|---|---|---|---|---|---|
| T01 | 0.00 to 2.00 | Ask for the blend. | Ask for the blend. | 4 / 4 | 1.44 s | title card |
| T02 | 2.00 to 4.30 | 2 · Beatmatch | Beatmatch the two tracks. | 4 / 4 | 1.76 s |  |
| T03 | 4.30 to 6.50 | 4 · Blend: crossfade, low-pass on the outgoing track | Blend with a crossfade. | 4 / 4 | 1.68 s |  |
| T04 | 6.50 to 11.00 | Listen: the transition, from 0:12 | (silent) | - | - | silent: the app's music plays here, narration would talk over it |
| T05 | 11.00 to 14.00 | edytlab · Free, open source · edytlab.com · macOS · Windows · Linux | edit lab. Free, open source. | 5 / 6 | 2.76 s | end card; clip sped up 1.10x to keep 0.3 s of room |

### Spanish highlights reel (reel-highlights-es)

Video: `reel-highlights-es-narrated.mp4` (no un-narrated Spanish video: its cuts are timed to the Spanish speech), 62.3 s. Spanish narration, the same shots with cut lengths set by the Spanish speech. Cue file: `reel-highlights-es-cues.json`. Audio: `audio/reel-highlights-es/<cue-id>.mp3`.

| Cue | Window (s) | On screen | Narration (text sent to ElevenLabs) | Words / max | Spoken | Note |
|---|---|---|---|---|---|---|
| R01 | 0.00 to 2.90 | Describe la mezcla. Escúchala. | [excited] Describe la mezcla. Escúchala. | 4 / 6 | 2.40 s | title card |
| R02 | 2.90 to 8.00 | 2 · Beatmatch | Dos: beatmatch. Neon Rush baja a ciento veinte. | 8 / 11 | 4.48 s |  |
| R03 | 8.00 to 12.90 | 4 · Blend: crossfade, low-pass on the outgoing track | Cuatro: crossfade, con un pasa-bajos en la pista que sale. | 10 / 11 | 4.24 s |  |
| R04 | 12.90 to 21.60 (voice to 14.90) | Escucha: la transición, desde el 0:12 | Escucha. | 1 / 4 | 0.80 s | lead-in only, silent over the music |
| R05 | 21.60 to 27.00 | 2 · Extend the intro | Dos: alarga la intro. Copia los primeros ocho compases. | 9 / 12 | 4.80 s |  |
| R06 | 27.00 to 31.20 | 3 · A filter that opens into the drop | Tres: un filtro que se abre en el drop. | 9 / 9 | 2.88 s |  |
| R07 | 31.20 to 41.70 (voice to 33.20) | Escucha: el grave llega con el drop (0:31) | Espera el drop. | 3 / 4 | 1.04 s | lead-in only, silent over the music |
| R08 | 41.70 to 46.90 | 1 · Match every track to 124 BPM | Uno: iguala cada pista a ciento veinticuatro B P M. | 10 / 11 | 4.56 s |  |
| R09 | 46.90 to 51.10 | 2 · Sequence with 4-bar overlaps | Dos: secuencia con solapes de cuatro compases. | 7 / 9 | 3.60 s |  |
| R10 | 51.10 to 59.60 (voice to 53.10) | Escucha: Midnight Drive entra en Solar Flare (0:23) | Así suena. | 2 / 4 | 0.96 s | lead-in only, silent over the music |
| R11 | 59.60 to 62.30 | edytlab · Gratis y de código abierto · edytlab.com · macOS · Windows · Linux | Gratis y de código abierto. | 5 / 5 | 2.24 s | end card |

## Full website demo videos


### Beatmatch and blend two tracks (demo-dj-beatmatched-transition)

Video: `dj-beatmatched-transition.mp4` (the website video, untouched) / `demo-dj-beatmatched-transition-narrated.mp4`, 187.7 s. Cue file: `demo-dj-beatmatched-transition-cues.json`. Audio: `audio/demo-dj-beatmatched-transition/<cue-id>.mp3`.

| Cue | Window (s) | On screen | Narration (text sent to ElevenLabs) | Words / max | Spoken | Note |
|---|---|---|---|---|---|---|
| BT01 | 0.00 to 2.40 | A DJ blends Midnight Drive (120 BPM) into Neon Rush (128 BPM) | Blending two tracks. | 3 / 5 | 1.68 s |  |
| BT02 | 2.40 to 10.20 | Open the tracks | First, open both tracks. | 4 / 18 | 2.24 s |  |
| BT03 | 10.20 to 22.33 (voice to 15.06) | Before: both tracks from the top, 120 against 128 BPM | From the top: one twenty against one twenty-eight. | 8 / 10 | 4.00 s | lead-in only, silent over the music |
| BT04 | 22.33 to 35.73 | 1 · What am I working with? | One: ask what you're working with: tempo and key. | 9 / 31 | 4.24 s |  |
| BT05 | 35.73 to 48.60 | 2 · Beatmatch | Two: beatmatch. Neon Rush stretches down to one twenty. | 9 / 30 | 4.80 s |  |
| BT06 | 48.60 to 63.87 | 3 · Line up the overlap | Three: line up the overlap, under the last eight bars. | 10 / 35 | 4.32 s |  |
| BT07 | 63.87 to 87.73 | 4 · Blend: crossfade, low-pass on the outgoing track | Four: crossfade, with a low-pass on the outgoing track. | 9 / 56 | 4.32 s |  |
| BT08 | 87.73 to 110.67 (voice to 92.30) | Listen: the transition, from 0:12 | Have a listen. | 3 / 10 | 0.88 s | lead-in only, silent over the music |
| BT09 | 110.67 to 155.93 | 5 · Master for streaming | Five: master it for streaming. | 5 / 107 | 2.64 s |  |
| BT10 | 155.93 to 167.00 | 6 · Export | Six: export the file. | 4 / 25 | 2.24 s |  |
| BT11 | 167.00 to 183.60 (voice to 171.31) | The mastered mix, from 0:20 | Now, the mastered mix. | 4 / 9 | 1.84 s | lead-in only, silent over the music |
| BT12 | 183.60 to 187.73 | Done: midnight-into-neon.wav, 0:50, -14 LUFS | Done: fifty seconds, minus fourteen loofs. | 6 / 9 | 3.84 s |  |

### Extend an intro for mixing (demo-dj-extended-club-intro)

Video: `dj-extended-club-intro.mp4` (the website video, untouched) / `demo-dj-extended-club-intro-narrated.mp4`, 115.9 s. Cue file: `demo-dj-extended-club-intro-cues.json`. Audio: `audio/demo-dj-extended-club-intro/<cue-id>.mp3`.

| Cue | Window (s) | On screen | Narration (text sent to ElevenLabs) | Words / max | Spoken | Note |
|---|---|---|---|---|---|---|
| EI01 | 0.00 to 2.40 | A DJ makes a mixable intro for Solar Flare (124 BPM) | A mixable intro. | 3 / 5 | 1.52 s |  |
| EI02 | 2.40 to 9.40 | Open the tracks | First, open the track. | 4 / 16 | 1.84 s |  |
| EI03 | 9.40 to 22.40 (voice to 14.15) | Before: the original intro | First, the original intro. | 4 / 10 | 2.16 s | lead-in only, silent over the music |
| EI04 | 22.40 to 34.73 | 1 · Tempo and bar length | One: check the tempo and bar length. | 7 / 28 | 3.28 s |  |
| EI05 | 34.73 to 53.60 | 2 · Extend the intro | Two: extend the intro. Copy the first eight bars. | 9 / 44 | 4.32 s |  |
| EI06 | 53.60 to 67.20 | 3 · A filter that opens into the drop | Three: a filter that opens into the drop. | 8 / 31 | 3.20 s |  |
| EI07 | 67.20 to 81.53 | 4 · Fade in and export | Four: fade it in, and export. | 6 / 33 | 2.96 s |  |
| EI08 | 81.53 to 95.27 (voice to 86.03) | Listen: the new intro fades in | Listen to the intro fade in. | 6 / 10 | 2.00 s | lead-in only, silent over the music |
| EI09 | 95.27 to 111.67 (voice to 99.51) | Listen: the low end arrives at the drop (0:31) | Wait for the drop. | 4 / 9 | 1.28 s | lead-in only, silent over the music |
| EI10 | 111.67 to 115.87 | Done: solar-flare-extended-intro.wav, a 16-bar intro | Done: a sixteen-bar intro, exported. | 5 / 9 | 3.52 s |  |

### A three-track mini-mix (demo-dj-mini-mix)

Video: `dj-mini-mix.mp4` (the website video, untouched) / `demo-dj-mini-mix-narrated.mp4`, 137.8 s. Cue file: `demo-dj-mini-mix-cues.json`. Audio: `audio/demo-dj-mini-mix/<cue-id>.mp3`.

| Cue | Window (s) | On screen | Narration (text sent to ElevenLabs) | Words / max | Spoken | Note |
|---|---|---|---|---|---|---|
| MM01 | 0.00 to 2.47 | A DJ builds a three-track mini-mix at 124 BPM | A three-track mini-mix. | 3 / 5 | 1.84 s |  |
| MM02 | 2.47 to 10.33 | Open the tracks | First, open the three tracks. | 5 / 18 | 2.40 s |  |
| MM03 | 10.33 to 23.40 | 1 · Match every track to 124 BPM | First, match every track to one twenty-four B P M. | 10 / 30 | 4.00 s |  |
| MM04 | 23.40 to 60.00 | 2 · Sequence with 4-bar overlaps | Second, sequence them with four-bar overlaps. | 6 / 87 | 3.52 s |  |
| MM05 | 60.00 to 86.80 | 3 · Match loudness, limit the master | Third, match the loudness, and limit the master. | 8 / 63 | 3.60 s |  |
| MM06 | 86.80 to 104.93 (voice to 91.75) | Listen: Midnight Drive into Solar Flare (0:23) | Here's the first blend. | 4 / 11 | 1.44 s | lead-in only, silent over the music |
| MM07 | 104.93 to 122.67 (voice to 109.29) | Listen: Solar Flare into Neon Rush (0:47) | And the second one. | 4 / 9 | 1.28 s | lead-in only, silent over the music |
| MM08 | 122.67 to 133.73 | 4 · Export the mix | Fourth, export the mix. | 4 / 25 | 2.24 s |  |
| MM09 | 133.73 to 137.80 | Done: mini-mix-124bpm.wav, 1:21 | Done: one minute twenty-one, exported. | 5 / 9 | 3.28 s |  |
