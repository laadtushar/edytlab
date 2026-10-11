/**
 * The empty state is the first thing a new user reads, and it made three
 * claims: which formats load, which key opens a file, and (visually) what
 * the wordmark is. The first two were wrong; the third was a display
 * serif that never took effect and is now the site's own lockup.
 */

import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { EmptyState } from "../components/EmptyState";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("EmptyState", () => {
  /**
   * "Open project" is a different verb from "Open audio" (#156). Until
   * this existed, the only way into edytlab was to open an audio
   * *file*, which created a project as a side effect and never said so
   * — leaving no way at all to reopen one by folder.
   */
  it("offers opening a project, distinctly from opening audio", () => {
    const onOpen = vi.fn();
    const onOpenProject = vi.fn();
    render(<EmptyState onOpen={onOpen} onOpenProject={onOpenProject} />);

    screen.getByTestId("empty-state-open-project-button").click();
    expect(onOpenProject).toHaveBeenCalled();
    expect(onOpen).not.toHaveBeenCalled();
  });

  /**
   * The empty state arrives once: the logo builds, then its groups
   * follow top to bottom. Nothing loops (the old ten pulsing bars kept
   * an idle window repainting), and the root has no fade of its own, or
   * the two would stack.
   */
  it("reveals itself once: the logo builds, then the groups arrive in order", () => {
    render(
      <EmptyState
        onOpen={() => {}}
        onOpenProject={() => {}}
        recents={[{ path: "/p/one", name: "one", last_opened_at: null }]}
        onOpenRecent={() => {}}
        onForgetRecent={() => {}}
      />,
    );
    const root = screen.getByTestId("empty-state");
    expect(root.className).not.toContain("app-fade-in");
    expect(root.querySelector("svg.logo-build")).not.toBeNull();
    // Nothing infinite on an idle screen.
    expect(root.querySelector('[class*="animate-"]')).toBeNull();

    const places = Array.from(root.querySelectorAll<HTMLElement>(".reveal-in")).map(
      (el) => Number(el.style.getPropertyValue("--i")),
    );
    // Heading, lead, buttons, then the rest together; never out of order.
    expect(places.slice(0, 3)).toEqual([0, 1, 2]);
    expect(places.length).toBeGreaterThan(3);
    expect([...places].sort((a, b) => a - b)).toEqual(places);
    expect(Math.max(...places)).toBe(3);
    // The recents list is one of the last group.
    expect(screen.getByTestId("recent-projects").className).toContain("reveal-in");
  });

  it("hides the project button when the caller cannot open one", () => {
    render(<EmptyState onOpen={() => {}} />);
    expect(screen.queryByTestId("empty-state-open-project-button")).toBeNull();
  });

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
   * The wordmark is the logo mark and the name, as on the site. The name
   * of the heading must stay "edytlab" (the mark is decorative, so it is
   * `aria-hidden` and sits beside the heading, not in it), and "edyt" is
   * the brand gradient. A structural assertion rather than a visual one,
   * deliberately: jsdom applies no CSS. The face the heading is drawn in
   * is checked in a real browser, by `e2e/fonts.spec.ts`.
   */
  it("is named edytlab, with the decorative logo mark outside the heading", () => {
    const { container } = render(<EmptyState onOpen={() => {}} />);
    const h1 = screen.getByRole("heading", { level: 1, name: "edytlab" });

    // Found by its own hook: the folder icon on the Open Audio button is
    // also an `svg[aria-hidden]`, and would satisfy a looser selector.
    const mark = container.querySelector("[data-logo-bar]")?.closest("svg") ?? null;
    expect(mark, "the logo mark").not.toBeNull();
    expect(mark?.getAttribute("aria-hidden")).toBe("true");
    expect(mark?.getAttribute("focusable")).toBe("false");
    expect(h1.contains(mark), "the mark must not be part of the heading's name").toBe(false);

    const emphasis = h1.querySelector(".gradient-text");
    expect(emphasis?.textContent).toBe("edyt");
  });
});
