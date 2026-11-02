import { nodeId, ok, projectWith, readyToLoad, recordingLatencyStatus, selecting, toneTrack } from "./backend";
import { expect, test } from "./fixtures";

test("template dialog traps focus, makes the background inert, and restores its opener", async ({ app }) => {
  await app.boot({ ...readyToLoad(), list_templates: ok([{ name: "Podcast", description: "Podcast template" }]) });
  const page = app.page;
  const opener = page.getByTestId("empty-state").getByRole("button", { name: "Start from template" });
  await opener.click();
  const dialog = page.getByRole("dialog", { name: "Start from template" });
  await expect(dialog).toHaveAttribute("aria-modal", "true");
  const cancel = dialog.getByRole("button", { name: "Cancel" });
  await expect(cancel).toBeFocused();
  expect(await opener.evaluate((node) => !!node.closest("[inert]"))).toBe(true);
  await page.keyboard.press("Tab");
  await expect(dialog.getByRole("button", { name: /Podcast/ })).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(cancel).toBeFocused();
  await page.keyboard.press("Control+k");
  await expect(page.getByRole("dialog", { name: "Command palette" })).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(opener).toBeFocused();
  expect(await opener.evaluate((node) => !!node.closest("[inert]"))).toBe(false);
});

test("shortcuts and timer dialogs suspend App shortcuts and release focus on Escape", async ({ app }) => {
  await app.boot(projectWith([toneTrack()], nodeId(1)));
  const page = app.page;
  await expect(page.getByTestId("ruler")).toBeVisible();
  await page.keyboard.press("?");
  const shortcuts = page.getByRole("dialog", { name: "Keyboard Shortcuts" });
  await expect(shortcuts.getByRole("button", { name: "Close" })).toBeFocused();
  await page.keyboard.press("Control+k");
  await expect(page.getByRole("dialog", { name: "Command palette" })).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(shortcuts).toHaveCount(0);
  const opener = page.getByRole("button", { name: "Scheduled recording" });
  await opener.click();
  const timer = page.getByRole("dialog", { name: "Scheduled recording" });
  await expect(timer.getByRole("spinbutton", { name: "Start after minutes" })).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(timer.getByRole("spinbutton", { name: "Record for seconds" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(opener).toBeFocused();
  await page.keyboard.press("Control+k");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await expect(palette.getByRole("textbox", { name: "Search commands" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(opener).toBeFocused();
});


test("Escape closes Punch without clearing the region underneath", async ({ app }) => {
  const head = nodeId(1);
  await app.boot({
    ...projectWith([toneTrack()], head),
    ...selecting(),
    get_view_state: ok({ head, selection: [0.2, 0.8] }),
    get_recording_latency: ok(recordingLatencyStatus()),
  });
  const page = app.page;
  const selection = page.getByTestId("timeline-selection-overlay");
  await expect(selection).toBeVisible();
  const opener = page.getByRole("button", { name: "Punch in" });
  await opener.click();
  const punch = page.getByRole("dialog", { name: "Punch in" });
  await expect(punch).toHaveAttribute("aria-modal", "true");
  await expect(punch.getByRole("combobox")).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(punch).toHaveCount(0);
  await expect(opener).toBeFocused();
  await expect(selection).toBeVisible();
});
