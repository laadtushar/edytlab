import { apiAreas, type ApiArea, type ApiSection } from "./api-data";
import {
  bareName,
  slugify,
  type AreaOutline,
  type OutlineSection,
  type Reference,
} from "./reference";
import { toolAreas, type ToolArea } from "./tools-data";

/**
 * The two references as `Reference`s: the outline of each area, derived
 * from the data arrays so that the index, the "jump to" lists, the
 * pager, the redirect map and the sitemap all read the same names.
 */

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

export const TOOLS_BASE = "/docs/tools";
export const API_BASE = "/docs/api-reference";

function toolOutline(area: ToolArea): AreaOutline {
  const sections: OutlineSection[] = area.sections.map((s) => ({
    id: slugify(s.title),
    title: s.title,
    entries: s.tools.map((t) => ({ id: t.name, label: t.name })),
  }));
  const count = sections.reduce((n, s) => n + s.entries.length, 0);
  return {
    slug: area.slug,
    title: area.title,
    summary: area.summary,
    countLabel: plural(count, "tool", "tools"),
    sections,
  };
}

function apiSectionOutline(s: ApiSection): OutlineSection {
  switch (s.kind) {
    case "commands":
    case "events":
      return {
        id: slugify(s.title),
        title: s.title,
        entries: s.items.map((i) => ({ id: bareName(i.name), label: bareName(i.name) })),
      };
    case "types":
      return { id: slugify(s.title), title: s.title, entries: [] };
  }
}

function apiOutline(area: ApiArea): AreaOutline {
  const sections = area.sections.map(apiSectionOutline);
  const commands = area.sections.reduce((n, s) => n + (s.kind === "commands" ? s.items.length : 0), 0);
  const events = area.sections.reduce((n, s) => n + (s.kind === "events" ? s.items.length : 0), 0);
  const parts = [
    ...(commands > 0 ? [plural(commands, "command", "commands")] : []),
    ...(events > 0 ? [plural(events, "event", "events")] : []),
    ...(area.sections.some((s) => s.kind === "types") ? ["the bridge types"] : []),
  ];
  return {
    slug: area.slug,
    title: area.title,
    summary: area.summary,
    countLabel: parts.join(" and "),
    sections,
  };
}

export const toolsReference: Reference = {
  base: TOOLS_BASE,
  name: "Audio Tools Reference",
  areas: toolAreas.map(toolOutline),
};

export const apiReference: Reference = {
  base: API_BASE,
  name: "API Reference",
  areas: apiAreas.map(apiOutline),
};

/**
 * Names the old single page used for something and the area that now has
 * it, for `hashTargets`. The tools page has none: its headings are the
 * category titles, which are section ids.
 */
export const API_ALIASES: Readonly<Record<string, string>> = {
  commands: apiAreas[0].slug,
};

export const TOOLS_ALIASES: Readonly<Record<string, string>> = {};

/** How many tools the reference documents: the number the site quotes. */
export const toolCount = toolAreas.reduce(
  (n, area) => n + area.sections.reduce((m, s) => m + s.tools.length, 0),
  0,
);
