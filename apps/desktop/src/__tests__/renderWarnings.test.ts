/**
 * The words the window uses for an effect a render leaves out (#532).
 *
 * The backend sends data (where, which kind, why); these pin what a person
 * reads, because the sentence is the whole of the feature: a warning that
 * does not say which track and which effect is not one the user can act on.
 */

import { describe, expect, it } from "vitest";

import { compareNotice, exportNotice, mixNotice, scopeLabel } from "../lib/renderWarnings";
import type { RenderWarning } from "../lib/tauri-bridge";

const reverbOnVocals: RenderWarning = {
  scope: { type: "track", index: 1, name: "Vocals" },
  effect_index: 0,
  kind: "reverb",
  reason: "not_yet_streamable",
};
const gateOnMaster: RenderWarning = {
  scope: { type: "master" },
  effect_index: 2,
  kind: "noise_gate",
  reason: "not_yet_streamable",
};
const echoOnBus: RenderWarning = {
  scope: { type: "bus", name: "Verb" },
  effect_index: 0,
  kind: "echo",
  reason: "not_yet_streamable",
};
const wobbulator: RenderWarning = {
  scope: { type: "master" },
  effect_index: 0,
  kind: "wobbulator",
  reason: "unknown",
};

describe("where", () => {
  it("names a track by the index the assistant uses and its name", () => {
    expect(scopeLabel(reverbOnVocals.scope)).toBe('track 1 "Vocals"');
  });

  it("names a track with no name by its index alone", () => {
    expect(scopeLabel({ type: "track", index: 3, name: "" })).toBe("track 3");
  });

  it("names a bus, and the master bus", () => {
    expect(scopeLabel(echoOnBus.scope)).toBe('bus "Verb"');
    expect(scopeLabel({ type: "bus", name: "" })).toBe("a bus");
    expect(scopeLabel(gateOnMaster.scope)).toBe("the master bus");
  });
});

describe("the mix notice", () => {
  it("says nothing when nothing is left out", () => {
    expect(mixNotice([])).toBeNull();
  });

  it("is one sentence for one effect: which, where, why, and what clears it", () => {
    expect(mixNotice([reverbOnVocals])).toEqual({
      lead:
        'Reverb on track 1 "Vocals" is left out: this build can\'t run it in a chain yet. ' +
        "Remove or bypass it to clear this.",
      items: [],
    });
  });

  it("writes a kind with an underscore as words", () => {
    expect(mixNotice([gateOnMaster])?.lead).toMatch(/^Noise gate on the master bus is left out/);
  });

  it("quotes a kind this build has never heard of, and says so", () => {
    const notice = mixNotice([wobbulator]);
    expect(notice?.lead).toBe(
      'Effect "wobbulator" on the master bus is left out: this build doesn\'t know that effect. ' +
        "Remove or bypass it to clear this.",
    );
  });

  it("counts several and lists each one", () => {
    const notice = mixNotice([reverbOnVocals, echoOnBus, gateOnMaster]);
    expect(notice?.lead).toBe(
      "3 effects are left out of the mix. Remove or bypass them to clear this.",
    );
    expect(notice?.items).toEqual([
      'Reverb on track 1 "Vocals": this build can\'t run it in a chain yet.',
      'Echo on bus "Verb": this build can\'t run it in a chain yet.',
      "Noise gate on the master bus: this build can't run it in a chain yet.",
    ]);
  });
});

describe("the export notice", () => {
  it("says nothing when the file lacks nothing", () => {
    expect(exportNotice("take.wav", [])).toBeNull();
  });

  it("names the file and what it was written without", () => {
    expect(exportNotice("take.wav", [reverbOnVocals])).toEqual({
      lead: 'Exported take.wav without Reverb on track 1 "Vocals": this build can\'t run it in a chain yet.',
      items: [],
    });
  });

  it("counts several and lists each one", () => {
    const notice = exportNotice("take.wav", [reverbOnVocals, gateOnMaster]);
    expect(notice?.lead).toBe("Exported take.wav without 2 effects it stores:");
    expect(notice?.items).toHaveLength(2);
  });
});

describe("the compare notice", () => {
  it("says nothing when neither side lacks anything", () => {
    expect(compareNotice([], [])).toBeNull();
  });

  it("tells the sides apart", () => {
    const notice = compareNotice([reverbOnVocals], [gateOnMaster]);
    expect(notice?.items).toEqual([
      'A · Reverb on track 1 "Vocals": this build can\'t run it in a chain yet.',
      "B · Noise gate on the master bus: this build can't run it in a chain yet.",
    ]);
  });
});
