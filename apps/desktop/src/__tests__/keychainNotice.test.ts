/**
 * What the app tells the user about the system keychain (#394).
 *
 * The words live in one pure function so the banner, the Settings note
 * and these tests cannot drift apart, and so the precedence between the
 * two situations is checked without rendering anything.
 */

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  SESSION_ONLY_DISMISSED_KEY,
  keychainNotice,
  readSessionOnlyDismissed,
  writeSessionOnlyDismissed,
} from "../lib/keychainNotice";

describe("keychainNotice", () => {
  it("says nothing when the keychain reads and persists", () => {
    expect(keychainNotice(null, null)).toBeNull();
    expect(keychainNotice(null, { persistent: true, reason: null })).toBeNull();
  });

  it("reports an unreadable keychain with the reason, and does not call it a missing key", () => {
    const n = keychainNotice("the system keychain could not be read: locked", null);
    expect(n?.kind).toBe("read-error");
    expect(n?.message).toContain("locked");
    expect(n?.message).toContain("can't tell whether an API key is stored");
    expect(n?.message).toContain("Settings");
  });

  it("warns that settings last only until restart when there is no Secret Service", () => {
    const n = keychainNotice(null, { persistent: false, reason: "X" });
    expect(n?.kind).toBe("session-only");
    expect(n?.message).toContain("Secret Service");
    expect(n?.message).toContain("restart");
    expect(n?.message).toContain("(X)");
  });

  it("leaves the reason off when there is none to give", () => {
    const n = keychainNotice(null, { persistent: false, reason: null });
    expect(n?.kind).toBe("session-only");
    expect(n?.message.endsWith("until you restart your computer.")).toBe(true);
  });

  it("puts a read error ahead of the persistence warning", () => {
    const n = keychainNotice("locked", { persistent: false, reason: "X" });
    expect(n?.kind).toBe("read-error");
  });
});

describe("the dismissed flag", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    window.localStorage.clear();
  });

  it("round-trips through localStorage", () => {
    window.localStorage.clear();
    expect(readSessionOnlyDismissed()).toBe(false);

    writeSessionOnlyDismissed(true);
    expect(window.localStorage.getItem(SESSION_ONLY_DISMISSED_KEY)).toBe("1");
    expect(readSessionOnlyDismissed()).toBe(true);

    writeSessionOnlyDismissed(false);
    expect(window.localStorage.getItem(SESSION_ONLY_DISMISSED_KEY)).toBeNull();
    expect(readSessionOnlyDismissed()).toBe(false);
  });

  it("reads as not dismissed, and writes without throwing, when storage is blocked", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    vi.spyOn(Storage.prototype, "removeItem").mockImplementation(() => {
      throw new Error("blocked");
    });

    expect(readSessionOnlyDismissed()).toBe(false);
    expect(() => writeSessionOnlyDismissed(true)).not.toThrow();
    expect(() => writeSessionOnlyDismissed(false)).not.toThrow();
  });
});
