import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { ShortcutsOverlay } from "../components/ShortcutsOverlay";

describe("ShortcutsOverlay", () => {
  it("renders nothing when closed", () => {
    const { container } = render(
      <ShortcutsOverlay open={false} onClose={() => {}} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("renders shortcut list when open", () => {
    render(<ShortcutsOverlay open={true} onClose={() => {}} />);
    expect(screen.getByText("Space")).toBeInTheDocument();
    expect(screen.getByText("Ctrl+Z")).toBeInTheDocument();
    expect(screen.getByText("?")).toBeInTheDocument();
  });

  it("calls onClose when Escape pressed", async () => {
    const onClose = vi.fn();
    render(<ShortcutsOverlay open={true} onClose={onClose} />);
    const user = userEvent.setup();
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalled();
  });
});
