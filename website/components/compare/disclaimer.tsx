import Link from "next/link";

import { displayDate } from "@/lib/competitors";
import { siteConfig } from "@/lib/site";

/**
 * The disclaimer every comparison page ends with (research report 7.3):
 * no affiliation, names used only to identify, every statement from the
 * product's own pages and dated, and a way to report a mistake.
 */
export function Disclaimer({
  competitor,
  checkedOn,
  plural = false,
}: {
  competitor: string;
  checkedOn: string;
  /** The page compares several products (the hub): the wording then covers all of them. */
  plural?: boolean;
}) {
  const body = plural ? (
    <>
      edytlab is an independent open-source project. It is not affiliated with, endorsed by or sponsored by any of the other
      products on this page or their owners. Their names, and other product names, are trademarks of their respective owners
      and are used here only to identify the products being compared. Everything stated about {competitor} comes from their
      own pages, linked above, and was last checked on {displayDate(checkedOn)}. Features and prices change; check each
      product&apos;s site before you decide. Found a mistake?
    </>
  ) : (
    <>
      edytlab is an independent open-source project. It is not affiliated with, endorsed by or sponsored by {competitor} or its
      owner. {competitor} and other product names are trademarks of their respective owners and are used here only to identify
      the products being compared. Everything stated about {competitor} comes from its own pages, linked above, and was last
      checked on {displayDate(checkedOn)}. Features and prices change; check {competitor}&apos;s site before you decide. Found
      a mistake?
    </>
  );
  return (
    <div className="mt-10 rounded-xl border border-border/60 bg-secondary/20 p-5 text-sm leading-relaxed text-muted-foreground">
      <div className="font-semibold text-foreground">About this comparison</div>
      <div className="mt-2">
        {body}{" "}
        <a href={`${siteConfig.github}/issues`} target="_blank" rel="noopener noreferrer">
          Tell us
        </a>{" "}
        and we will correct it (<Link href="/about#corrections">how corrections work</Link>,{" "}
        <Link href="/about#how-we-compare">how we compare</Link>).
      </div>
    </div>
  );
}
