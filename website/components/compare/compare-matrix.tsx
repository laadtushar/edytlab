import Link from "next/link";

import { hostAndPath } from "@/lib/competitors";
import { COLUMNS, isExternal, matrix, shortDate, type Cell } from "@/lib/compare-matrix";

/**
 * The comparison hub's one matrix: a row for each product, a column for
 * each question, and in every cell what the product's own page says with
 * a link to that page and the day it was read.
 *
 * edytlab's row is the last of the rows and is drawn exactly as the
 * others are (same columns, same type, no highlight). Its cells link
 * edytlab's own docs, which have no "read on" day.
 *
 * Layout. From `xl` up it is an ordinary table with the product in the
 * first column. Below that each product is a card, with its seven
 * answers in a grid of labelled cells, because seven columns of prose
 * fit in no phone and a table that scrolls sideways hides its last
 * columns. The table's roles are written out, since a browser may drop
 * the table semantics from an element that is drawn as a block.
 * A server component: no script.
 */
const LINK = "underline decoration-muted-foreground/50 underline-offset-2 hover:text-foreground hover:decoration-foreground";

function CellView({ label, cell }: { label: string; cell: Cell }) {
  const external = isExternal(cell.href);
  const link = external ? (
    <a href={cell.href} target="_blank" rel="noopener noreferrer" className={LINK}>
      {cell.label ?? hostAndPath(cell.href)}
    </a>
  ) : (
    <Link href={cell.href} className={LINK}>
      {cell.label ?? cell.href}
    </Link>
  );
  return (
    <div
      role="cell"
      data-label={label}
      className="block px-0 py-2 text-sm before:mb-1 before:block before:text-[11px] before:font-medium before:uppercase before:tracking-wider before:text-muted-foreground before:content-[attr(data-label)] xl:table-cell xl:px-2 xl:py-3 xl:align-top xl:text-xs xl:before:hidden"
    >
      <div className={cell.absent ? "text-muted-foreground" : undefined}>{cell.text}</div>
      <div className="mt-1 text-xs text-muted-foreground [overflow-wrap:anywhere]">
        {link}
        {cell.checkedOn ? (
          <>
            {" · read "}
            <time dateTime={cell.checkedOn}>{shortDate(cell.checkedOn)}</time>
          </>
        ) : null}
      </div>
    </div>
  );
}

export function CompareMatrix() {
  return (
    <div
      role="table"
      aria-label="AI audio editors compared: where each runs, its license, price model, chat or agent control, stem separation, transcription and platforms"
      className="mt-6 block w-full text-sm xl:table xl:table-fixed xl:border-collapse"
    >
      <div role="rowgroup" className="sr-only xl:not-sr-only xl:table-header-group">
        <div role="row" className="xl:table-row">
          <div role="columnheader" className="xl:table-cell xl:w-[11%] xl:border-b xl:border-border xl:py-2 xl:pr-2 xl:text-left xl:text-xs xl:font-semibold">
            Product
          </div>
          {COLUMNS.map((c) => (
            <div
              key={c.key}
              role="columnheader"
              className="xl:table-cell xl:border-b xl:border-border xl:px-2 xl:py-2 xl:text-left xl:text-xs xl:font-semibold"
            >
              {c.label}
            </div>
          ))}
        </div>
      </div>
      <div role="rowgroup" className="grid gap-4 xl:table-row-group">
        {matrix.map((row) => (
          <div
            key={row.id}
            role="row"
            className="block rounded-xl border border-border/60 p-4 sm:grid sm:grid-cols-2 sm:gap-x-6 xl:table-row xl:rounded-none xl:border-0 xl:border-b xl:border-border/60 xl:p-0"
          >
            <div
              role="rowheader"
              className="pb-2 sm:col-span-2 xl:table-cell xl:py-3 xl:pr-2 xl:align-top"
            >
              <a href={`#${row.id}`} className="text-base font-semibold text-foreground xl:text-sm">
                {row.name}
              </a>
              {row.scope ? <div className="text-xs text-muted-foreground">{row.scope}</div> : null}
            </div>
            {COLUMNS.map((c) => (
              <CellView key={c.key} label={c.label} cell={row.cells[c.key]} />
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}
