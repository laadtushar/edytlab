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

import { deferred, firstRun, ok, readyToLoad, reject } from "./backend";
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

test.describe("a keychain that is still being read", () => {
  /**
   * #497. The backend reads the keychain off the setup thread, so the
   * window is up while a locked keyring's unlock prompt waits on the user
   * and `has_api_key` is pending. The app has to say what it is waiting
   * for, and must not guess: it is neither the first-run prompt (a key
   * may be stored) nor an error (nothing has failed yet).
   */
  test("says it is waiting for the keyring, and carries on when it answers", async ({
    app,
  }) => {
    // `has_api_key` waits on `AppState::startup_reads_settled` first.
    await app.boot({ ...firstRun(), has_api_key: deferred("keyring") });
    const page = app.page;

    const waiting = page.getByTestId("keychain-waiting");
    await expect(waiting).toBeVisible();
    await expect(waiting).toContainText("Waiting for your keyring");
    await expect(page.getByRole("heading", { name: "Welcome to edytlab" })).toHaveCount(0);
    await expect(page.getByTestId("keychain-error")).toHaveCount(0);

    // The keyring answered: no key stored.
    await app.release("keyring", false);
    await expect(waiting).toHaveCount(0);
    await expect(page.getByRole("heading", { name: "Welcome to edytlab" })).toBeVisible();
  });

  test("says nothing at all when the keychain answers at once", async ({ app }) => {
    await app.boot(readyToLoad());
    await app.settle();

    await expect(app.page.getByTestId("keychain-waiting")).toHaveCount(0);
  });
});

test.describe("Settings after a startup the keychain refused", () => {
  /**
   * #497. The base URL, the reasoning level and the model read as "nothing
   * stored", and Save sent a blank URL, no level and the model the field
   * happened to show, which delete or overwrite the stored ones. The user
   * who dismissed the startup prompts and accepted the one Save raises
   * lost all three. Here every read is refused the way the backend refuses
   * it (opening Settings asks the keychain for none of them again), and
   * Save must write the key and nothing else.
   */
  test("saves the key and leaves the stored base URL, level and model alone", async ({
    app,
  }) => {
    const refused = reject("Couldn't access platform secure storage: locked");
    await app.boot({
      ...firstRun(),
      has_api_key: refused,
      // `active_provider_answer`: the default is not an answer while
      // startup's read is unaccounted for.
      get_active_provider: refused,
      // `stored_setting_unless_refused(..)` for the base URL and the level,
      // `active_model_answer(..)` for the model, and `catalogue_inputs`
      // for the model list: all rejected, none read.
      get_base_url_for: refused,
      get_effort_for: refused,
      get_active_model: refused,
      list_models_for: refused,
      // `CmdResult<()>` serialises as `null`.
      set_api_key_for: ok(null),
      set_active_model: ok(null),
    });
    const page = app.page;

    await page
      .getByTestId("keychain-error")
      .getByRole("button", { name: "Open Settings" })
      .click();
    const settings = page.getByRole("dialog", { name: "Settings" });
    await expect(settings).toHaveAttribute("data-mode", "panel");
    await expect(settings.getByTestId("settings-base-url-unread")).toContainText("locked");
    await expect(settings.getByTestId("settings-effort-unread")).toContainText("locked");
    await expect(settings.getByTestId("settings-model-unread")).toContainText("locked");
    await expect(settings.getByTestId("settings-model-error")).toContainText("locked");

    await settings.getByTestId("settings-key-input").fill("sk-ant-test");
    await settings.getByTestId("settings-save-button").click();

    // The panel closes on a save that went through.
    await expect(settings).toHaveCount(0);
    expect(await app.requestsFor("set_api_key_for")).toEqual([
      { provider: "anthropic", key: "sk-ant-test" },
    ]);
    expect(await app.requestsFor("set_base_url_for")).toEqual([]);
    expect(await app.requestsFor("set_effort_for")).toEqual([]);
    expect(await app.requestsFor("set_active_model")).toEqual([]);
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
