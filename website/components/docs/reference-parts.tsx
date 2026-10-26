import Link from "next/link";
import { ArrowLeft, ArrowRight } from "lucide-react";

import type { ApiCommand, ApiEvent } from "@/lib/docs/api-data";
import {
  areaPath,
  bareName,
  entriesAtoZ,
  entryHref,
  neighbours,
  slugify,
  type AreaOutline,
  type Reference,
} from "@/lib/docs/reference";
import type { Tool } from "@/lib/docs/tools-data";

/**
 * The pieces the two reference indexes and their area pages are made of.
 * Server components all of them: links and markup, no scripts, so a page
 * reads the same with JavaScript off.
 */

const MONO_LINK =
  "inline-block rounded-md border border-border/60 bg-card/40 px-2.5 py-1.5 font-mono text-xs text-foreground/90 transition-colors hover:border-primary/40 hover:text-primary";

/** The areas as cards: the title, one line, and how much is on the page. */
export function AreaCards({ reference }: { reference: Reference }) {
  return (
    <div className="not-prose grid gap-3 sm:grid-cols-2" data-cascade-items>
      {reference.areas.map((area) => (
        <Link
          key={area.slug}
          href={areaPath(reference.base, area.slug)}
          data-lift
          className="ring-hover group flex flex-col gap-1.5 rounded-xl border border-border/60 bg-card/40 p-4 hover:bg-card"
        >
          <span className="font-semibold transition-colors group-hover:text-primary">{area.title}</span>
          <span className="text-sm leading-relaxed text-muted-foreground">{area.summary}</span>
          <span className="mt-auto pt-1 font-mono text-xs text-primary">{area.countLabel}</span>
        </Link>
      ))}
    </div>
  );
}

/**
 * Every name in a reference, A to Z, each a link to its place on its
 * area's page. It is the index's jump list, and it is why a page without
 * scripts can still reach any name.
 */
export function NameIndex({
  reference,
  filter,
}: {
  reference: Reference;
  /** Keep only the entries of these sections (the API's events, say). */
  filter?: (sectionTitle: string) => boolean;
}) {
  const areas = filter
    ? reference.areas.map((a) => ({ ...a, sections: a.sections.filter((s) => filter(s.title)) }))
    : reference.areas;
  return (
    <ul className="not-prose flex list-none flex-wrap gap-1.5 pl-0">
      {entriesAtoZ(areas).map(({ entry, area }) => (
        <li key={`${area.slug}/${entry.id}`} className="mt-0">
          <Link href={entryHref(reference.base, area.slug, entry.id)} className={MONO_LINK}>
            {entry.label}
          </Link>
        </li>
      ))}
    </ul>
  );
}

/** What an area page lists, as one flat list of links for the two "jump to" forms. */
function OutlineLinks({
  reference,
  area,
  variant,
}: {
  reference: Reference;
  area: AreaOutline;
  variant: "list" | "chips";
}) {
  const multiple = area.sections.length > 1;
  return (
    <div className="space-y-4">
      {area.sections.map((section) => (
        <div key={section.id}>
          {multiple ? (
            <p className="mb-1.5 text-xs font-semibold uppercase tracking-widest text-muted-foreground">
              <a href={`#${section.id}`} className="hover:text-foreground">
                {section.title}
              </a>
            </p>
          ) : null}
          {variant === "chips" ? (
            <ul className="flex list-none flex-wrap gap-1.5 pl-0">
              {section.entries.map((e) => (
                <li key={e.id} className="mt-0">
                  <a href={`#${e.id}`} className={MONO_LINK}>
                    {e.label}
                  </a>
                </li>
              ))}
            </ul>
          ) : (
            <ul className="list-none space-y-0.5 pl-0">
              {section.entries.map((e) => (
                <li key={e.id} className="mt-0">
                  <a
                    href={`#${e.id}`}
                    className="block break-words rounded px-2 py-1 font-mono text-xs text-muted-foreground transition-colors hover:bg-secondary/60 hover:text-foreground"
                  >
                    {e.label}
                  </a>
                </li>
              ))}
            </ul>
          )}
        </div>
      ))}
    </div>
  );
}

function hasEntries(area: AreaOutline): boolean {
  return area.sections.some((s) => s.entries.length > 0);
}

/**
 * "On this page" below the wide breakpoint: a disclosure, closed, so a
 * phone sees one line and opens the list only when it wants a name. It is
 * a `<details>`, so it works without scripts. From `xl` up the same links
 * are in the sticky column (`AreaOutlineAside`) and this is hidden.
 */
export function AreaJump({ reference, area }: { reference: Reference; area: AreaOutline }) {
  if (!hasEntries(area)) return null;
  return (
    <details className="not-prose mb-6 rounded-lg border border-border/60 bg-card/40 xl:hidden [&[open]>summary]:border-b [&[open]>summary]:border-border/60">
      <summary className="cursor-pointer select-none px-4 py-3 text-sm font-medium">
        On this page
        <span className="ml-2 font-mono text-xs font-normal text-muted-foreground">{area.countLabel}</span>
      </summary>
      <div className="px-4 py-4">
        <OutlineLinks reference={reference} area={area} variant="chips" />
      </div>
    </details>
  );
}

/** The sticky "On this page" column of the wide layout, given to `DocShell`'s `aside`. */
export function AreaOutlineAside({ reference, area }: { reference: Reference; area: AreaOutline }) {
  if (!hasEntries(area)) return null;
  return (
    <nav aria-label="On this page">
      <p className="mb-3 text-xs font-semibold uppercase tracking-widest text-muted-foreground">On this page</p>
      <OutlineLinks reference={reference} area={area} variant="list" />
    </nav>
  );
}

/** Previous and next area, and the way back to the index, at the foot of an area page. */
export function AreaPager({ reference, area }: { reference: Reference; area: AreaOutline }) {
  const { prev, next } = neighbours(reference.areas, area.slug);
  const card =
    "ring-hover group flex flex-col gap-1 rounded-xl border border-border/60 bg-card/40 p-4 hover:bg-card";
  return (
    <nav aria-label={`${reference.name} pages`} className="not-prose mt-12 border-t border-border/50 pt-6">
      <div className="grid gap-3 sm:grid-cols-2">
        {prev ? (
          <Link href={areaPath(reference.base, prev.slug)} rel="prev" className={card}>
            <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <ArrowLeft className="size-3.5" aria-hidden /> Previous
            </span>
            <span className="font-semibold transition-colors group-hover:text-primary">{prev.title}</span>
          </Link>
        ) : (
          <span className="hidden sm:block" />
        )}
        {next ? (
          <Link href={areaPath(reference.base, next.slug)} rel="next" className={`${card} sm:items-end sm:text-right`}>
            <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
              Next <ArrowRight className="size-3.5" aria-hidden />
            </span>
            <span className="font-semibold transition-colors group-hover:text-primary">{next.title}</span>
          </Link>
        ) : null}
      </div>
      <p className="mt-4 text-sm">
        <Link href={reference.base} className="text-primary underline underline-offset-4 hover:no-underline">
          All of the {reference.name}
        </Link>
      </p>
    </nav>
  );
}

/** Where an entry or a section lands when linked: room under the fixed header, and a mark. */
const ANCHOR = "scroll-mt-20 target:ring-2 target:ring-primary/60";

/**
 * The cards of one section of tools. An area with several sections gets
 * an `h2` per section; an area with one gets none (the page's heading
 * says it) and the section's id goes on the card list.
 */
export function ToolSection({
  title,
  tools,
  showHeading,
}: {
  title: string;
  tools: Tool[];
  showHeading: boolean;
}) {
  const id = slugify(title);
  return (
    <>
      {showHeading ? <h2 id={id} className="scroll-mt-20">{title}</h2> : null}
      {/* Flat — heading, cards, heading, cards — and the card list
          marked as items, so the page's cascade brings each tool in as
          it scrolls up rather than a whole section at once. */}
      <div id={showHeading ? undefined : id} className="not-prose scroll-mt-20 space-y-4" data-cascade-items>
        {tools.map((tool) => (
          <div
            key={tool.name}
            id={tool.name}
            className={`${ANCHOR} rounded-lg border border-border/50 bg-card/40 p-4`}
          >
            <div className="flex flex-wrap items-start gap-2">
              <code className="rounded bg-primary/10 px-2 py-0.5 font-mono text-sm text-primary">{tool.name}</code>
            </div>
            <p className="mt-2 text-sm leading-relaxed text-foreground/90">{tool.what}</p>
            <p className="mt-2 text-xs text-muted-foreground">
              <span className="font-medium text-foreground/70">Example prompt: </span>
              <code className="rounded bg-secondary px-1.5 py-0.5 font-mono">{tool.prompt}</code>
            </p>
            <p className="mt-1 text-xs text-muted-foreground">
              <span className="font-medium text-foreground/70">Returns: </span>
              {tool.output}
            </p>
            {tool.note ? (
              <p className="mt-2 rounded border border-primary/20 bg-primary/5 px-3 py-2 text-xs text-foreground/80">
                {tool.note}
              </p>
            ) : null}
          </div>
        ))}
      </div>
    </>
  );
}

// Below the sm breakpoint a row is a block (CSS only, the markup stays one
// table): the name as a monospace heading, what it returns under it, then the
// description as a paragraph. From sm up this is the ordinary table.
const stackedBase = [
  "max-sm:!block max-sm:!overflow-visible",
  "max-sm:[&_thead]:hidden max-sm:[&_tbody]:block max-sm:[&_tr]:block",
  "max-sm:[&_tr]:border-b max-sm:[&_tr]:border-border/50 max-sm:[&_tr]:py-3",
  "max-sm:[&_td]:block max-sm:[&_td]:border-0 max-sm:[&_td]:px-0 max-sm:[&_td]:py-0.5",
  "max-sm:[&_td:first-child]:text-base max-sm:[&_td:first-child]:font-semibold",
  "max-sm:[&_td:first-child_code]:break-words max-sm:[&_td:first-child_code]:bg-transparent max-sm:[&_td:first-child_code]:p-0 max-sm:[&_td:first-child_code]:font-mono",
  "max-sm:[&_td:nth-child(2)]:text-muted-foreground",
  "max-sm:[&_td:nth-child(2)_code]:break-words",
  "max-sm:[&_td:nth-child(3)]:mt-1 max-sm:[&_td:nth-child(3)]:leading-6",
].join(" ");
// The label before the second cell. Whole class names, so Tailwind sees them.
const stackedCommands = `${stackedBase} max-sm:[&_td:nth-child(2)]:before:content-['Returns_']`;
const stackedEvents = `${stackedBase} max-sm:[&_td:nth-child(2)]:before:content-['Payload_']`;

const ROW = `${ANCHOR} target:bg-primary/5`;

/** One section of commands as a table (rows, not a block, so the cascade brings them in one by one). */
export function CommandSection({
  title,
  items,
  showHeading,
}: {
  title: string;
  items: ApiCommand[];
  showHeading: boolean;
}) {
  const id = slugify(title);
  return (
    <>
      {showHeading ? <h2 id={id} className="scroll-mt-20">{title}</h2> : null}
      <table id={showHeading ? undefined : id} className={`${stackedCommands} scroll-mt-20`}>
        <thead>
          <tr>
            <th>Function</th>
            <th>Returns</th>
            <th>Description</th>
          </tr>
        </thead>
        <tbody>
          {items.map((cmd) => (
            <tr key={cmd.name} id={bareName(cmd.name)} className={ROW}>
              <td>
                <code>{cmd.name}</code>
              </td>
              <td>
                <code>{cmd.returns}</code>
              </td>
              <td>{cmd.desc}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </>
  );
}

export function EventSection({
  title,
  items,
  showHeading,
}: {
  title: string;
  items: ApiEvent[];
  showHeading: boolean;
}) {
  const id = slugify(title);
  return (
    <>
      {showHeading ? <h2 id={id} className="scroll-mt-20">{title}</h2> : null}
      <p id={showHeading ? undefined : id}>
        Subscribe with the unlisten pattern. Each handler returns an <code>UnlistenFn</code> — call it in{" "}
        <code>useEffect</code> cleanup.
      </p>
      <pre>
        <code>{`const unlisten = await bridge.onTextDelta((chunk) => {
  appendText(chunk);
});
// cleanup:
return () => { unlisten(); };`}</code>
      </pre>
      <table className={stackedEvents}>
        <thead>
          <tr>
            <th>Event</th>
            <th>Payload</th>
            <th>Description</th>
          </tr>
        </thead>
        <tbody>
          {items.map((ev) => (
            <tr key={ev.name} id={bareName(ev.name)} className={ROW}>
              <td>
                <code>{ev.name}</code>
              </td>
              <td>
                <code>{ev.payload}</code>
              </td>
              <td>{ev.desc}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </>
  );
}

export function TypesSection({
  title,
  code,
  showHeading,
}: {
  title: string;
  code: string;
  showHeading: boolean;
}) {
  const id = slugify(title);
  return (
    <>
      {showHeading ? <h2 id={id} className="scroll-mt-20">{title}</h2> : null}
      <pre id={showHeading ? undefined : id} className="scroll-mt-20">
        <code>{code}</code>
      </pre>
    </>
  );
}
