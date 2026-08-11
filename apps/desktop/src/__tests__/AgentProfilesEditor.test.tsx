/**
 * The profile editor had three defects that all shared a shape: the form
 * accepted input, reported success, and stored something other than what
 * the user meant.
 *
 * Found by an audit pass over the components that had no tests. Each of
 * these was demonstrated against the original before being fixed.
 */

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const listAgentProfiles = vi.fn();
const readAgentProfile = vi.fn();
const upsertAgentProfile = vi.fn();
const deleteAgentProfile = vi.fn();
const getActiveAgentProfile = vi.fn();
const setActiveAgentProfile = vi.fn();

vi.mock("../lib/tauri-bridge", () => ({
  listAgentProfiles: (...a: unknown[]) => listAgentProfiles(...a),
  readAgentProfile: (...a: unknown[]) => readAgentProfile(...a),
  upsertAgentProfile: (...a: unknown[]) => upsertAgentProfile(...a),
  deleteAgentProfile: (...a: unknown[]) => deleteAgentProfile(...a),
  getActiveAgentProfile: (...a: unknown[]) => getActiveAgentProfile(...a),
  setActiveAgentProfile: (...a: unknown[]) => setActiveAgentProfile(...a),
}));

import { AgentProfilesEditor } from "../components/AgentProfilesEditor";

beforeEach(() => {
  vi.clearAllMocks();
  listAgentProfiles.mockResolvedValue([
    { name: "precision", description: "careful edits" },
  ]);
  readAgentProfile.mockResolvedValue({
    name: "precision",
    description: "careful edits",
    model: null,
    tools: null,
    body: "Be careful.",
  });
  getActiveAgentProfile.mockResolvedValue(null);
  upsertAgentProfile.mockResolvedValue(undefined);
});

async function openNewProfileForm() {
  render(<AgentProfilesEditor />);
  await screen.findByText("precision");
  fireEvent.click(screen.getByText("New"));
  return screen.getByTestId("profiles-name") as HTMLInputElement;
}

describe("AgentProfilesEditor", () => {
  /**
   * The field is documented "comma-separated", and a comma could not be
   * typed into it.
   *
   * It was a controlled input whose value was `tools.join(", ")` while
   * every keystroke re-parsed with `.filter(Boolean)`. That filter drops
   * the empty segment a fresh comma creates, so React restored the joined
   * value and the comma vanished.
   */
  it("lets a comma survive being typed into the tools whitelist", async () => {
    await openNewProfileForm();
    const tools = screen.getByTestId("profiles-tools") as HTMLInputElement;

    // The moment that breaks: a trailing comma, which is what exists for
    // one keystroke every time someone types a separator. Setting the
    // whole string at once does NOT reproduce it — the parse round-trips
    // cleanly — which is why this asserts on the intermediate state.
    fireEvent.change(tools, { target: { value: "load," } });

    expect(
      tools.value,
      "the old field parsed and re-joined on every keystroke, and " +
        "`.filter(Boolean)` dropped the empty segment, so the comma was " +
        "erased as fast as it was typed",
    ).toBe("load,");

    fireEvent.change(tools, { target: { value: "load, gain" } });
    expect(tools.value).toBe("load, gain");
  });

  it("saves the whitelist as separate tools, not one joined name", async () => {
    const name = await openNewProfileForm();
    fireEvent.change(name, { target: { value: "new-profile" } });
    fireEvent.change(screen.getByTestId("profiles-tools"), {
      target: { value: "load, gain, reverb" },
    });
    fireEvent.click(screen.getByText("Save"));

    await waitFor(() => expect(upsertAgentProfile).toHaveBeenCalled());
    const [, content] = upsertAgentProfile.mock.calls[0];
    expect(content.tools).toEqual(["load", "gain", "reverb"]);
  });

  /**
   * Creating a profile over an existing name overwrote it and said
   * "Saved." The backend upsert is unconditional by design — it is the
   * same call the edit path uses — so the create path is the only layer
   * that can tell "save my edits" from "make a new one".
   */
  it("refuses to create a profile over an existing name", async () => {
    const name = await openNewProfileForm();
    fireEvent.change(name, { target: { value: "precision" } });
    fireEvent.click(screen.getByText("Save"));

    await screen.findByText(/already exists/i);
    expect(
      upsertAgentProfile,
      "the existing profile's body would have been replaced with an empty draft",
    ).not.toHaveBeenCalled();
  });

  /**
   * A model override needs both halves. An empty provider resolves to no
   * provider at all, which disables the agent once the profile is
   * activated — so a half-filled pair must not persist.
   */
  it("does not persist a model override with only the id filled in", async () => {
    const name = await openNewProfileForm();
    fireEvent.change(name, { target: { value: "new-profile" } });
    fireEvent.change(screen.getByTestId("profiles-model-id"), {
      target: { value: "claude-opus-4-7" },
    });
    fireEvent.click(screen.getByText("Save"));

    await waitFor(() => expect(upsertAgentProfile).toHaveBeenCalled());
    const [, content] = upsertAgentProfile.mock.calls[0];
    expect(
      content.model,
      'a provider of "" disables the agent when the profile is activated',
    ).toBeNull();
  });

  it("does not persist a model override with only the provider filled in", async () => {
    const name = await openNewProfileForm();
    fireEvent.change(name, { target: { value: "new-profile" } });
    fireEvent.change(screen.getByTestId("profiles-model-provider"), {
      target: { value: "anthropic" },
    });
    fireEvent.click(screen.getByText("Save"));

    await waitFor(() => expect(upsertAgentProfile).toHaveBeenCalled());
    const [, content] = upsertAgentProfile.mock.calls[0];
    expect(content.model).toBeNull();
  });

  it("persists a model override once both halves are filled", async () => {
    const name = await openNewProfileForm();
    fireEvent.change(name, { target: { value: "new-profile" } });
    fireEvent.change(screen.getByTestId("profiles-model-provider"), {
      target: { value: "anthropic" },
    });
    fireEvent.change(screen.getByTestId("profiles-model-id"), {
      target: { value: "claude-opus-4-7" },
    });
    fireEvent.click(screen.getByText("Save"));

    await waitFor(() => expect(upsertAgentProfile).toHaveBeenCalled());
    const [, content] = upsertAgentProfile.mock.calls[0];
    expect(content.model).toEqual({
      provider: "anthropic",
      id: "claude-opus-4-7",
    });
  });
});
