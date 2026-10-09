/**
 * The small inline markup blog text can carry, so a post can link to
 * the docs and set a prompt or a tool name in code type without every
 * block becoming a bag of nodes:
 *
 * - `[label](href)` — a link; `/…` and `#…` hrefs stay on the site,
 *   anything else opens as an external link;
 * - `` `code` `` — monospace;
 * - `**bold**` — emphasis.
 *
 * Parsed by one function so the page, the RSS feed and the tests agree
 * on what a string means. Text with none of it passes through as a
 * single `text` token, which is every paragraph written before this
 * existed.
 */

export type Inline =
  | { type: "text"; text: string }
  | { type: "link"; text: string; href: string }
  | { type: "code"; text: string }
  | { type: "bold"; text: string };

const TOKEN = /\[([^\]]+)\]\(([^)\s]+)\)|`([^`]+)`|\*\*([^*]+)\*\*/g;

export function parseInline(source: string): Inline[] {
  const out: Inline[] = [];
  let last = 0;
  for (const m of source.matchAll(TOKEN)) {
    const at = m.index ?? 0;
    if (at > last) out.push({ type: "text", text: source.slice(last, at) });
    if (m[1] !== undefined) out.push({ type: "link", text: m[1], href: m[2] });
    else if (m[3] !== undefined) out.push({ type: "code", text: m[3] });
    else out.push({ type: "bold", text: m[4] });
    last = at + m[0].length;
  }
  if (last < source.length) out.push({ type: "text", text: source.slice(last) });
  return out;
}

/** The text with the markup removed — for word counts and plain-text summaries. */
export function stripInline(source: string): string {
  return parseInline(source)
    .map((t) => t.text)
    .join("");
}

/** True for a link that leaves the site. */
export function isExternal(href: string): boolean {
  return /^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith("//");
}

/** Every href a piece of text links to. */
export function linksIn(source: string): string[] {
  return parseInline(source).flatMap((t) => (t.type === "link" ? [t.href] : []));
}
