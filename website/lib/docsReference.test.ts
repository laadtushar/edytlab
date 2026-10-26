// @vitest-environment jsdom
/**
 * The two split reference pages: the Audio Tools Reference and the API
 * Reference are each an index plus one page per area, all made from one
 * data array.
 *
 * What is held here:
 *
 *  - the pages are made from the data: `generateStaticParams` returns one
 *    area per entry, so a new area needs no page file, and a page for a
 *    slug that is not an area is a 404;
 *  - every name is on its area's page with the id a link to it needs, and
 *    the index lists every name with a link to that place (the page reads
 *    without scripts);
 *  - an old link with a hash (`/docs/tools#cut_range`) is mapped to the
 *    page that has it now, and the one component that does it does so;
 *  - each area page stays short enough for a phone (a word budget, which
 *    is what the page's height at 390 px follows: the PR measures it);
 *  - the pager, the breadcrumbs and the metadata of each area page.
 *
 * That the data matches the app (every tool, every command) is
 * `crates/tools/tests/website_tool_docs.rs` and `apiReferencePage.test.ts`.
 */

import { cleanup, render } from "@testing-library/react";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const nav = vi.hoisted(() => {
  // jsdom has no `matchMedia`, which GSAP's ScrollTrigger asks for when
  // `lib/gsap.ts` registers it as the docs shell is imported.
  window.matchMedia ??= ((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    addListener() {},
    removeListener() {},
    dispatchEvent: () => false,
  })) as typeof window.matchMedia;
  return { pathname: "/docs/tools", replace: vi.fn() };
});
vi.mock("next/navigation", () => ({
  usePathname: () => nav.pathname,
  useRouter: () => ({ replace: nav.replace }),
  notFound: () => {
    throw new Error("NEXT_NOT_FOUND");
  },
}));

import ApiAreaPage, * as apiAreaModule from "../app/docs/api-reference/[area]/page";
import ApiReferencePage from "../app/docs/api-reference/page";
import ToolsAreaPage, * as toolsAreaModule from "../app/docs/tools/[area]/page";
import ToolsPage from "../app/docs/tools/page";
import { HashRedirect } from "../components/docs/hash-redirect";
import { apiAreas } from "./docs/api-data";
import {
  allIds,
  entriesAtoZ,
  hashTargets,
  neighbours,
  resolveHash,
  slugify,
  type Reference,
} from "./docs/reference";
import { API_ALIASES, API_BASE, TOOLS_ALIASES, TOOLS_BASE, apiReference, toolCount, toolsReference } from "./docs/references";
import { toolAreas } from "./docs/tools-data";
import { absoluteUrl } from "./seo";

const references: { name: string; reference: Reference; aliases: Record<string, string> }[] = [
  { name: "tools", reference: toolsReference, aliases: TOOLS_ALIASES },
  { name: "api", reference: apiReference, aliases: API_ALIASES },
];

const html = (element: unknown) => renderToStaticMarkup(element as Parameters<typeof renderToStaticMarkup>[0]);

/** The visible text of some markup, without the scripts and the markup. */
const textOf = (markup: string) =>
  markup
    .replace(/<script[\s\S]*?<\/script>/g, " ")
    .replace(/<[^>]+>/g, " ")
    .replace(/&#x27;/g, "'")
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, "&")
    .replace(/\s+/g, " ")
    .trim();

async function areaPage(kind: "tools" | "api", slug: string): Promise<string> {
  nav.pathname = `${kind === "tools" ? TOOLS_BASE : API_BASE}/${slug}`;
  const props = { params: Promise.resolve({ area: slug }) };
  return html(await (kind === "tools" ? ToolsAreaPage(props) : ApiAreaPage(props)));
}

/** The JSON-LD blocks of some markup, parsed. */
const jsonLd = (markup: string): { itemListElement: { name: string; item: string }[] }[] =>
  [...markup.matchAll(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/g)].map((m) => JSON.parse(m[1]));

beforeEach(() => {
  nav.replace.mockReset();
  window.location.hash = "";
});
afterEach(cleanup);

describe.each(references)("the $name reference's data", ({ reference, aliases }) => {
  it("has areas with distinct, URL-safe slugs", () => {
    const slugs = reference.areas.map((a) => a.slug);
    expect(slugs.length).toBeGreaterThan(1);
    expect(new Set(slugs).size).toBe(slugs.length);
    for (const slug of slugs) expect(slug).toMatch(/^[a-z][a-z0-9]*(-[a-z0-9]+)*$/);
  });

  it("gives every area a one-line summary and something to list", () => {
    for (const area of reference.areas) {
      expect(area.summary.length, `${area.slug} summary`).toBeGreaterThan(20);
      expect(area.summary, `${area.slug} summary is one sentence`).not.toMatch(/\n/);
      expect(area.countLabel, `${area.slug} count`).toMatch(/\d/);
      expect(area.sections.length, `${area.slug} sections`).toBeGreaterThan(0);
    }
  });

  it("never defines the same id twice, on one page or across pages", () => {
    // Across pages too: the redirect for an old link maps an id to one
    // page, so an id on two would send one of them to the wrong place.
    const seen = new Map<string, string>();
    const clashes: string[] = [];
    for (const { id, at } of allIds(reference.areas)) {
      const earlier = seen.get(id);
      if (earlier !== undefined) clashes.push(`${id}: ${earlier} and ${at}`);
      else seen.set(id, at);
    }
    expect(clashes).toEqual([]);
  });

  it("maps every id, and every area, to the page that has it", () => {
    const targets = hashTargets(reference.areas, aliases);
    for (const area of reference.areas) {
      expect(targets[area.slug], `#${area.slug}`).toMatch(new RegExp(`^${area.slug}(#.+)?$`));
      for (const section of area.sections) {
        expect(targets[section.id], `#${section.id}`).toBe(`${area.slug}#${section.id}`);
        for (const entry of section.entries) expect(targets[entry.id], `#${entry.id}`).toBe(`${area.slug}#${entry.id}`);
      }
    }
    for (const [alias, slug] of Object.entries(aliases)) {
      expect(reference.areas.some((a) => a.slug === slug), `alias #${alias} names ${slug}`).toBe(true);
    }
  });

  it("is ordered so the pager walks every area once", () => {
    const walked: string[] = [];
    let at = reference.areas[0];
    for (;;) {
      walked.push(at.slug);
      const { next } = neighbours(reference.areas, at.slug);
      if (!next) break;
      expect(neighbours(reference.areas, next.slug).prev?.slug).toBe(at.slug);
      at = next;
    }
    expect(walked).toEqual(reference.areas.map((a) => a.slug));
    expect(neighbours(reference.areas, "no-such-area")).toEqual({});
  });
});

describe("resolving an old hash", () => {
  const targets = hashTargets(toolsReference.areas);

  it("finds a tool, a section and an area", () => {
    expect(resolveHash("#cut_range", targets)).toBe("region-editing#cut_range");
    expect(resolveHash("#region-editing", targets)).toBe("region-editing#region-editing");
    expect(resolveHash("#effect-chains", targets)).toBeDefined();
    expect(resolveHash("cut_range", targets)).toBe("region-editing#cut_range");
  });

  it("answers nothing for a hash that is not a name, or is empty", () => {
    expect(resolveHash("", targets)).toBeUndefined();
    expect(resolveHash("#", targets)).toBeUndefined();
    expect(resolveHash("#no-such-tool", targets)).toBeUndefined();
    expect(resolveHash("#%E0%A4%A", targets)).toBeUndefined();
  });

  it("does not read names off Object.prototype", () => {
    for (const hash of ["#constructor", "#__proto__", "#toString", "#hasOwnProperty"]) {
      expect(resolveHash(hash, targets), hash).toBeUndefined();
    }
  });

  it("slugifies a heading the way a section id is made", () => {
    expect(slugify("Rendering and A/B")).toBe("rendering-and-a-b");
    expect(slugify("  API Keys ")).toBe("api-keys");
  });
});

describe("the redirect component", () => {
  const targets = hashTargets(toolsReference.areas);
  const mount = () => render(createElement(HashRedirect, { base: TOOLS_BASE, targets }));

  it("sends an old link with a tool's hash to the area page, replacing the index in history", () => {
    window.location.hash = "#cut_range";
    mount();
    expect(nav.replace).toHaveBeenCalledExactlyOnceWith("/docs/tools/region-editing#cut_range");
  });

  it("leaves a link with no hash, or one it does not know, where it is", () => {
    mount();
    cleanup();
    window.location.hash = "#nothing-by-this-name";
    mount();
    expect(nav.replace).not.toHaveBeenCalled();
  });

  it("follows a hash put on the address while the page is open", () => {
    mount();
    window.location.hash = "#set_send";
    window.dispatchEvent(new HashChangeEvent("hashchange"));
    expect(nav.replace).toHaveBeenCalledExactlyOnceWith("/docs/tools/volume-and-dynamics#set_send");
  });

  it("renders nothing", () => {
    expect(mount().container.innerHTML).toBe("");
  });
});

describe.each([
  { name: "tools", mod: toolsAreaModule, areas: toolAreas, reference: toolsReference, kind: "tools" as const },
  { name: "api", mod: apiAreaModule, areas: apiAreas, reference: apiReference, kind: "api" as const },
])("the $name area pages", ({ mod, areas, reference, kind }) => {
  it("are made from the data: one static page per area, and a 404 for anything else", async () => {
    expect(mod.generateStaticParams()).toEqual(areas.map((a) => ({ area: a.slug })));
    expect(mod.dynamicParams).toBe(false);
    await expect(areaPage(kind, "no-such-area")).rejects.toThrow("NEXT_NOT_FOUND");
    expect(await mod.generateMetadata({ params: Promise.resolve({ area: "no-such-area" }) })).toEqual({});
  });

  it.each(reference.areas.map((a) => [a.slug, a] as const))("%s: every name is on the page, with the id a link needs", async (slug, area) => {
    const markup = await areaPage(kind, slug);
    for (const section of area.sections) {
      expect(markup, `section #${section.id}`).toContain(`id="${section.id}"`);
      for (const entry of section.entries) {
        expect(markup, `#${entry.id} of ${slug}`).toContain(`id="${entry.id}"`);
        // And the "On this page" lists link to it (the disclosure and the column).
        expect(markup.split(`href="#${entry.id}"`).length - 1, `links to #${entry.id}`).toBe(2);
      }
    }
  });

  it.each(reference.areas.map((a) => [a.slug, a] as const))("%s: has one h1, the area's title, and a link back to the index", async (slug, area) => {
    const markup = await areaPage(kind, slug);
    expect((markup.match(/<h1\b/g) ?? []).length).toBe(1);
    expect(textOf(markup)).toContain(area.title);
    expect(markup).toContain(`href="${reference.base}"`);
  });

  it.each(reference.areas.map((a) => [a.slug, a] as const))("%s: names its neighbours, and no others", async (slug, area) => {
    const markup = await areaPage(kind, slug);
    const { prev, next } = neighbours(reference.areas, slug);
    const links = new Set([...markup.matchAll(new RegExp(`href="${reference.base}/([a-z0-9-]+)"`, "g"))].map((m) => m[1]));
    // The sidebar and the pager link only the previous and next area.
    expect([...links].sort()).toEqual([prev?.slug, next?.slug].filter(Boolean).sort());
    expect(markup.includes('rel="prev"')).toBe(prev !== undefined);
    expect(markup.includes('rel="next"')).toBe(next !== undefined);
    expect(slug).not.toBe(prev?.slug);
  });

  it.each(reference.areas.map((a) => [a.slug, a] as const))("%s: has a breadcrumb trail down to it", async (slug, area) => {
    const [crumbs] = jsonLd(await areaPage(kind, slug));
    expect(crumbs.itemListElement.map((i) => i.name)).toEqual(["Home", "Documentation", reference.name, area.title]);
    expect(crumbs.itemListElement.map((i) => i.item)).toEqual([
      absoluteUrl("/"),
      absoluteUrl("/docs"),
      absoluteUrl(reference.base),
      absoluteUrl(`${reference.base}/${slug}`),
    ]);
  });

  it.each(reference.areas.map((a) => [a.slug, a] as const))("%s: has its own canonical, social card and description", async (slug, area) => {
    const meta = await mod.generateMetadata({ params: Promise.resolve({ area: slug }) });
    const path = `${reference.base}/${slug}`;
    expect(meta.alternates?.canonical).toBe(path);
    expect((meta.openGraph as { url: string }).url).toBe(absoluteUrl(path));
    expect(String(meta.title)).toContain(area.title);
    expect(String(meta.description)).toContain(area.summary);
    expect(String(meta.description).length).toBeLessThanOrEqual(300);
  });

  it("each stays short enough for a phone", async () => {
    // The page's height at 390 px follows its words (about 6 to 10 px a
    // word, measured; the PR has the heights). 900 of them is about 8,000.
    const budget = 900;
    const long: string[] = [];
    for (const area of reference.areas) {
      const markup = await areaPage(kind, area.slug);
      // The content column, not the header, footer and sidebar.
      const main = markup.slice(markup.indexOf("<main"), markup.indexOf("</main>"));
      const words = textOf(main).split(" ").length;
      if (words > budget) long.push(`${area.slug}: ${words} words`);
    }
    expect(long, `split these areas in two (a new area in the data, no new page file), over ${budget} words`).toEqual([]);
  });
});

describe.each([
  { name: "tools", Page: ToolsPage, reference: toolsReference, path: TOOLS_BASE },
  { name: "api", Page: ApiReferencePage, reference: apiReference, path: API_BASE },
])("the $name index", ({ Page, reference, path }) => {
  const render = () => {
    nav.pathname = path;
    return html(createElement(Page));
  };

  it("lists every area as a link with its summary and count", () => {
    const markup = render();
    for (const area of reference.areas) {
      expect(markup, area.slug).toContain(`href="${path}/${area.slug}"`);
      expect(textOf(markup)).toContain(area.summary);
      expect(textOf(markup)).toContain(area.countLabel);
    }
  });

  it("links every name to its place on its area's page, so it reads without scripts", () => {
    const markup = render();
    for (const { entry, area } of entriesAtoZ(reference.areas)) {
      expect(markup, `${entry.id}`).toContain(`href="${path}/${area.slug}#${entry.id}"`);
    }
  });

  it("is short: it holds the list and not the entries", () => {
    expect(textOf(render()).split(" ").length).toBeLessThan(700);
  });

  it("carries the redirect for old hashes", () => {
    // Rendered by the client component, which renders nothing; the page
    // passing it the right map is what is checked, through its props.
    const element = Page() as { props: { children: unknown } };
    const found: { base: string; targets: Record<string, string> }[] = [];
    const walk = (node: unknown): void => {
      if (Array.isArray(node)) return node.forEach(walk);
      if (node && typeof node === "object" && "props" in node) {
        const n = node as { type: unknown; props: { base?: string; targets?: Record<string, string>; children?: unknown } };
        if (n.type === HashRedirect) found.push({ base: n.props.base!, targets: n.props.targets! });
        walk(n.props.children);
      }
    };
    walk(element);
    expect(found).toHaveLength(1);
    expect(found[0].base).toBe(path);
    expect(Object.keys(found[0].targets).length).toBeGreaterThan(reference.areas.length);
  });
});

describe("the counts", () => {
  it("say how many tools there are, from the data", () => {
    const n = toolAreas.reduce((m, a) => m + a.sections.reduce((k, s) => k + s.tools.length, 0), 0);
    expect(toolCount).toBe(n);
    nav.pathname = TOOLS_BASE;
    expect(textOf(html(createElement(ToolsPage)))).toContain(`All ${n} tools`);
    expect(toolsReference.areas.map((a) => a.countLabel).join(" ")).toMatch(/\d+ tools?/);
  });
});
