/**
 * "Waiting for your keyring" (#497).
 *
 * The backend reads the keychain off the setup thread, so the window is up
 * while a locked keyring's unlock prompt waits on the user, and
 * `has_api_key` is pending meanwhile. The row that says so must not flash
 * on every launch (most answer in milliseconds), must appear when the
 * wait is real, and must be gone the moment it ends.
 *
 * Real timers with a short delay rather than fake ones: the suite also
 * runs with React's scheduler 40 ms late, and a test that advanced a fake
 * clock would have to know about that.
 */

import { act, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { KeyringWaiting } from "../components/KeyringWaiting";
import {
  KEYRING_WAITING_MESSAGE,
  KEYRING_WAIT_NOTICE_DELAY_MS,
} from "../lib/keychainNotice";

const DELAY = 80;

/** Let `ms` pass, inside `act` so React draws whatever the wait caused. */
const pass = (ms: number) =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, ms));
  });

describe("KeyringWaiting", () => {
  it("says nothing at first, and says it is waiting once the wait is real", async () => {
    render(<KeyringWaiting pending delayMs={DELAY} />);
    expect(screen.queryByTestId("keychain-waiting")).toBeNull();

    const row = await screen.findByTestId("keychain-waiting");
    expect(row).toHaveTextContent(KEYRING_WAITING_MESSAGE);
    expect(row).toHaveAttribute("role", "status");
  });

  /** The ordinary launch: the keychain answers in a few milliseconds. */
  it("never appears for a read that finishes before the delay", async () => {
    const { rerender } = render(<KeyringWaiting pending delayMs={DELAY} />);
    rerender(<KeyringWaiting pending={false} delayMs={DELAY} />);

    await pass(DELAY * 2);
    expect(screen.queryByTestId("keychain-waiting")).toBeNull();
  });

  it("goes away when the wait ends", async () => {
    const { rerender } = render(<KeyringWaiting pending delayMs={DELAY} />);
    await screen.findByTestId("keychain-waiting");

    rerender(<KeyringWaiting pending={false} delayMs={DELAY} />);
    expect(screen.queryByTestId("keychain-waiting")).toBeNull();
    // And it does not come back on its own.
    await pass(DELAY * 2);
    expect(screen.queryByTestId("keychain-waiting")).toBeNull();
  });

  it("says nothing when nothing is pending", async () => {
    render(<KeyringWaiting pending={false} delayMs={DELAY} />);
    await pass(DELAY * 2);
    expect(screen.queryByTestId("keychain-waiting")).toBeNull();
  });

  it("tells the user what to look for", async () => {
    render(<KeyringWaiting pending delayMs={DELAY} />);
    await waitFor(() =>
      expect(screen.getByTestId("keychain-waiting")).toHaveTextContent(
        /unlock prompt|asking you to unlock/i,
      ),
    );
  });

  /** The default is what App uses; a launch that is merely slow stays quiet. */
  it("waits a beat by default", () => {
    expect(KEYRING_WAIT_NOTICE_DELAY_MS).toBeGreaterThanOrEqual(300);
    render(<KeyringWaiting pending />);
    expect(screen.queryByTestId("keychain-waiting")).toBeNull();
  });
});
