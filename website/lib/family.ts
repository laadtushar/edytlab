/**
 * The LabyNator family: the studio, its apps and its founder, as the
 * footer and the structured data name them.
 *
 * The source of truth is labynator's `src/lib/apps.ts` (laadtushar/labynator,
 * on `master`). Names, blurbs and order come from there; each URL is the
 * canonical one, the host that actually serves the page, so a link is
 * never sent through a redirect. When an app moves or is renamed, change
 * it there first and then here, in this one file.
 */

import { siteConfig } from "./site";

/** The studio behind the apps. Also the `publisher` in the structured data. */
export const LABYNATOR = {
  name: "LabyNator",
  url: "https://www.labynator.com",
} as const;

/** The founder. Also the `author` in the structured data. */
export const FOUNDER = {
  name: "Tushar Laad",
  url: "https://www.tusharlaad.com",
} as const;

export interface FamilyApp {
  name: string;
  /** The one-line description labynator gives it. */
  blurb: string;
  url: string;
}

/**
 * Every app in the family, in the order every site lists them. This site
 * is in the list, so the list is the same everywhere; `otherApps` leaves
 * it out for the footer.
 */
export const familyApps: readonly FamilyApp[] = [
  { name: "MemryLab", blurb: "Local AI Memory", url: "https://www.memrylab.com" },
  { name: "XpenseLab", blurb: "Expense tracking", url: "https://xpenselab.com" },
  { name: "HyredLab", blurb: "Job tracking", url: "https://www.hyredlab.com" },
  { name: siteConfig.name, blurb: "AI audio editor", url: siteConfig.url },
];

/** The family without this site, as the footer shows it. */
export const otherApps: readonly FamilyApp[] = familyApps.filter(
  (app) => app.url !== siteConfig.url,
);
