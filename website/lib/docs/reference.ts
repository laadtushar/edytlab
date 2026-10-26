/**
 * What the two long reference pages (the Audio Tools Reference and the
 * API Reference) share.
 *
 * Each is one index page plus one page per area, all made from a single
 * data array (`tools-data.ts`, `api-data.ts`). Everything the index, the
 * "jump to" lists, the previous/next links and the old-link redirect need
 * is read from an `AreaOutline`, which each data module derives from its
 * own array. Nothing here knows what a tool or a command is.
 *
 * An area is a page: long enough to be worth a page of its own, short
 * enough to read on a phone (about 8,000 px at 390 px wide, at most).
 * A section is an `h2` inside an area; an area made of one section shows
 * no `h2`, because it would repeat the page's heading.
 */

export interface ReferenceArea<Section> {
  /** The last segment of the page's path: `/docs/tools/<slug>`. */
  slug: string;
  title: string;
  /** One sentence: the card on the index, the page's lead, its meta description. */
  summary: string;
  sections: Section[];
}

/** One name a reader can jump to: a tool, a command or an event. */
export interface OutlineEntry {
  /** The element id on the area's page, and the hash old links used. */
  id: string;
  /** What the link says: the bare name, without arguments. */
  label: string;
}

export interface OutlineSection {
  id: string;
  title: string;
  entries: OutlineEntry[];
}

export interface AreaOutline {
  slug: string;
  title: string;
  summary: string;
  /** The card's count, as words: how many tools or commands the area has. */
  countLabel: string;
  sections: OutlineSection[];
}

/** A reference: where it lives and the outline of each of its areas. */
export interface Reference {
  /** The index page's path: `/docs/tools`. */
  base: string;
  /** What the index is called, for a breadcrumb and a back link. */
  name: string;
  areas: AreaOutline[];
}

/** `Rendering and A/B` -> `rendering-and-a-b`. Section ids are made this way. */
export function slugify(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

/** The name before its arguments: `setTrackGain(track, db)` -> `setTrackGain`. */
export function bareName(signature: string): string {
  const open = signature.indexOf("(");
  return open < 0 ? signature : signature.slice(0, open);
}

export function areaPath(base: string, slug: string): string {
  return `${base}/${slug}`;
}

/** The address of one entry: its area's page with the entry's id as the hash. */
export function entryHref(base: string, slug: string, id: string): string {
  return `${areaPath(base, slug)}#${id}`;
}

export function areaByPath(reference: Reference, slug: string): AreaOutline | undefined {
  return reference.areas.find((a) => a.slug === slug);
}

/** The areas either side of one, in the order the index lists them. */
export function neighbours(
  areas: readonly AreaOutline[],
  slug: string,
): { prev?: AreaOutline; next?: AreaOutline } {
  const i = areas.findIndex((a) => a.slug === slug);
  if (i < 0) return {};
  return { prev: areas[i - 1], next: areas[i + 1] };
}

/** Every entry of a reference with the area it is on, A to Z by label. */
export function entriesAtoZ(
  areas: readonly AreaOutline[],
): { entry: OutlineEntry; area: AreaOutline }[] {
  return areas
    .flatMap((area) => area.sections.flatMap((s) => s.entries.map((entry) => ({ entry, area }))))
    .sort((a, b) => a.entry.label.toLowerCase().localeCompare(b.entry.label.toLowerCase()));
}

/**
 * Where an old `#hash` on the index page now lives, as a path under the
 * index: `{ cut_range: "region-editing#cut_range", "region-editing":
 * "region-editing" }`.
 *
 * The index used to be one long page, so a link to it could carry a hash
 * for anything on it. Every id a child page defines (an entry, a section)
 * maps to `<area>#<id>`; an area's own slug maps to the area when no id
 * has that name; `aliases` map a name that is neither (a heading the old
 * page had) to an area slug. The map is small and plain on purpose: it is
 * passed to the one client component that reads the hash.
 */
export function hashTargets(
  areas: readonly AreaOutline[],
  aliases: Readonly<Record<string, string>> = {},
): Record<string, string> {
  const targets: Record<string, string> = {};
  for (const area of areas) {
    for (const section of area.sections) {
      targets[section.id] = `${area.slug}#${section.id}`;
      for (const entry of section.entries) targets[entry.id] = `${area.slug}#${entry.id}`;
    }
  }
  for (const area of areas) targets[area.slug] ??= area.slug;
  for (const [alias, slug] of Object.entries(aliases)) targets[alias] ??= slug;
  return targets;
}

/**
 * The id `hashTargets` resolves, or undefined. Own properties only, so a
 * hash like `#constructor` or `#__proto__` is not a lookup into
 * `Object.prototype`.
 */
export function resolveHash(
  hash: string,
  targets: Readonly<Record<string, string>>,
): string | undefined {
  let id = hash.replace(/^#/, "");
  try {
    id = decodeURIComponent(id);
  } catch {
    return undefined;
  }
  return id !== "" && Object.prototype.hasOwnProperty.call(targets, id) ? targets[id] : undefined;
}

/** Every id a reference defines on its pages, with where it was found. */
export function allIds(areas: readonly AreaOutline[]): { id: string; at: string }[] {
  return areas.flatMap((area) =>
    area.sections.flatMap((section) => [
      { id: section.id, at: `${area.slug} (section ${section.title})` },
      ...section.entries.map((e) => ({ id: e.id, at: `${area.slug} (${e.label})` })),
    ]),
  );
}
