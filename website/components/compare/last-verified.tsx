import Link from "next/link";

import { displayDate } from "@/lib/competitors";
import { FOUNDER } from "@/lib/family";

/**
 * The byline and the day the page's facts were last read at their
 * sources. The date is the newest `checkedOn` the page leans on, never
 * the day it was built: moving it without reading the sources again is
 * the "changing dates to look fresh" that Google's guidance warns about.
 */
export function LastVerified({ checkedOn }: { checkedOn: string }) {
  return (
    <div className="mt-6 text-sm text-muted-foreground">
      By{" "}
      <a href={FOUNDER.url} rel="author" className="underline underline-offset-4 hover:text-foreground">
        {FOUNDER.name}
      </a>
      {" · "}Prices and features checked on <time dateTime={checkedOn}>{displayDate(checkedOn)}</time>
      {" · "}
      <Link href="/about#how-we-compare" className="underline underline-offset-4 hover:text-foreground">
        How we compare
      </Link>
    </div>
  );
}
