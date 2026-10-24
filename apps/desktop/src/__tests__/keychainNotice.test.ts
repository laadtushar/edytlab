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
  chatGptStorageNotice,
  keychainNotice,
  readSessionOnlyDismissed,
  savedNotice,
  writeSessionOnlyDismissed,
} from "../lib/keychainNotice";

describe("keychainNotice", () => {
  it("says nothing when the keychain reads and persists", () => {
    expect(keychainNotice(null, null)).toBeNull();
    expect(keychainNotice(null, { persistent: true, reason: null })).toBeNull();
  });

  it("reports an unreadable keychain with the reason, and does not call it a missing key", () => {
    const n = keychainNotice("Couldn't access platform secure storage: locked", null);
    expect(n?.kind).toBe("read-error");
    expect(n?.message).toContain("(Couldn't access platform secure storage: locked)");
    expect(n?.message).toContain("can't tell whether an API key is stored");
    expect(n?.message).toContain("Settings");
  });

  it("says the keychain could not be read once: the backend sends the reason alone", () => {
    // `has_api_key` rejects with `KeychainError::reason()`. The full
    // message ("the system keychain could not be read: …") would make the
    // banner open with the same words its parenthesis then repeats.
    const reason = "dismissed the unlock prompt";
    const n = keychainNotice(reason, null);
    expect(n?.message.split(reason)).toHaveLength(2);
    expect(n?.message).not.toContain("could not be read");
    expect(n?.message.match(/system keychain/g)).toHaveLength(1);
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

describe("savedNotice", () => {
  const durable = { persistent: true, reason: null };
  const fellBack = (reason: string | null) => ({ persistent: false, reason });

  it("says nothing when every write went to the durable store", () => {
    expect(savedNotice([])).toBeNull();
    expect(savedNotice([durable, durable])).toBeNull();
  });

  it("treats an answer that is missing as durable: there is nothing to report", () => {
    expect(savedNotice([undefined, null, durable])).toBeNull();
  });

  it("says it in the launch banner's words when any write fell back", () => {
    const banner = keychainNotice(null, fellBack("no bus"));
    expect(banner?.kind).toBe("session-only");
    for (const landings of [
      [fellBack("no bus")],
      [durable, fellBack("no bus")],
      [fellBack("no bus"), durable, undefined],
    ]) {
      expect(savedNotice(landings), JSON.stringify(landings)).toEqual(banner);
    }
  });

  it("gives the reason of the first write that fell back", () => {
    const n = savedNotice([durable, fellBack("first"), fellBack("second")]);
    expect(n?.message).toContain("(first)");
    expect(n?.message).not.toContain("second");
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

describe("chatGptStorageNotice", () => {
  it("says nothing for a token in the keychain, or no token", () => {
    expect(chatGptStorageNotice(null)).toBeNull();
    expect(chatGptStorageNotice({ kind: "keychain", persistent: true, reason: null })).toBeNull();
  });

  it("uses the launch banner's words for the kernel keyring", () => {
    const n = chatGptStorageNotice({ kind: "kernel_keyring", persistent: false, reason: "X" });
    expect(n).toEqual(keychainNotice(null, { persistent: false, reason: "X" }));
  });

  it("says a token kept in memory lasts until edytlab quits", () => {
    const n = chatGptStorageNotice({ kind: "memory", persistent: false, reason: "too long" });
    expect(n?.kind).toBe("session-only");
    expect(n?.message).toContain("until you quit edytlab");
    expect(n?.message).toContain("(too long)");
  });
});
