/**
 * The graph's rename overlay can actually be submitted (#253).
 *
 * The Save button carried a bare `disabled` attribute and the tooltip
 * "available after M24 lands" long after M24 landed. A disabled default
 * submit button also suppresses implicit form submission, so Enter in
 * the input did nothing either — and Cancel is `type="button"`, so the
 * form had no reachable submit path at all.
 *
 * The result was an overlay a user could open and type into but never
 * commit, sitting on top of a working `rename_node` command that the
 * component's own doc comment claimed it was wired to. Naming a version
 * from the graph — the natural place to do it — was impossible.
 *
 * These drive the real overlay, so they fail if it is ever hard-disabled
 * again.
 */

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { RenameOverlay } from "../components/GraphView";

function mount() {
  const onSubmit = vi.fn(async () => undefined);
  const onClose = vi.fn();
  render(
    <RenameOverlay nodeId="abc1234def" onSubmit={onSubmit} onClose={onClose} />,
  );
  return { user: userEvent.setup(), onSubmit, onClose };
}

const input = () => screen.getByLabelText(/rename/i);
const saveButton = () => screen.getByRole("button", { name: /save/i });

describe("the graph rename overlay", () => {
  it("submits the typed label when Save is clicked", async () => {
    const { user, onSubmit } = mount();

    await user.type(input(), "mix v2");
    expect(saveButton()).toBeEnabled();
    await user.click(saveButton());

    expect(onSubmit).toHaveBeenCalledWith("mix v2");
  });

  /**
   * The half a "just remove the disabled attribute" fix could still
   * miss: the disabled default submit is *also* what stopped Enter
   * working, and Enter is how anyone actually names a thing.
   */
  it("submits on Enter in the input", async () => {
    const { user, onSubmit } = mount();

    await user.type(input(), "chapter two{Enter}");

    expect(onSubmit).toHaveBeenCalledWith("chapter two");
  });

  it("closes after a successful submit", async () => {
    const { user, onSubmit, onClose } = mount();

    await user.type(input(), "done{Enter}");

    expect(onSubmit).toHaveBeenCalled();
    await vi.waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("will not submit an empty or whitespace-only name", async () => {
    const { user, onSubmit } = mount();

    expect(saveButton()).toBeDisabled();
    await user.type(input(), "   {Enter}");

    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("trims the label rather than saving the user's stray spaces", async () => {
    const { user, onSubmit } = mount();

    await user.type(input(), "  padded  {Enter}");

    expect(onSubmit).toHaveBeenCalledWith("padded");
  });
});
