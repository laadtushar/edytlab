/**
 * Recording outcomes, as values rather than side effects (#248).
 *
 * These lived inline in `App.tsx` as `try { … } catch (e) {
 * console.error(…) }` — the only two handlers in the file that reported
 * to the console instead of the error banner, which is the app's single
 * error surface. In a packaged desktop build there is no console to
 * read, so a failed Record was indistinguishable from a dead button.
 *
 * They are here so the outcomes can be asserted: `App.tsx` mounts a
 * Tauri surface a unit test cannot render (#273), and these functions
 * are the part worth testing — the mapping from a bridge failure to
 * what the user is told.
 */

/** What happened when the user pressed Record. */
export type StartOutcome =
  | { kind: "recording" }
  | { kind: "failed"; message: string };

/**
 * What happened when the user pressed Stop.
 *
 * `saveFailed` and `loadFailed` are deliberately distinct. If the WAV
 * was written but importing it failed, the take is **on disk** — naming
 * the path is far more useful than calling it lost. Only `saveFailed`
 * means the audio is actually gone.
 */
export type StopOutcome =
  | { kind: "loaded"; path: string; nodeId: string | null }
  | { kind: "saveFailed"; message: string }
  | { kind: "loadFailed"; path: string; message: string };

export async function startTake(start: () => Promise<unknown>): Promise<StartOutcome> {
  try {
    await start();
    return { kind: "recording" };
  } catch (e) {
    // No input device, permission denied, device busy — all reachable
    // from `recorder`, and all previously silent.
    return { kind: "failed", message: `Could not start recording: ${String(e)}` };
  }
}

export async function stopTake(
  stop: () => Promise<{ path: string }>,
  load: (paths: string[]) => Promise<{ last_node_id: string | null }>,
): Promise<StopOutcome> {
  let saved: { path: string };
  try {
    saved = await stop();
  } catch (e) {
    return {
      kind: "saveFailed",
      message: `Recording stopped but could not be saved — the take was lost: ${String(e)}`,
    };
  }

  try {
    const { last_node_id } = await load([saved.path]);
    return { kind: "loaded", path: saved.path, nodeId: last_node_id };
  } catch (e) {
    return {
      kind: "loadFailed",
      path: saved.path,
      message:
        `Recording saved to ${saved.path}, but could not be added to the ` +
        `session: ${String(e)}`,
    };
  }
}

/**
 * What happened to a scheduled take (#225 §4).
 *
 * `timer_record` resolves when the take is saved, having already
 * written the WAV — so unlike `stopTake` there is no window where the
 * audio exists but the result does not. What it can still do is come
 * back cancelled, which is an outcome rather than a failure: the user
 * pressed Cancel, and no take was meant to exist.
 */
export type TimerOutcome =
  | { kind: "loaded"; path: string; nodeId: string | null }
  | { kind: "cancelled" }
  | { kind: "failed"; message: string }
  | { kind: "loadFailed"; path: string; message: string };

/**
 * Arm an unattended take and report what became of it.
 *
 * Same split as `stopTake`: a take that was written but could not be
 * imported is **on disk**, and naming the path is far more useful than
 * calling it lost. Only `failed` means there is no audio.
 */
export async function scheduledTake(
  record: () => Promise<{ path?: string; cancelled: boolean }>,
  load: (paths: string[]) => Promise<{ last_node_id: string | null }>,
): Promise<TimerOutcome> {
  let result: { path?: string; cancelled: boolean };
  try {
    result = await record();
  } catch (e) {
    // No input device, a device that went away during the countdown,
    // already recording, or a schedule with neither half set.
    return { kind: "failed", message: `Scheduled recording failed: ${String(e)}` };
  }

  if (result.cancelled) return { kind: "cancelled" };
  if (!result.path) {
    return {
      kind: "failed",
      message: "Scheduled recording finished but reported no file.",
    };
  }

  try {
    const { last_node_id } = await load([result.path]);
    return { kind: "loaded", path: result.path, nodeId: last_node_id };
  } catch (e) {
    return {
      kind: "loadFailed",
      path: result.path,
      message:
        `Recording saved to ${result.path}, but could not be added to the ` +
        `session: ${String(e)}`,
    };
  }
}

/**
 * The countdown label for a `timer_record` progress report.
 *
 * The two phases read differently on purpose. Before it starts, the
 * only question is when; once it is capturing, the only question is how
 * much longer — and "recording" has to be unmistakable, because the
 * whole feature exists for takes nobody is watching.
 *
 * A schedule with only a start delay has no end to count down to, so
 * `remaining_sec` is absent once recording and the label says so
 * rather than showing a blank.
 */
export function timerLabel(p: {
  recording?: boolean;
  remaining_sec?: number;
}): string {
  const left = p.remaining_sec;
  if (!p.recording) {
    return left === undefined
      ? "Waiting to record…"
      : `Recording starts in ${formatSeconds(left)}`;
  }
  return left === undefined
    ? "Recording… (until you stop it)"
    : `Recording — ${formatSeconds(left)} left`;
}

/**
 * Seconds as something a person reads at a glance.
 *
 * Whole seconds below a minute; `m:ss` above. Rounded up, so a timer
 * never shows "0s" while it is still going.
 */
export function formatSeconds(sec: number): string {
  const s = Math.max(0, Math.ceil(sec));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  return `${m}:${String(s % 60).padStart(2, "0")}`;
}
