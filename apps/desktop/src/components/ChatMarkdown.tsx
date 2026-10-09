/**
 * ChatMarkdown — an assistant message's text, rendered as markdown (#440).
 *
 * Claude answers in markdown — `**bold**`, `- ` lists, headings, `code`,
 * fenced blocks — and the bubble used to print the symbols themselves.
 * Only the assistant's side goes through here; what the user typed is
 * shown as typed (see `MessageBubble`).
 *
 * What this renders is model output, and model output is not trusted: a
 * page the model read, or a file it was handed, can put text in a reply.
 * So the policy is the narrowest one that still reads as formatting:
 *
 *  - **No raw HTML.** `react-markdown` turns an HTML tag in the text into
 *    inert text, never an element. `rehype-raw` is what would change
 *    that, and it must not be added.
 *  - **Links do not navigate.** The webview *is* the app: following a
 *    link would replace the editor with a web page, and nothing in this
 *    app opens a browser for it (no opener plugin, no `shell:open`
 *    permission). So a link renders as its text with the address written
 *    out beside it, which is also what lets you see where a link claims
 *    to go. An `<a href>` is never emitted. Making them clickable means
 *    adding `tauri-plugin-opener` and a capability for it — a decision
 *    about the app's reach, not about formatting.
 *  - **No images.** The CSP already refuses remote ones; rendering the
 *    alt text instead means a reply cannot even ask for one.
 *
 * Streaming: the text is re-parsed from the start on every delta, so it
 * is always a prefix of the finished message. An unclosed `**` or a code
 * fence with no end is parsed as what it is so far rather than throwing.
 * Two things would still make the bubble jump while typing, and both are
 * handled here:
 *
 *  - the caret has to sit at the end of the last line of text, not drop
 *    below the last block, so it is put *inside* the tree (`remarkCaret`);
 *  - `Heading:` followed by a lone `-` is, to markdown, a heading with an
 *    underline until the next delta turns the dash into a list item, so a
 *    trailing underline-shaped line is held back while pending
 *    (`holdBackSetextUnderline`).
 */

import {
  Children,
  memo,
  type ComponentProps,
  type ReactNode,
} from "react";
import Markdown, {
  type Components,
  type ExtraProps,
  type Options,
} from "react-markdown";
import remarkGfm from "remark-gfm";

/**
 * The streaming caret. Shared with `MessageBubble`, which draws the same
 * bar after a user bubble's plain text. A drawn bar has no font
 * dependency and cannot clip.
 */
export const CARET_CLASS =
  "ml-0.5 inline-block h-[1.05em] w-[2px] translate-y-[0.18em] animate-pulse rounded-[1px] bg-[var(--accent)] align-baseline";

/** The slice of an mdast node the two plugins below touch. */
interface MdNode {
  type: string;
  value?: string;
  children?: MdNode[];
  data?: Record<string, unknown>;
}

/**
 * Markdown joins the lines of a paragraph with a space. A chat reply
 * that was `Done.\nSpeed is now 2x.` has always shown two lines, and a
 * model that writes one thing per line without blank lines between them
 * means it. So a single newline is a line break, as it is in chat.
 */
function remarkSoftBreaks() {
  return (tree: MdNode) => breakAtNewlines(tree);
}

function breakAtNewlines(node: MdNode): void {
  if (!node.children) return;
  const next: MdNode[] = [];
  for (const child of node.children) {
    if (child.type === "text" && child.value?.includes("\n")) {
      child.value.split("\n").forEach((part, i) => {
        if (i > 0) next.push({ type: "break" });
        if (part) next.push({ type: "text", value: part });
      });
    } else {
      breakAtNewlines(child);
      next.push(child);
    }
  }
  node.children = next;
}

/** A fresh node each time: the pipeline is free to annotate what it is given. */
function caretNode(): MdNode {
  return {
    type: "caret",
    // `data.h*` is how an mdast node says what element it becomes.
    data: {
      hName: "span",
      hProperties: {
        dataTestid: "caret",
        ariaHidden: "true",
        className: CARET_CLASS.split(" "),
      },
    },
  };
}

/** Containers whose last child is where the text actually ends. */
const CONTAINERS = new Set(["list", "listItem", "blockquote"]);
/** Blocks that hold running text, which the caret can follow. */
const TEXT_BLOCKS = new Set(["paragraph", "heading"]);

/**
 * Put the caret at the end of the last line of text.
 *
 * Appended as a sibling of the blocks it would sit on its own line below
 * them, and move up and down as the reply grew. Inside the last
 * paragraph (or list item, or heading) it trails the final word, as it
 * did when the bubble was one string. Where the message ends in a code
 * block or a table there is no line to trail, and it follows the block.
 */
function remarkCaret() {
  return (tree: MdNode) => {
    let host = tree;
    for (;;) {
      const last = host.children?.[host.children.length - 1];
      if (last && CONTAINERS.has(last.type)) {
        host = last;
        continue;
      }
      if (last && TEXT_BLOCKS.has(last.type)) host = last;
      break;
    }
    host.children = [...(host.children ?? []), caretNode()];
  };
}

/**
 * While streaming, drop a final line that is only `-` or `=` when it
 * directly follows a line of text. Markdown reads that as the underline
 * of a heading, so `Fade:` would flash up as a large bold heading for
 * the delta between the dash and the words after it. Once the reply is
 * finished nothing is dropped.
 */
function holdBackSetextUnderline(text: string): string {
  return text.replace(/([^\n])\n[ \t]{0,3}(?:-+|=+)[ \t]*$/, "$1");
}

/** What a link's text is, as far as it is plain text. */
function plainText(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(plainText).join("");
  return "";
}

/** `https://example.com/` and `example.com` name the same place. */
function bare(s: string): string {
  return s
    .trim()
    .replace(/^(?:https?:\/\/|mailto:)/i, "")
    .replace(/\/+$/, "")
    .toLowerCase();
}

const URL_CLASS = "font-mono text-[0.85em] text-[var(--text-dim)] break-all";

/**
 * A link, without the link. See the module comment for why.
 *
 * `href` has already been through react-markdown's URL transform, which
 * blanks anything that is not http(s), irc(s), mailto or xmpp — a
 * `javascript:` target arrives here as an empty string and shows just
 * its text.
 */
function Link({ href, children }: ComponentProps<"a"> & ExtraProps) {
  const url = href && href.trim() ? href : null;
  if (url && bare(plainText(children)) === bare(url)) {
    // `https://example.com` written out, or autolinked: say it once.
    return <span className={URL_CLASS}>{children}</span>;
  }
  return (
    <>
      {children}
      {url ? (
        <>
          {Children.count(children) > 0 ? " " : null}
          <span className={URL_CLASS}>({url})</span>
        </>
      ) : null}
    </>
  );
}

/** An image is never fetched; its description stands in for it. */
function Image({ alt }: ComponentProps<"img"> & ExtraProps) {
  return alt ? (
    <span className="italic text-[var(--text-dim)]">{alt}</span>
  ) : null;
}

/** A wide table scrolls on its own instead of widening the chat. */
function Table({ children }: ComponentProps<"table"> & ExtraProps) {
  return (
    <div className="chat-md-table">
      <table>{children}</table>
    </div>
  );
}

const COMPONENTS: Components = { a: Link, img: Image, table: Table };

const PLUGINS: NonNullable<Options["remarkPlugins"]> = [
  remarkGfm,
  remarkSoftBreaks,
];
const PLUGINS_WITH_CARET: NonNullable<Options["remarkPlugins"]> = [
  ...PLUGINS,
  remarkCaret,
];

export interface ChatMarkdownProps {
  text: string;
  /** Still streaming: show the caret, and hold back half-typed syntax. */
  pending?: boolean;
}

/**
 * Renders into its parent — give the parent the `chat-md` class (see
 * `styles.css`), which is what styles the elements.
 *
 * Memoised on `(text, pending)`: `Chat` re-renders on every streamed
 * delta, and without this every earlier message in the transcript would
 * be parsed again for each one.
 */
export const ChatMarkdown = memo(function ChatMarkdown({
  text,
  pending = false,
}: ChatMarkdownProps) {
  return (
    <Markdown
      remarkPlugins={pending ? PLUGINS_WITH_CARET : PLUGINS}
      components={COMPONENTS}
    >
      {pending ? holdBackSetextUnderline(text) : text}
    </Markdown>
  );
});
