import { nodeId, projectWith, readyToLoad, selecting, toneTrack } from "./backend";
import { expect, test } from "./fixtures";
import { dragSelect } from "./timeline";

test("template dialog traps focus, makes the background inert, and suppresses App shortcuts", async ({ app }) => {
  await app.boot(readyToLoad());
  const page = app.page;
  const trigger = page.getByRole("button", { name: "Start from template", exact: true });
  await trigger.click();
  const dialog = page.getByRole("dialog", { name: "Start from template" });
  await expect(dialog).toHaveAttribute("aria-modal", "true");
  const cancel = dialog.getByRole("button", { name: "Cancel" });
  await expect(cancel).toBeFocused();
  await expect.poll(() => trigger.evaluate(el => !!el.closest("[inert]"))).toBe(true);
  await page.keyboard.press("Tab");
  await expect(dialog.getByRole("button").first()).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(cancel).toBeFocused();
  await page.keyboard.press("Control+k");
  await page.keyboard.press("?");
  await expect(page.getByRole("dialog")).toHaveCount(1);
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(trigger).toBeFocused();
  await expect.poll(() => trigger.evaluate(el => !!el.closest("[inert]"))).toBe(false);
});

test("palette and shortcuts enter and restore focus", async ({ app }) => {
  await app.boot(readyToLoad());
  const page = app.page;
  const trigger = page.getByRole("button", { name: "Start from template", exact: true });
  await trigger.focus();
  await page.keyboard.press("Control+k");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await expect(palette.getByRole("textbox", { name: "Search commands" })).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(palette.getByRole("button").last()).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(palette.getByRole("textbox")).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(trigger).toBeFocused();
  await page.keyboard.press("?");
  const shortcuts = page.getByRole("dialog", { name: "Keyboard Shortcuts" });
  await expect(shortcuts.getByRole("button", { name: "Close" })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(shortcuts.getByRole("button", { name: "Close" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(trigger).toBeFocused();
});

for (const [testId, name, field] of [
  ["punch-btn", "Punch in", "punch-track"],
  ["timer-record-btn", "Scheduled recording", "timer-start-min"],
]) {
  test(`${name} traps focus and Escape preserves the timeline selection`, async ({ app }) => {
    await app.boot({ ...projectWith([toneTrack()], nodeId(1)), ...selecting() });
    const page = app.page;
    await expect(page.getByTestId("ruler")).toContainText("0:03");
    await dragSelect(app, 0.2, 0.5);
    const trigger = page.getByTestId(testId);
    await trigger.click();
    const dialog = page.getByRole("dialog", { name, exact: true });
    await expect(dialog.getByTestId(field)).toBeFocused();
    await expect(dialog).toHaveAttribute("aria-modal", "true");
    // Disabled actions are skipped; every tab remains inside the card.
    for (let i = 0; i < 7; i++) {
      await page.keyboard.press("Tab");
      await expect.poll(() => dialog.evaluate(el => el.contains(document.activeElement))).toBe(true);
    }
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
    await expect(trigger).toBeFocused();
    await expect(page.getByTestId("punch-btn")).toBeEnabled();
  });
}
