/**
 * The links in the top bar, in the order they read left to right, and
 * the same list in the phone menu.
 *
 * Five, on purpose: the bar sits between the logo and two buttons, and a
 * ninth link overflowed it (and was dropped below `lg` link by link to
 * make room). The home page's other sections (Demos, Tools, Interface,
 * FAQ) are reached by scrolling it, and every page, those anchors
 * included, is in the footer, which is the full index. The header is the
 * short path.
 *
 * A `/#id` href is a section of the home page: the bar marks it while the
 * reader is in that section. Any other href is a route: the bar marks it
 * while the reader is on that page or under it.
 */
export const navLinks: readonly { href: string; label: string }[] = [
  { href: "/#features", label: "Features" },
  { href: "/videos", label: "Videos" },
  { href: "/docs", label: "Docs" },
  { href: "/blog", label: "Blog" },
  { href: "/compare", label: "Compare" },
];

/** The section id a home-page link points at, if it is one. */
export function sectionId(href: string) {
  return href.startsWith("/#") ? href.slice(2) : undefined;
}

/**
 * How a route link relates to the page being read: `"page"` when it is
 * this page, `"true"` when this page is under it (`/blog/<post>` under
 * `/blog`), and `undefined` otherwise, so the attribute is left off.
 * Home-page anchors are never "current" by route.
 */
export function currentFor(href: string, pathname: string | null): "page" | "true" | undefined {
  if (!pathname || sectionId(href) || href === "/") return undefined;
  if (pathname === href || pathname === `${href}/`) return "page";
  return pathname.startsWith(`${href}/`) ? "true" : undefined;
}
