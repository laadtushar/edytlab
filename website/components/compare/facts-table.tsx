import Link from "next/link";
import type { ReactNode } from "react";

import { displayDate, hostAndPath } from "@/lib/competitors";

/** A source for edytlab's side of a row: a page of this site or of the repository. */
export interface OursSource {
  href: string;
  label: string;
}

export interface FactRow {
  label: string;
  /** What edytlab does, in words a reader can check against the app or its docs. */
  ours: ReactNode;
  oursSource?: OursSource;
  /** What the other product's own page says. */
  theirs: ReactNode;
  /** That page. Its host and path are the link text unless `label` says otherwise. */
  source: { href: string; label?: string };
  /** The day `theirs` was read there, `YYYY-MM-DD`. */
  checkedOn: string;
}

/**
 * Facts about edytlab and one other product, side by side, each row
 * carrying the link to the page that states the other product's side and
 * the day it was read. A fact with no source and no date has no row here:
 * the type does not allow one.
 *
 * Phones get each row as a block (the fact, then edytlab's side, then the
 * other product's, each labelled), because three columns of prose do not
 * fit in 360 px and a table that scrolls sideways hides its last column.
 * From `sm` up it is an ordinary table. A server component: no script.
 */
export function FactsTable({
  theirName,
  rows,
  caption = `Facts about edytlab and ${theirName}`,
}: {
  theirName: string;
  rows: FactRow[];
  caption?: string;
}) {
  const label =
    "before:mb-1 before:block before:text-[11px] before:font-medium before:uppercase before:tracking-wider before:text-muted-foreground before:content-[attr(data-label)] sm:before:hidden";
  return (
    <table className="mt-6 block w-full border-collapse text-sm sm:table sm:table-fixed">
      <caption className="sr-only">{caption}</caption>
      <thead className="sr-only sm:not-sr-only sm:table-header-group">
        <tr className="sm:table-row">
          <th scope="col" className="w-[22%] border-b border-border px-0 py-2 pr-3 text-left font-semibold">
            Fact
          </th>
          <th scope="col" className="w-[39%] border-b border-border px-3 py-2 text-left font-semibold">
            edytlab
          </th>
          <th scope="col" className="w-[39%] border-b border-border px-3 py-2 text-left font-semibold">
            {theirName}
          </th>
        </tr>
      </thead>
      <tbody className="block sm:table-row-group">
        {rows.map((row) => (
          <tr key={row.label} className="block border-b border-border/60 py-4 sm:table-row sm:py-0">
            <th
              scope="row"
              className="block px-0 pb-2 text-left text-base font-semibold sm:table-cell sm:py-3 sm:pr-3 sm:align-top sm:text-sm"
            >
              {row.label}
            </th>
            <td data-label="edytlab" className={`block px-0 py-1 sm:table-cell sm:px-3 sm:py-3 sm:align-top ${label}`}>
              <div>{row.ours}</div>
              {row.oursSource ? (
                <div className="mt-1 text-xs text-muted-foreground">
                  <Link href={row.oursSource.href}>{row.oursSource.label}</Link>
                </div>
              ) : null}
            </td>
            <td data-label={theirName} className={`block px-0 py-1 sm:table-cell sm:px-3 sm:py-3 sm:align-top ${label}`}>
              <div>{row.theirs}</div>
              <div className="mt-1 text-xs text-muted-foreground [overflow-wrap:anywhere]">
                <a href={row.source.href} target="_blank" rel="noopener noreferrer">
                  {row.source.label ?? hostAndPath(row.source.href)}
                </a>
                {" · checked "}
                <time dateTime={row.checkedOn}>{displayDate(row.checkedOn)}</time>
              </div>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
