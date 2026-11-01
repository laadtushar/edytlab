# LinkedIn: v0.9.3 and the DJ demo

**Links:** site <https://edytlab.com> · download
<https://github.com/laadtushar/edytlab/releases/latest>

Upload `website/public/demos/dj-beatmatched-transition.mp4` as native video.
LinkedIn de-prioritises external links in the body, so the links go in the first
comment, which you post right after publishing.

## Post

About 175 words. The single-post version of the launch, for when you would
rather post once than follow the five-post plan.

```
You know what you want the transition to sound like. Getting the software to do it is the slow part.

I've released edytlab 0.9.3, a free, open-source audio editor you drive by describing what you want.

The demo is a DJ transition, asked for in six short requests. Get both tracks' tempos, match the incoming track to the outgoing one, start it 8 bars before the outgoing one ends, crossfade with a low-pass on the old track, then compress the mix, bring it to -14 LUFS and export a WAV. The AI turns that into editing steps and shows you each one. The recording is 2:23 with sound, and I sped up the waits.

The part I care about most: the AI never touches your audio. It reads a summary of the session and picks the tools. The editing runs on your computer, so your files aren't uploaded. And every edit can be undone.

It edits prepared files. It isn't a live performance tool.

If you work with audio, which edit would you ask for first?

#DJing #AudioEditing #OpenSource
```

## First comment

```
Download for macOS, Windows and Linux: https://github.com/laadtushar/edytlab/releases/latest
The installers aren't signed yet, so expect a warning the first time you open it.

Demo, docs and the DJ walkthrough: https://edytlab.com
Source (MIT): https://github.com/laadtushar/edytlab
```

## Image

`../social-kit/linkedin/out/post-1-launch.png` (1200x1200): the app's chat panel
with a scripted example conversation (the image says so) and the three numbers
from the site. Use it as the video's cover if the uploader
offers a custom thumbnail, or post it on its own if you skip the video. How it
is made, and what each number is read from, is in
[`../social-kit/linkedin/README.md`](../social-kit/linkedin/README.md).

## Optional second post

Same format, built around the extended-intro demo with sound.
`https://edytlab.com/demos/dj-extended-club-intro.mp4`. Describe only what the recording shows; read the
caption in `website/lib/demos.ts` and quote it, rather than writing from
memory.
