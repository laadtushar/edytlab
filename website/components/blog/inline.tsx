import Link from "next/link";

import { isExternal, parseInline } from "@/lib/inline";

/** Blog text with its inline markup (links, `code`, **bold**) drawn as elements. */
export function InlineText({ text }: { text: string }) {
  return (
    <>
      {parseInline(text).map((t, i) => {
        switch (t.type) {
          case "text":
            return t.text;
          case "code":
            return (
              <code
                key={i}
                className="rounded bg-secondary px-1.5 py-0.5 font-mono text-[0.9em] text-foreground"
              >
                {t.text}
              </code>
            );
          case "bold":
            return (
              <strong key={i} className="font-semibold text-foreground">
                {t.text}
              </strong>
            );
          case "link": {
            const className =
              "text-primary underline underline-offset-4 hover:no-underline";
            return isExternal(t.href) ? (
              <a
                key={i}
                href={t.href}
                target="_blank"
                rel="noopener noreferrer"
                className={className}
              >
                {t.text}
              </a>
            ) : (
              <Link key={i} href={t.href} className={className}>
                {t.text}
              </Link>
            );
          }
        }
      })}
    </>
  );
}
