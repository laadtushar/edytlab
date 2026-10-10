/**
 * What to tell the user about the system keychain, and the one flag that
 * remembers they were told (#394).
 *
 * Two different things can be wrong, and they need different words:
 *
 * - the keychain could not be *read* at launch (a locked keyring, a
 *   dismissed unlock prompt). A key may well be stored; edytlab cannot
 *   tell, so it must not show the first-run prompt as if there were none.
 * - the keychain works but is *not persistent*: on Linux with no Secret
 *   Service running, settings are kept in the kernel keyring and are gone
 *   after the next restart of the machine.
 *
 * All the copy is here so the banner, the Settings note and the tests say
 * the same thing.
 */
import type { KeychainPersistence } from "./tauri-bridge";

export interface KeychainNotice {
  kind: "read-error" | "session-only";
  message: string;
}

/**
 * The notice to show, or null when there is nothing to say.
 *
 * `readError` wins: if the keychain cannot be read, whether it would
 * have persisted is beside the point.
 */
export function keychainNotice(
  readError: string | null,
  persistence: KeychainPersistence | null,
): KeychainNotice | null {
  if (readError) {
    return {
      kind: "read-error",
      message:
        "Couldn't read your saved settings from the system keychain, so edytlab can't tell whether an API key is stored. " +
        "Unlock your keyring and restart edytlab, or enter your key again in Settings. " +
        `(${readError})`,
    };
  }
  if (persistence && !persistence.persistent) {
    return {
      kind: "session-only",
      message:
        "No Secret Service (GNOME Keyring, KWallet or KeePassXC) is running, so your API key and provider are kept only until you restart your computer." +
        (persistence.reason ? ` (${persistence.reason})` : ""),
    };
  }
  return null;
}

/**
 * What the app says while the backend is still reading the keychain at
 * launch (#497).
 *
 * The reads run off the setup thread so the window can come up, and a
 * locked Secret Service's unlock prompt has no time limit, so they can
 * take as long as the user does. `has_api_key` is pending until they are
 * done; this is what the window says meanwhile, so a prompt the user has
 * not noticed is something they are told to look for.
 */
export const KEYRING_WAITING_MESSAGE =
  "Waiting for your keyring. If it is asking you to unlock it, answer or dismiss the prompt; edytlab carries on when it's done.";

/**
 * How long `has_api_key` must have been pending before the app says it is
 * waiting. A keychain that answers at once is no news, and a line that
 * flashed on every launch would be noise.
 */
export const KEYRING_WAIT_NOTICE_DELAY_MS = 600;

/**
 * Where the dismissal of the session-only warning is remembered. It is
 * a per-viewer convenience, so it lives in localStorage, and the page
 * must work without it.
 */
export const SESSION_ONLY_DISMISSED_KEY = "edytlab.keychainSessionOnlyDismissed";

/** Whether the user already dismissed the session-only warning. */
export function readSessionOnlyDismissed(): boolean {
  try {
    return window.localStorage.getItem(SESSION_ONLY_DISMISSED_KEY) === "1";
  } catch {
    return false;
  }
}

/** Remember (or forget) that the session-only warning was dismissed. */
export function writeSessionOnlyDismissed(dismissed: boolean): void {
  try {
    if (dismissed) {
      window.localStorage.setItem(SESSION_ONLY_DISMISSED_KEY, "1");
    } else {
      window.localStorage.removeItem(SESSION_ONLY_DISMISSED_KEY);
    }
  } catch {
    // Storage can be blocked or full; the warning just shows again.
  }
}
