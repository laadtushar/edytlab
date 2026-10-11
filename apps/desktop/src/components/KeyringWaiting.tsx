/**
 * KeyringWaiting — says the app is waiting on the system keyring (#497).
 *
 * The backend reads the keychain at launch off the setup thread, so the
 * window is up while a locked keyring's unlock prompt waits on the user.
 * `has_api_key` is pending until that is over, and `pending` is that: the
 * app does not yet know whether a key is stored, or whether it can find
 * out.
 *
 * Shown only once `pending` has held for `delayMs`. Most launches answer
 * within a few milliseconds, and a line that flashed on each of them
 * would be noise. It is a status row rather than an alert: nothing is
 * wrong yet, and nothing here can be fixed from the app.
 */

import { useEffect, useState } from "react";

import {
  KEYRING_WAITING_MESSAGE,
  KEYRING_WAIT_NOTICE_DELAY_MS,
} from "../lib/keychainNotice";

interface KeyringWaitingProps {
  /** Whether the keychain read is still pending. */
  pending: boolean;
  /** How long it must have been pending before this appears. */
  delayMs?: number;
}

export function KeyringWaiting({
  pending,
  delayMs = KEYRING_WAIT_NOTICE_DELAY_MS,
}: KeyringWaitingProps) {
  const [late, setLate] = useState(false);

  useEffect(() => {
    if (!pending) {
      setLate(false);
      return;
    }
    const timer = window.setTimeout(() => setLate(true), delayMs);
    return () => window.clearTimeout(timer);
  }, [pending, delayMs]);

  if (!pending || !late) return null;

  return (
    <div
      role="status"
      data-testid="keychain-waiting"
      className="
        banner-in
        flex items-center gap-3
        border-b border-[var(--border-strong)]
        bg-[var(--accent-soft)]
        px-4 py-2.5
        text-sm
      "
    >
      <span
        aria-hidden="true"
        className="h-2 w-2 shrink-0 rounded-full bg-[var(--accent)] motion-safe:animate-pulse"
      />
      <span className="flex-1 text-[var(--text)]">{KEYRING_WAITING_MESSAGE}</span>
    </div>
  );
}
