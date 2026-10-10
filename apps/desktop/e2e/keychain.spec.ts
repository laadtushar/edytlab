/**
 * What the app says about the system keychain (#394).
 *
 * On Linux the kernel keyring that keys used to live in is wiped by a
 * reboot, so the first launch afterwards looked like a fresh install and
 * the user was asked for a key they had already entered. Two things fix
 * that, and both are about what is on screen:
 *
 * - a keychain that could not be *read* (a locked keyring) is reported,
 *   not shown as the first-run Welcome;
 * - a keychain that works but will not *persist* (no Secret Service)
 *   says so once, so the Welcome after the next reboot is no surprise.
 *
 * The backend is faked at the IPC boundary, so what these pin is the
 * frontend half: what it does with a rejection, and with `persistent:
 * false`. The Rust half is in `commands.rs` and `keychain.rs`.
 */

import { firstRun, ok, readyToLoad, reject } from "./backend";
import { expect, test } from "./fixtures";

const DISMISSED_KEY = "edytlab.keychainSessionOnlyDismissed";

test.describe("a keychain that cannot be read", () => {
  test("says so at launch instead of asking for a key", async ({ app }) => {
    await app.boot({
      ...firstRun(),
      // What `has_api_key` rejects with: `KeychainError::reason()`, the
      // keyring library's words without the "could not be read" prefix.
      has_api_key: reject("Couldn't access platform secure storage: locked"),
    });
    const page = app.page;

    const banner = page.getByTestId("keychain-error");
    await expect(banner).toBeVisible();
    await expect(banner).toContainText("locked");
    await expect(banner).toContainText("Unlock your keyring");
    // Said once: the banner's own words, then the reason in brackets.
    await expect(banner).not.toContainText("could not be read");

    // The point of the fix: a stored key was never looked at, so the
    // first-run prompt would be about something that is not true.
    await expect(
      page.getByRole("heading", { name: "Welcome to edytlab" }),
    ).toHaveCount(0);

    // The way forward is one click.
    await banner.getByRole("button", { name: "Open Settings" }).click();
    const settings = page.getByRole("dialog", { name: "Settings" });
    await expect(settings).toBeVisible();
    await expect(settings).toHaveAttribute("data-mode", "panel");
  });
});

test.describe("a keychain that will not survive a reboot", () => {
  test("warns once that settings last until restart, and remembers the dismissal", async ({
    app,
  }) => {
    await app.boot({
      ...readyToLoad(),
      get_keychain_persistence: ok({
        persistent: false,
        reason: "org.freedesktop.DBus.Error.ServiceUnknown",
      }),
    });
    const page = app.page;

    const banner = page.getByTestId("keychain-session-only");
    await expect(banner).toBeVisible();
    await expect(banner).toContainText("restart");
    await expect(banner).toContainText("ServiceUnknown");

    await banner.getByRole("button", { name: "Dismiss warning" }).click();
    await expect(banner).toHaveCount(0);
    expect(
      await page.evaluate((k) => localStorage.getItem(k), DISMISSED_KEY),
    ).toBe("1");
  });

  test("a dismissed warning stays dismissed on the next launch", async ({
    app,
  }) => {
    await app.boot({
      ...readyToLoad(),
      get_keychain_persistence: ok({ persistent: false, reason: null }),
    });
    const page = app.page;
    await expect(page.getByTestId("keychain-session-only")).toBeVisible();
    await page
      .getByTestId("keychain-session-only")
      .getByRole("button", { name: "Dismiss warning" })
      .click();

    await page.reload();
    // Gone before the backend has said anything new: let the app
    // settle with the same answers, then look.
    await app.settle();
    await expect(page.getByTestId("keychain-session-only")).toHaveCount(0);
  });

  test("the warning reaches the onboarding dialog, which covers the banner", async ({
    app,
  }) => {
    await app.boot({
      ...firstRun(),
      get_keychain_persistence: ok({ persistent: false, reason: null }),
    });

    const onboarding = app.page.getByRole("dialog", { name: "Settings" });
    await expect(onboarding).toHaveAttribute("data-mode", "blocking");
    await expect(onboarding.getByTestId("settings-storage-warning")).toContainText(
      "until you restart your computer",
    );
  });
});

test.describe("a keychain that works", () => {
  test("shows neither notice", async ({ app }) => {
    await app.boot(readyToLoad());
    await app.settle();

    await expect(app.page.getByTestId("keychain-error")).toHaveCount(0);
    await expect(app.page.getByTestId("keychain-session-only")).toHaveCount(0);
    await expect(app.page.getByTestId("settings-storage-warning")).toHaveCount(0);
  });
});
