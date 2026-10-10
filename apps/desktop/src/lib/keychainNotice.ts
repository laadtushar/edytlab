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
