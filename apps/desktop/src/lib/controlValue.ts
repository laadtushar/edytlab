/**
 * Whether a slider value already is what the session holds.
 *
 * A gain or pan slider commits on pointer-up, on key-up and on blur, and
 * most of those carry a value that has not changed: tabbing through the
 * slider, or releasing the Ctrl and Z keys of an undo made while it has
 * focus. The key-up of that undo arrives before the track list has been
 * refreshed, so it carries the slider's stale value and would write it
 * back as a new edit, redoing what was just undone. A commit that
 * matches the session's own value is not an edit and is skipped.
 *
 * Compared with a tolerance because the session stores `f32`: a pan of
 * 0.02 comes back as 0.019999999552965164. Real steps are 0.5 dB and
 * 0.02, far above the tolerance.
 */
const TOLERANCE = 1e-4;

export function isSessionValue(session: number | undefined, value: number): boolean {
  return session !== undefined && Math.abs(session - value) < TOLERANCE;
}
