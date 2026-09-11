/**
 * AuditionPlayer — hear an `audition_effect` excerpt in the transcript
 * (#258).
 *
 * `audition_effect` renders the session as it *would* sound with an
 * effect added to a track's chain, and appends nothing to the store.
 * The render already worked; what was missing was any way to hear it.
 * The tool returned an absolute path, the path was printed into the
 * chat as JSON, and listening meant leaving the app and opening the
 * file in another player — so the "evaluate before you commit" workflow
 * the tool exists for was unchanged for anyone actually using the app.
 *
 * This is the player. It sits under the tool badge that produced it,
 * because an excerpt belongs to the call that rendered it rather than
 * to whatever the assistant said next.
 *
 * Two things worth knowing:
 *
 * *`convertFileSrc`.* The path is a real path on disk, and the webview
 * cannot open one. Tauri's asset protocol turns it into a URL the
 * `<audio>` element can load; without this it fails silently, which
 * looks exactly like the bug this component fixes.
 *
 * *It is not the transport.* Playing an audition deliberately does not
 * touch the timeline playhead or the mix player. The point of an
 * audition is to compare against what you already have, and hijacking
 * the main transport to do it would lose your place.
 */

import { convertFileSrc } from "@tauri-apps/api/core";
import { useMemo } from "react";

interface AuditionPlayerProps {
  /** Absolute path to the rendered excerpt. */
  path: string;
  /** The effect kind being auditioned, e.g. `low_pass_filter`. */
  kind: string;
  track: number;
  startSec: number;
  endSec: number;
  /** The tool's own summary line, shown as a caption. */
  caption?: string;
}

/** `12.5` → `12.50s`, so the two ends of a range line up. */
const secs = (n: number): string => `${n.toFixed(2)}s`;

export function AuditionPlayer({
  path,
  kind,
  track,
  startSec,
  endSec,
  caption,
}: AuditionPlayerProps) {
  // Re-derived only when the path changes: nudging a parameter renders
  // a new excerpt at a new content-addressed path, which is what makes
  // the element reload rather than replay the previous audio.
  const src = useMemo(() => convertFileSrc(path), [path]);

  const label = `Audition of ${kind} on track ${track}, ${secs(startSec)} to ${secs(endSec)}`;

  return (
    <figure
      data-testid="audition-player"
      className="m-0 flex w-full max-w-sm flex-col gap-1 rounded-md border border-[var(--border)] p-2"
    >
      <figcaption className="flex items-baseline justify-between gap-2 text-[10px] text-[var(--text-faint)]">
        <span className="font-mono uppercase tracking-[0.14em]">{kind}</span>
        <span>
          track {track} · {secs(startSec)}–{secs(endSec)}
        </span>
      </figcaption>
      {/* eslint-disable-next-line jsx-a11y/media-has-caption --
          a rendered audio excerpt has no captions to offer; the
          aria-label carries what a caption would say. */}
      <audio
        data-testid="audition-audio"
        controls
        preload="metadata"
        src={src}
        aria-label={label}
        className="w-full"
      />
      {caption ? (
        <span
          data-testid="audition-caption"
          className="text-[10px] text-[var(--text-faint)]"
        >
          {caption}
        </span>
      ) : null}
    </figure>
  );
}
