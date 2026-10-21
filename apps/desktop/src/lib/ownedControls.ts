import type { TrackSummary } from "./tauri-bridge";
import { isSessionValue } from "./controlValue";

/**
 * The lane controls a user has set that the session has not yet confirmed
 * (#596).
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
 * A control goes through these steps:
 *
 * - `set`: the user changed it (a slider's input event). It is *unsent*.
 * - `send`: its commit went out (a slider's key-up, a button's click). It is
 *   *in flight* until the command has been answered, by the session taking
 *   the edit or refusing it (`finish`).
 * - `applied`: a track list read **after** the commit finished has landed.
 *   That list is the session's own word on the control, so the user's value
 *   is handed back. A refused commit ends here too, with the session's real
 *   value on the control rather than the one it did not take.
 *
 * A list read before the commit finished is not that word, whatever order
 * the lists arrive in: each read has a number, and `finish` records the
 * first one that was asked for after the commit was answered. Only a list
 * with that number or a later one lets go.
 *
 * `clear` lets go of everything at once, for when the session moves by
 * itself: an undo, a redo, a head set from the history graph, a project
 * opening. What the user had set belonged to the session before the move.
 *
 * The last value *sent* for a control is kept until the control is let go.
 * It is what the session will hold once the commits in line have been
 * applied, which the slider compares with before sending, so a value that is
 * already on its way is not sent twice and one that differs from it is, even
 * when the last list read still shows an older value (#498).
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

/** What `send` hands back, for `finish` to name the commit it ends. */
export interface Claim {
  readonly key: string;
  readonly epoch: number;
}

interface Entry {
  /** What the user last set the control to: what the lane draws. */
  value: ControlValue;
  /** Set since the last commit went out, so the session has not been asked for it. */
  unsent: boolean;
  /** The value of the last commit sent for the control. */
  sent: ControlValue | undefined;
  /** Commits sent and not yet answered. */
  inFlight: number;
  /**
   * Once every commit has been answered: the number of the first track-list
   * read asked for after that. Null until then.
   */
  confirmedBy: number | null;
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

const keyOf = (track: string, control: ControlName) => `${track}\u0000${control}`;

function sameValue(a: ControlValue | undefined, b: ControlValue): boolean {
  return typeof b === "number" ? typeof a === "number" && isSessionValue(a, b) : a === b;
}

export class OwnedControls {
  private entries = new Map<string, Entry>();
  private epoch = 0;

  /**
   * @param changed called when a control is handed back to the session, so
   *   the caller draws the list again without the user's value over it. Not
   *   called by `set` or `send`: what they hold is already on screen.
   */
  constructor(private readonly changed: () => void) {}

  /** The user changed a control; it is theirs until the session confirms it. */
  set(track: string, control: ControlName, value: ControlValue): void {
    const entry = this.entries.get(keyOf(track, control));
    if (entry) {
      entry.value = value;
      entry.unsent = true;
    } else {
      this.entries.set(keyOf(track, control), {
        value,
        unsent: true,
        sent: undefined,
        inFlight: 0,
        confirmedBy: null,
      });
    }
  }

  /** The last value sent for a control that has not been handed back, if any. */
  sent(track: string, control: ControlName): ControlValue | undefined {
    return this.entries.get(keyOf(track, control))?.sent;
  }

  /** A commit of `value` is going out. Pass the claim to `finish` when it is answered. */
  send(track: string, control: ControlName, value: ControlValue): Claim {
    const key = keyOf(track, control);
    let entry = this.entries.get(key);
    if (!entry) {
      entry = { value, unsent: false, sent: value, inFlight: 0, confirmedBy: null };
      this.entries.set(key, entry);
    }
    entry.value = value;
    entry.sent = value;
    entry.unsent = false;
    entry.inFlight++;
    return { key, epoch: this.epoch };
  }

  /**
   * A release, key-up or blur whose value needed no commit: it is what the
   * session holds, or what a commit already on its way will leave it with.
   * Nothing is unsent any more; with nothing on its way either, the control
   * is the session's again.
   */
  skip(track: string, control: ControlName): void {
    const key = keyOf(track, control);
    const entry = this.entries.get(key);
    if (!entry) return;
    entry.unsent = false;
    if (entry.inFlight === 0 && entry.sent === undefined) {
      this.entries.delete(key);
      this.changed();
    }
  }

  /**
   * The commit has been answered, taken or refused, and the track list is
   * about to be read again. `firstRead` is the number of that read: the
   * first one asked for after the answer.
   */
  finish(claim: Claim, firstRead: number): void {
    if (claim.epoch !== this.epoch) return;
    const entry = this.entries.get(claim.key);
    if (!entry) return;
    entry.inFlight--;
    entry.confirmedBy = Math.max(entry.confirmedBy ?? 0, firstRead);
  }

  /**
   * The track list read number `request` has landed. It hands back every
   * control whose commits were answered before that read was asked for and
   * that the user has not touched since. Call it in the same batch as the
   * state that holds the list, so the two are drawn together.
   */
  applied(request: number): void {
    let released = false;
    for (const [key, entry] of this.entries) {
      if (
        !entry.unsent &&
        entry.inFlight === 0 &&
        entry.confirmedBy !== null &&
        entry.confirmedBy <= request
      ) {
        this.entries.delete(key);
        released = true;
      }
    }
    if (released) this.changed();
  }

  /** The session moved by itself: nothing set before belongs to what it is now. */
  clear(): void {
    this.epoch++;
    if (this.entries.size === 0) return;
    this.entries.clear();
    this.changed();
  }

  /** `tracks` with the user's values over the session's, for the controls the user holds. */
  overlay(tracks: TrackSummary[]): TrackSummary[] {
    if (this.entries.size === 0) return tracks;
    return tracks.map((track, index) => {
      let drawn = track;
      for (const control of Object.keys(FIELD) as ControlName[]) {
        const entry = this.entries.get(keyOf(trackKey(tracks, index) as string, control));
        if (entry && !sameValue(drawn[FIELD[control]], entry.value)) {
          drawn = { ...drawn, [FIELD[control]]: entry.value };
        }
      }
      return drawn;
    });
  }
}
