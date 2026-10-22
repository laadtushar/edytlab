/**
 * SubRip (`.srt`) captions as WebVTT, which is what a `<track>` reads.
 *
 * The two formats differ here in three ways only: WebVTT starts with a
 * `WEBVTT` line, a time has a point where SubRip has a comma
 * (`00:00:04,350` becomes `00:00:04.350`), and line endings are `\n`.
 * Cue numbers are optional in WebVTT and are kept. The text of a cue is
 * passed through as it is, a comma in the words included: only a line
 * that is a time range is rewritten.
 *
 * `videos.test.ts` converts the source `.srt` with this and compares the
 * result to `public/videos/launch-video.vtt`, so a re-cut's new captions
 * cannot be left unconverted.
 */
export function srtToVtt(srt: string): string {
  const lines = srt
    .replace(/^﻿/, "")
    .replace(/\r\n?/g, "\n")
    .trimEnd()
    .split("\n")
    .map((line) =>
      /^\d{2}:\d{2}:\d{2},\d{3} --> \d{2}:\d{2}:\d{2},\d{3}/.test(line) ? line.replace(/,(\d{3})/g, ".$1") : line,
    );
  return `WEBVTT\n\n${lines.join("\n")}\n`;
}
