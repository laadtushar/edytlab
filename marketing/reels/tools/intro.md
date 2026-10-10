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
@@USAGE@@

## Cue ids

`R` highlights reel, `B` beatmatch reel, `E` intro reel, `M` mini-mix reel, `T` teaser (all in `reel-*-cues.json`);
`BT`, `EI`, `MM` are the three full website videos (`demo-<slug>-cues.json`). The Spanish reel reuses `R01` to `R11`
(`audio/reel-highlights-es/`, `transcripts/es/`). A line that is word for word the same in two files was generated once
and the clip is shared (same text, same audio). Table columns: **Words / max** is the spoken word count against the brief's
`floor(max_seconds x 2.4)` for the cue; **Spoken** is the measured length of the line from the character alignment
(`max_seconds` is the cue window minus 0.3 s). "(silent)" cues are the "Listen" moments, where the app's music plays.
