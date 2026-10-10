/**
 * The LabyNator family block in the footer: one list, the same on every
 * site, linking each member at the URL that actually serves it.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { FOUNDER, LABYNATOR, familyApps, otherApps } from "./family";
import { siteConfig } from "./site";

const ROOT = fileURLToPath(new URL("../", import.meta.url));
const read = (path: string) => readFileSync(join(ROOT, path), "utf8");

describe("the family list", () => {
  it("names the studio and the founder at their canonical URLs", () => {
    expect(LABYNATOR).toEqual({ name: "LabyNator", url: "https://www.labynator.com" });
    expect(FOUNDER).toEqual({ name: "Tushar Laad", url: "https://www.tusharlaad.com" });
  });

  it("lists the four apps in the family order, with labynator's blurbs", () => {
    expect(familyApps.map((a) => [a.name, a.blurb, a.url])).toEqual([
      ["MemryLab", "Local AI Memory", "https://www.memrylab.com"],
      ["XpenseLab", "Expense tracking", "https://xpenselab.com"],
      ["HyredLab", "Job tracking", "https://www.hyredlab.com"],
      ["edytlab", "AI audio editor", "https://www.edytlab.com"],
    ]);
  });

  it("includes this site at the URL siteConfig names, and the footer leaves it out", () => {
    expect(familyApps.map((a) => a.url)).toContain(siteConfig.url);
    expect(otherApps.map((a) => a.name)).toEqual(["MemryLab", "XpenseLab", "HyredLab"]);
  });

  it("links only to canonical hosts, none of them an apex that redirects", () => {
    for (const { url } of [LABYNATOR, FOUNDER, ...familyApps]) {
      const { protocol, pathname, hostname } = new URL(url);
      expect(protocol, url).toBe("https:");
      expect(pathname, url).toBe("/");
      // The one deliberate exception: XpenseLab serves from its apex.
      if (hostname !== "xpenselab.com") expect(hostname, url).toMatch(/^www\./);
    }
  });

  it("names its source of truth at the top", () => {
    expect(read("lib/family.ts").split("*/")[0]).toContain("src/lib/apps.ts");
  });
});

describe("the footer's family block", () => {
  const footer = read("components/landing/footer.tsx");
  const block = footer.slice(footer.indexOf('aria-label="The LabyNator family"'), footer.indexOf("The rule draws out"));

  it("is on every page, because every page ends in the footer", () => {
    expect(block.length).toBeGreaterThan(0);
    for (const shell of [
      "components/landing/page-shell.tsx",
      "components/landing/legal-shell.tsx",
      "components/docs/doc-shell.tsx",
      "app/page.tsx",
      "app/blog/page.tsx",
      "app/blog/[slug]/page.tsx",
    ]) {
      expect(read(shell), shell).toContain("<Footer />");
    }
  });

  it("reads Part of LabyNator, the other apps, then Built by Tushar Laad", () => {
    expect(block.indexOf("Part of")).toBeGreaterThan(-1);
    expect(block.indexOf("LABYNATOR.url")).toBeLessThan(block.indexOf("otherApps.map"));
    expect(block.indexOf("otherApps.map")).toBeLessThan(block.indexOf("Built by"));
    expect(block.indexOf("Built by")).toBeLessThan(block.indexOf("FOUNDER.url"));
    expect(block).toContain("{app.name} &mdash; {app.blurb}");
  });

  it("draws its links from the one list, not from copies of the URLs", () => {
    expect(footer).toContain('from "@/lib/family"');
    expect(block).not.toMatch(/https?:\/\//);
  });

  it("uses plain same-tab links, followed", () => {
    expect(block).not.toContain("<Link");
    expect(block).not.toContain("target=");
    expect(block).not.toContain("rel=");
    expect(block).not.toContain("nofollow");
  });

  it("shows without scripts and without motion: no entrance tween around it", () => {
    // `Reveal`, `Stagger` and friends set their from-state with GSAP, so
    // they are safe too, but this block needs none of them. Keeping it
    // plain markup is what makes "visible without JavaScript" true by
    // construction rather than by a component's care.
    expect(block).not.toMatch(/<(Reveal|Stagger|Cascade|LineDraw|Parallax)\b/);
    expect(block).not.toMatch(/opacity-0|invisible|hidden/);
  });
});
