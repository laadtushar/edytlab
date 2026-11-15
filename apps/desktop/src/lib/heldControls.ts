import type { TrackSummary } from "./tauri-bridge";
import { isSessionValue } from "./controlValue";

/**
 * The lane controls a user has set that the session has not yet confirmed
 * (#596), as a pure function of what `useSession` holds.
 *
 * A gain or pan slider, a mute button, a solo button: each shows the value
 * the user gave it at once and commits it afterwards (a slider on key-up,
 * on pointer-up and on blur; a button on the click), and each commit is
 * followed by a read of the track list. The timeline draws whatever list it
 * is handed, so a list that was already on its way, requested before the
 * user's last input and landing after it, carried the older value and put
 * it back under the control: a flicker, or a lost key tap, since the key-up
 * then committed a value the session already held and sent nothing.
 *
 * One rule for every control: **from the moment a user sets a control until
 * the session has confirmed it, that control is the user's, and a track list
 * that arrives does not overwrite it.** `overlay` puts those values over a
 * list on its way to the timeline.
 *
 * Nothing here is state. A held value is one of two things `useSession`
 * already has:
 *
 * - the slider's input value before its commit (a draft); and
 * - the `variables` of a pending mutation in the session's scope: the edit
 *   that commits it, then the `confirm` that waits for the first track read
 *   asked for after the edit was answered. That read is the session's own
 *   word on the control, so the value is handed back when it lands. A
 *   refused commit ends the same way, with the session's real value.
 *
 * When the session moves by itself (an undo, a redo, Set as head, Accept B,
 * a project opening, a node the assistant created) `useSession` lets go of
 * every hold at once: what the user had set belonged to the session before
 * the move.
 */

export type ControlName = "gain" | "pan" | "muted" | "soloed";
export type ControlValue = number | boolean;

/** The field of a track list entry each control is drawn from. */
const FIELD = {
  gain: "gain_db",
  pan: "pan",
  muted: "muted",
  soloed: "soloed",
} as const satisfies Record<ControlName, keyof TrackSummary>;

/** One held control: what the user set it to, on the track it was set on. */
export interface Hold {
  /** The track's id (`trackKey`), not its position. */
  readonly track: string;
  readonly control: ControlName;
  readonly value: ControlValue;
}

/**
 * The key a track's controls are held under. The track's id, not its
 * position: a removal or an undo moves positions, and a control the user
 * set belongs to the track it was set on.
 */
export function trackKey(tracks: readonly TrackSummary[], index: number): string | null {
  const track = tracks[index];
  return track ? (track.id ?? `#${index}`) : null;
}

/** The identity of a held control, for comparing holds. */
export const holdKey = (hold: Pick<Hold, "track" | "control">): string =>
  `${hold.track}\u0000${hold.control}`;

function sameValue(a: ControlValue | undefined, b: ControlValue): boolean {
  return typeof b === "number" ? typeof a === "number" && isSessionValue(a, b) : a === b;
}

/**
 * `tracks` with the user's values over the session's, for the controls the
 * user holds. `holds` run oldest first: for one control the last wins.
 * Answers `tracks` itself when nothing is held, so a list nobody touched
 * keeps its identity.
 */
export function overlay(tracks: TrackSummary[], holds: readonly Hold[]): TrackSummary[] {
  if (holds.length === 0) return tracks;
  const latest = new Map<string, ControlValue>();
  for (const hold of holds) latest.set(holdKey(hold), hold.value);
  return tracks.map((track, index) => {
    const id = trackKey(tracks, index) as string;
    let drawn = track;
    for (const control of Object.keys(FIELD) as ControlName[]) {
      const value = latest.get(holdKey({ track: id, control }));
      if (value !== undefined && !sameValue(drawn[FIELD[control]], value)) {
        drawn = { ...drawn, [FIELD[control]]: value };
      }
    }
    return drawn;
  });
}

/** The last value held for a control, if any: what the session will hold once the commits in line are applied. */
export function lastHeld(
  holds: readonly Hold[],
  track: string,
  control: ControlName,
): ControlValue | undefined {
  for (let i = holds.length - 1; i >= 0; i--) {
    if (holds[i].track === track && holds[i].control === control) return holds[i].value;
  }
  return undefined;
}
