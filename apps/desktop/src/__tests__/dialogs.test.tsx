import { StrictMode, useState } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { TemplatePickerModal } from "../components/TemplatePickerModal";
import { useDialog } from "../hooks/useDialog";
import { ShortcutsOverlay } from "../components/ShortcutsOverlay";

function Stack() {
  const [templates, setTemplates] = useState(false);
  const [shortcuts, setShortcuts] = useState(false);
  return <>
    <button onClick={() => setTemplates(true)}>Open templates</button>
    <TemplatePickerModal open={templates} templates={[{ name: "Podcast", description: "Two speakers" }]}
      onSelect={() => setShortcuts(true)} onClose={() => setTemplates(false)} />
    <ShortcutsOverlay open={shortcuts} onClose={() => setShortcuts(false)} />
  </>;
}

describe("shared dialogs", () => {
  it("names the template dialog, traps focus, makes the background inert and restores focus", async () => {
    const user = userEvent.setup();
    render(<StrictMode><Stack /></StrictMode>);
    const trigger = screen.getByRole("button", { name: "Open templates" });
    await user.click(trigger);
    const dialog = await screen.findByRole("dialog", { name: "Start from template" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(trigger.closest("[inert]")).not.toBeNull();
    const cancel = screen.getByRole("button", { name: "Cancel" });
    expect(cancel).toHaveFocus();
    await user.tab();
    expect(screen.getByRole("button", { name: /Podcast/ })).toHaveFocus();
    await user.tab({ shift: true });
    expect(cancel).toHaveFocus();
    trigger.focus();
    expect(dialog.contains(document.activeElement)).toBe(true);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(trigger).toHaveFocus());
    expect(trigger.closest("[inert]")).toBeNull();
    // The animation tail must no longer expose a focusable dialog.
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("closes only the top dialog and restores focus through the stack", async () => {
    const user = userEvent.setup();
    render(<Stack />);
    await user.click(screen.getByRole("button", { name: "Open templates" }));
    const podcast = screen.getByRole("button", { name: /Podcast/ });
    await user.click(podcast);
    await screen.findByRole("dialog", { name: "Keyboard Shortcuts" });
    await user.keyboard("{Escape}");
    await waitFor(() => expect(podcast).toHaveFocus());
    expect(screen.getByRole("dialog", { name: "Start from template" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("cleans up inertness on unmount without removing pre-existing inert attributes", async () => {
    const existing = document.createElement("div");
    existing.setAttribute("inert", "");
    document.body.append(existing);
    const { unmount } = render(<Stack />);
    fireEvent.click(screen.getByRole("button", { name: "Open templates" }));
    await screen.findByRole("dialog", { name: "Start from template" });
    unmount();
    expect(existing).toHaveAttribute("inert");
    expect(document.querySelectorAll("[inert]")).toHaveLength(1);
    existing.remove();
  });
});

function EmptyDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { dialogProps, headingId } = useDialog(open, onClose);
  return open ? <div {...dialogProps}>
    <h2 id={headingId}>Empty dialog</h2>
    <button disabled>Disabled</button>
    <button hidden>Hidden</button>
  </div> : null;
}

it("focuses an empty dialog, skips hidden and disabled controls, and inerts newly mounted background", async () => {
  const user = userEvent.setup();
  const close = vi.fn();
  const { unmount } = render(<StrictMode><EmptyDialog open onClose={close} /></StrictMode>);
  const dialog = screen.getByRole("dialog", { name: "Empty dialog" });
  expect(dialog).toHaveFocus();
  await user.tab();
  expect(dialog).toHaveFocus();
  await user.tab({ shift: true });
  expect(dialog).toHaveFocus();
  const portal = document.createElement("button");
  document.body.append(portal);
  await waitFor(() => expect(portal).toHaveAttribute("inert"));
  fireEvent.keyDown(window, { key: "Escape", repeat: true });
  expect(close).not.toHaveBeenCalled();
  await user.keyboard("{Escape}");
  expect(close).toHaveBeenCalledOnce();
  unmount();
  expect(portal).not.toHaveAttribute("inert");
  portal.remove();
});
