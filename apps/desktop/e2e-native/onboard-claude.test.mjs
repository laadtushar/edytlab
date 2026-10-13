// How onboardClaude gets past the welcome, against a stand-in dialog (no
// display, driver, app or model needed):
//
//   node --test apps/desktop/e2e-native/onboard-claude.test.mjs
//
// run-env.sh points the session bus at nothing, so the harness has no
// Secret Service and every save lands in the kernel keyring. Since #539 the
// welcome then stays up with a note on that and a Continue button, and
// closes only when Continue is pressed. onboardClaude used to press Save
// and wait for the welcome to close, which it never did: every Claude story
// and demo timed out on "the welcome to close".
import { beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { Driver } from "./webdriver.mjs";
import { onboardClaude } from "./stories/claude.mjs";

const id = (css) => /data-testid='([^']+)'/.exec(css)?.[1];

/** The welcome: what it shows, and what a save does. */
let page;
beforeEach(() => {
  page = { open: true, footer: null, saveError: "", clicked: [], typed: {} };
});

/** The real Driver's polling (`until`), over the stand-in dialog. `onSave`
 * is what pressing Save does to it. */
function fakeCtx(onSave) {
  const d = Object.create(Driver.prototype);
  d.waitFor = async () => "settings";
  d.type = async (css, text) => {
    page.typed[id(css)] = text;
  };
  d.click = async (css) => {
    page.clicked.push(id(css));
    if (id(css) === "settings-save-button") setTimeout(() => onSave(page), 300);
    if (id(css) === "settings-continue-button") page.open = false;
  };
  d.count = async (css) => {
    const t = id(css);
    if (t === "settings") return page.open ? 1 : 0;
    if (t === "settings-continue-button" || t === "settings-save-error") return page.open && page.footer === t ? 1 : 0;
    return 0;
  };
  d.text = async (css) => {
    if (id(css) === "settings-save-error" && page.footer === "settings-save-error") return page.saveError;
    throw new Error(`no such element: ${css}`);
  };
  return { d };
}

test("a save that fell back to the kernel keyring: Continue is pressed and the welcome closes", async () => {
  const ctx = fakeCtx((p) => (p.footer = "settings-continue-button"));
  await onboardClaude(ctx, { key: "test-key" });
  assert.deepEqual(page.clicked, ["settings-provider-anthropic", "settings-save-button", "settings-continue-button"]);
  assert.equal(page.open, false);
  assert.equal(page.typed["settings-key-input"], "test-key");
});

test("a durable save closes the welcome by itself: nothing more is pressed", async () => {
  const ctx = fakeCtx((p) => (p.open = false));
  await onboardClaude(ctx, { key: "test-key" });
  assert.deepEqual(page.clicked, ["settings-provider-anthropic", "settings-save-button"]);
});

test("a failed save throws with the dialog's message rather than waiting out the welcome", async () => {
  const ctx = fakeCtx((p) => {
    p.footer = "settings-save-error";
    p.saveError = "keychain refused the write";
  });
  await assert.rejects(onboardClaude(ctx, { key: "test-key" }), { message: "the save failed: keychain refused the write" });
});
