import { displayDate, hostAndPath } from "@/lib/competitors";

export interface Source {
  label: string;
  /** The product's own page. */
  href: string;
  /** The day it was read, `YYYY-MM-DD`. */
  checkedOn: string;
}

/**
 * Every page a comparison leans on, once each, with the day it was read.
 * It is the list a reader (or the person who re-checks the page) works
 * down; the facts above carry the same links beside them.
 */
export function SourcesList({
  sources,
  heading = "Sources",
}: {
  /** Kept for callers; the disclaimer below the list says whose pages these are. */
  competitor?: string;
  sources: Source[];
  heading?: string;
}) {
  return (
    <section aria-labelledby="sources">
      <h2 id="sources">{heading}</h2>
      <ol className="!mt-4 list-decimal space-y-2 pl-6 text-sm">
        {sources.map((s) => (
          <li key={s.href}>
            <a href={s.href} target="_blank" rel="noopener noreferrer">
              {s.label}
            </a>
            <span className="text-muted-foreground [overflow-wrap:anywhere]">
              {" "}
              ({hostAndPath(s.href)}), checked <time dateTime={s.checkedOn}>{displayDate(s.checkedOn)}</time>
            </span>
          </li>
        ))}
      </ol>
    </section>
  );
}
