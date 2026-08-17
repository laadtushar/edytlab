/**
 * Snapping a cut boundary to the nearest zero crossing (#161).
 *
 * A cut in the middle of a waveform leaves a step discontinuity where
 * the two sides meet, and a step is a click — broadband, and audible
 * even when the edit is otherwise perfect. Moving the boundary a few
 * milliseconds to a point where the signal is already at zero removes
 * the step entirely. This is why Audacity has had it for twenty years:
 * it is a *quality* feature, not a convenience one.
 *
 * The maths is deliberately dull. What matters is the policy:
 *
 * - **Search a window, not the whole file.** A crossing 400 ms away is
 *   not the edit the user asked for. Ten milliseconds is roughly one
 *   cycle at 100 Hz, so anything with energy has a crossing inside it,
 *   and anything that does not is quiet enough that the step is
 *   inaudible anyway.
 * - **Nearest wins, in either direction.** Snapping only forwards
 *   biases every edit late, which accumulates across a session.
 * - **No crossing found means no snap.** Returning the original time is
 *   the honest answer; inventing a boundary would move the edit
 *   somewhere the user did not choose.
 */

/** How far either side of the requested point to look, in seconds. */
export const DEFAULT_SEARCH_WINDOW_SEC = 0.01;

/**
 * The nearest sample index at or after `i` where the signal crosses
 * zero, searching outwards from `centre`.
 *
 * A crossing is a sign change between consecutive samples, or a sample
 * that is exactly zero. Both are points where the waveform can be cut
 * without a step.
 */
function isCrossing(samples: Float32Array, i: number): boolean {
  if (i <= 0 || i >= samples.length) return false;
  const prev = samples[i - 1];
  const cur = samples[i];
  if (cur === 0) return true;
  return (prev < 0 && cur >= 0) || (prev > 0 && cur <= 0);
}

/**
 * A sign change sits *between* two samples, so there are two candidates
 * to cut on. Take whichever is closer to zero: it is the one that
 * leaves the smaller step, which is the entire point of snapping.
 */
function bestOfPair(samples: Float32Array, i: number): number {
  if (i <= 0) return i;
  return Math.abs(samples[i]) <= Math.abs(samples[i - 1]) ? i : i - 1;
}

/**
 * Move `timeSec` to the nearest zero crossing within `windowSec`.
 *
 * Returns the original time when there is no crossing in range, when
 * the buffer is empty, or when the time is outside it — every one of
 * those is a case where snapping would move the edit somewhere the user
 * did not ask for.
 */
export function snapToZeroCrossing(
  samples: Float32Array,
  sampleRate: number,
  timeSec: number,
  windowSec: number = DEFAULT_SEARCH_WINDOW_SEC,
): number {
  if (!samples.length || sampleRate <= 0 || !Number.isFinite(timeSec)) {
    return timeSec;
  }

  const centre = Math.round(timeSec * sampleRate);
  if (centre < 0 || centre >= samples.length) return timeSec;

  const radius = Math.max(1, Math.round(windowSec * sampleRate));

  // Outwards from the centre so the first hit is the nearest one. The
  // forward candidate is checked before the backward one at equal
  // distance, which is arbitrary but has to be decided somewhere.
  for (let d = 0; d <= radius; d++) {
    const forward = centre + d;
    if (forward < samples.length && isCrossing(samples, forward)) {
      return bestOfPair(samples, forward) / sampleRate;
    }
    const back = centre - d;
    if (back >= 0 && isCrossing(samples, back)) {
      return bestOfPair(samples, back) / sampleRate;
    }
  }

  return timeSec;
}

export interface SnappableRange {
  start: number;
  end: number;
}

/**
 * Snap both edges of a range.
 *
 * Returns the range unchanged if snapping would collapse or invert it —
 * a selection whose edges land on the same crossing is not a selection,
 * and silently emptying one would be worse than leaving it a sample off
 * a crossing.
 */
export function snapRange(
  samples: Float32Array,
  sampleRate: number,
  range: SnappableRange,
  windowSec: number = DEFAULT_SEARCH_WINDOW_SEC,
): SnappableRange {
  const start = snapToZeroCrossing(samples, sampleRate, range.start, windowSec);
  const end = snapToZeroCrossing(samples, sampleRate, range.end, windowSec);
  if (end <= start) return range;
  return { start, end };
}
