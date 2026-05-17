import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { TemplatePickerModal } from "../components/TemplatePickerModal";

const TEMPLATES = [
  { name: "Podcast", description: "Two-speaker podcast" },
  { name: "Music", description: "Four-track music" },
];

describe("TemplatePickerModal", () => {
  it("renders nothing when closed", () => {
    const { container } = render(
      <TemplatePickerModal open={false} templates={TEMPLATES} onSelect={vi.fn()} onClose={vi.fn()} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders template names when open", () => {
    render(
      <TemplatePickerModal open={true} templates={TEMPLATES} onSelect={vi.fn()} onClose={vi.fn()} />,
    );
    expect(screen.getByText("Podcast")).toBeInTheDocument();
    expect(screen.getByText("Music")).toBeInTheDocument();
  });

  it("calls onSelect with template name", async () => {
    const onSelect = vi.fn();
    render(
      <TemplatePickerModal open={true} templates={TEMPLATES} onSelect={onSelect} onClose={vi.fn()} />,
    );
    await userEvent.click(screen.getByText("Podcast"));
    expect(onSelect).toHaveBeenCalledWith("Podcast");
  });
});
