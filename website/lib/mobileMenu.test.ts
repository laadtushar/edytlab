// @vitest-environment jsdom
/**
 * The phone menu's behaviour in a DOM: what the script adds to the
 * `<details>` disclosure. (That the links are in the server's HTML, so the
 * menu works without it, is `siteNav.test.ts`.)
 */

import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";

import { MobileMenu } from "../components/landing/mobile-menu";

afterEach(() => {
  cleanup();
  document.body.style.overflow = "";
});

/** `details.open = …` fires `toggle` on a later task in jsdom, as in a browser. */
const settle = () => act(() => new Promise<void>((r) => setTimeout(r, 0)));

async function open() {
  const view = render(createElement("div", null, createElement(MobileMenu), createElement("button", { id: "outside" }, "outside")));
  const details = view.container.querySelector("details")!;
  const summary = view.container.querySelector("summary")!;
  details.open = true;
  await settle();
  return { view, details, summary };
}

describe("the phone menu", () => {
  it("starts closed, with the button collapsed", () => {
    const { container } = render(createElement(MobileMenu));
    expect(container.querySelector("details")!.open).toBe(false);
    expect(container.querySelector("summary")!.getAttribute("aria-expanded")).toBe("false");
  });

  it("opens: aria-expanded true, focus in the panel, the page not scrolling behind it", async () => {
    const { details, summary } = await open();
    expect(summary.getAttribute("aria-expanded")).toBe("true");
    const first = details.querySelector("a")!;
    expect(document.activeElement).toBe(first);
    expect(document.body.style.overflow).toBe("hidden");
  });

  it("closes on Escape, puts focus back on the button and lets the page scroll", async () => {
    const { details, summary } = await open();
    fireEvent.keyDown(document, { key: "Escape" });
    await settle();
    expect(details.open).toBe(false);
    expect(summary.getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(summary);
    expect(document.body.style.overflow).toBe("");
  });

  it("closes on a tap on a link", async () => {
    const { details } = await open();
    const link = details.querySelector<HTMLAnchorElement>("a[href='/videos']")!;
    link.addEventListener("click", (e) => e.preventDefault());
    fireEvent.click(link);
    await settle();
    expect(details.open).toBe(false);
    expect(document.body.style.overflow).toBe("");
  });

  it("closes on a tap outside, and not on a tap inside", async () => {
    const { view, details } = await open();
    fireEvent.pointerDown(details.querySelector("nav")!);
    await settle();
    expect(details.open).toBe(true);
    fireEvent.pointerDown(view.container.querySelector("#outside")!);
    await settle();
    expect(details.open).toBe(false);
  });

  it("restores the body's own overflow, not an empty one", async () => {
    document.body.style.overflow = "scroll";
    const { details } = await open();
    expect(document.body.style.overflow).toBe("hidden");
    fireEvent.keyDown(document, { key: "Escape" });
    await settle();
    expect(details.open).toBe(false);
    expect(document.body.style.overflow).toBe("scroll");
  });
});
