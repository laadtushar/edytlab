import { StrictMode, useState } from "react";
import { OverlayDialog } from "../components/OverlayDialog";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { TemplatePickerModal } from "../components/TemplatePickerModal";
import { ShortcutsOverlay } from "../components/ShortcutsOverlay";

function Overlays() {
  const [templates, setTemplates] = useState(false);
  const [shortcuts, setShortcuts] = useState(false);
  return <>
    <button onClick={() => setTemplates(true)}>Open templates</button>
    <TemplatePickerModal open={templates} templates={[
      { name: "Podcast", description: "Podcast template" },
    ]} onSelect={() => setShortcuts(true)} onClose={() => setTemplates(false)} />
    <ShortcutsOverlay open={shortcuts} onClose={() => setShortcuts(false)} />
  </>;
}

describe("overlay dialog contract", () => {
  it("names the template dialog, traps Tab both ways, and restores its opener", async () => {
    const user = userEvent.setup();
    render(<Overlays />);
    const opener = screen.getByRole("button", { name: "Open templates" });
    await user.click(opener);
    const dialog = await screen.findByRole("dialog", { name: "Start from template" });
    expect(dialog).toContainElement(document.activeElement as HTMLElement);
    const first = screen.getByRole("button", { name: /Podcast template/ });
    const last = screen.getByRole("button", { name: "Cancel" });
    last.focus();
    await user.tab();
    expect(first).toHaveFocus();
    await user.tab({ shift: true });
    expect(last).toHaveFocus();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(opener).toHaveFocus());
  });

  it("closes only the top overlay per Escape and restores focus down the stack", async () => {
    const user = userEvent.setup();
    render(<Overlays />);
    await user.click(screen.getByRole("button", { name: "Open templates" }));
    const template = await screen.findByRole("dialog", { name: "Start from template" });
    const choice = screen.getByRole("button", { name: /Podcast template/ });
    await user.click(choice);
    await screen.findByRole("dialog", { name: "Keyboard Shortcuts" });
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(choice).toHaveFocus());
    expect(template).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(screen.getByRole("button", { name: "Open templates" })).toHaveFocus());
  });
});

describe("dialog lifecycle", () => {
  it("keeps an empty dialog focusable and restores pre-existing inert state on unmount", async () => {
    const background = document.createElement("button");
    background.setAttribute("inert", "");
    document.body.append(background);
    const close = () => {};
    const { unmount } = render(<StrictMode>
      <OverlayDialog active onClose={close} aria-labelledby="empty-heading">
        <h2 id="empty-heading">Empty dialog</h2>
      </OverlayDialog>
    </StrictMode>);
    const dialog = screen.getByRole("dialog", { name: "Empty dialog" });
    await waitFor(() => expect(dialog).toHaveFocus());
    fireEvent.keyDown(window, { key: "Tab", shiftKey: true });
    expect(dialog).toHaveFocus();
    unmount();
    expect(background).toHaveAttribute("inert");
    background.remove();
  });

  it("inerts newly inserted background controls and prevents programmatic focus escape", async () => {
    const user = userEvent.setup();
    render(<Overlays />);
    await user.click(screen.getByRole("button", { name: "Open templates" }));
    const dialog = screen.getByRole("dialog", { name: "Start from template" });
    const outside = document.createElement("button");
    document.body.append(outside);
    await waitFor(() => expect(outside).toHaveAttribute("inert"));
    outside.focus();
    expect(dialog).toContainElement(document.activeElement as HTMLElement);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(outside).not.toHaveAttribute("inert"));
    outside.remove();
  });
});


it("keeps a covered dialog inert if it closes before the top dialog", async () => {
  const view = (lower: boolean, upper: boolean) => <>
    <OverlayDialog active={lower} onClose={() => {}} aria-labelledby="lower-heading">
      <h2 id="lower-heading">Lower</h2><button>Lower action</button>
    </OverlayDialog>
    <OverlayDialog active={upper} onClose={() => {}} aria-labelledby="upper-heading">
      <h2 id="upper-heading">Upper</h2><button>Upper action</button>
    </OverlayDialog>
  </>;
  const { rerender } = render(view(true, true));
  const lower = screen.getByText("Lower").parentElement!;
  const upper = screen.getByRole("dialog", { name: "Upper" });
  rerender(view(false, true));
  await waitFor(() => expect(upper).toHaveAttribute("aria-modal", "true"));
  rerender(view(false, false));
  await waitFor(() => expect(lower).toHaveAttribute("inert"));
  expect(upper).toHaveAttribute("inert");
});
