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

/**
 * Where the next match of `pattern` (a global one) is at or after a position,
 * or -1. Asked about growing positions only: the answer is looked up again
 * once the position has passed it, so a line of unclosed openers is read
 * once, not once from each.
 */
function nextOf(source: string, pattern: RegExp): (from: number) => number {
  let found = -2;
  return (from) => {
    if (found === -1 || found >= from) return found;
    pattern.lastIndex = from;
    found = pattern.exec(source)?.index ?? -1;
    return found;
  };
}

/**
 * The tokens are `[text](href)`, `` `code` `` and `**bold**`, found
 * left to right, the first that fits at each place. This is what the
 * regular expression `\[([^\]]+)\]\(([^)\s]+)\)|`([^`]+)`|\*\*([^*]+)\*\*`
 * matched, written out because that one read to the end of the text from
 * every opener that never closed (quadratic on a line of them); the
 * expression's behaviour is pinned in `inline.test.ts`.
 */
export function parseInline(source: string): Inline[] {
  const out: Inline[] = [];
  const nextBracket = nextOf(source, /]/g);
  const nextTick = nextOf(source, /`/g);
  const nextStar = nextOf(source, /\*/g);
  const nextHrefEnd = nextOf(source, /[)\s]/g);
  let last = 0;
  let failedLink = -1; // a `]` after which no `(href)` follows: the same for every `[` before it
  let at = 0;
  while (at < source.length) {
    let end = -1; // where the token at `at` ends, if one does
    let token: Inline | undefined;
    const c = source[at];
    if (c === "[") {
      const close = nextBracket(at + 1);
      if (close > at + 1 && close !== failedLink) {
        if (source[close + 1] === "(") {
          const stop = nextHrefEnd(close + 2);
          if (stop > close + 2 && source[stop] === ")") {
            token = { type: "link", text: source.slice(at + 1, close), href: source.slice(close + 2, stop) };
            end = stop + 1;
          }
        }
        if (!token) failedLink = close;
      }
    } else if (c === "`") {
      const close = nextTick(at + 1);
      if (close > at + 1) {
        token = { type: "code", text: source.slice(at + 1, close) };
        end = close + 1;
      }
    } else if (c === "*" && source[at + 1] === "*") {
      const close = nextStar(at + 2);
      if (close > at + 2 && source[close + 1] === "*") {
        token = { type: "bold", text: source.slice(at + 2, close) };
        end = close + 2;
      }
    }
    if (token) {
      if (at > last) out.push({ type: "text", text: source.slice(last, at) });
      out.push(token);
      last = at = end;
    } else {
      at++;
    }
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
