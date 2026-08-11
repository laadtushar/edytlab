/**
 * The empty state is the first thing a new user reads, and it made three
 * claims: which formats load, which key opens a file, and (visually) that
 * the wordmark is set in the display serif. Two were wrong and the third
 * never took effect.
 */

import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { EmptyState } from "../components/EmptyState";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("EmptyState", () => {
  /**
   * `audio-decoder` builds symphonia with mp3/wav/flac, and symphonia's
   * defaults add ogg + vorbis. Nothing provides isomp4 or aac, so an
   * .m4a was advertised on the landing screen and failed on open.
   *
   * Note ogg IS supported — the declared feature list understates what is
   * compiled, which is why this list came from `cargo tree`, not from
   * reading Cargo.toml.
   */
  it("advertises only formats the decoder can actually open", () => {
    render(<EmptyState onOpen={() => {}} />);
    const formats = screen.getByText(/wav/i, { selector: "p" });

    expect(formats.textContent).toContain("wav");
    expect(formats.textContent).toContain("mp3");
    expect(formats.textContent).toContain("flac");
    expect(formats.textContent).toContain("ogg");

    expect(
      formats.textContent,
      "no isomp4/aac codec is compiled in; loading one fails",
    ).not.toContain("m4a");
    expect(formats.textContent).not.toContain("aac");
  });

  /**
   * The menu registers `CmdOrCtrl+O`, which resolves to Cmd on macOS.
   * The hint was hardcoded "Ctrl+O" — the wrong key for the app's single
   * most important action, on the platform it ships to first.
   */
  it("shows the Command key on macOS", () => {
    vi.stubGlobal("navigator", { platform: "MacIntel", userAgent: "Macintosh" });
    render(<EmptyState onOpen={() => {}} />);
    expect(screen.getByText(/⌘\+O/)).toBeTruthy();
  });

  it("shows Ctrl elsewhere", () => {
    vi.stubGlobal("navigator", { platform: "Win32", userAgent: "Windows NT" });
    render(<EmptyState onOpen={() => {}} />);
    expect(screen.getByText(/Ctrl\+O/)).toBeTruthy();
  });

  /**
   * A class assertion rather than a visual one, deliberately: jsdom
   * applies no CSS, so the rendered font cannot be observed here.
   *
   * `font-[var(--x)]` is ambiguous in Tailwind — it cannot infer whether
   * an arbitrary value is a family or a weight, and resolved it as a
   * weight, so the wordmark silently rendered in the body face. The
   * `family-name:` hint is what disambiguates it, and this pins it.
   */
  it("uses the family-name hint so the serif face actually applies", () => {
    const { container } = render(<EmptyState onOpen={() => {}} />);
    const h1 = container.querySelector("h1");
    expect(h1?.className).toContain("font-[family-name:var(--font-serif)]");
  });
});
