import { siteConfig } from "@/lib/site";

/**
 * The standing note on every comparison page: what edytlab has not
 * shipped. Stem separation is wired in as a tool that returns an error
 * today (#385), and everything that waits on it is missing with it.
 * (Transcription shipped in v0.8.0, #384, and is no longer listed.)
 * Other products do this; the note says edytlab does not, in words that
 * cannot be read as a promise.
 *
 * The page's other sentences lean on this one: any sentence that names
 * one of these capabilities has to deny it too (`competitors.test.ts`).
 */
export function NotBuiltYet() {
  return (
    <div role="note" aria-labelledby="not-built-yet" className="my-8 rounded-xl border border-primary/30 bg-primary/5 p-5 text-sm">
      <div id="not-built-yet" className="font-semibold">
        Not built yet
      </div>
      <div className="mt-2 leading-relaxed">
        Stem separation has not shipped in edytlab. Its tool exists but returns an error today, so edytlab does not offer vocal
        isolation. Progress is tracked in{" "}
        <a href={`${siteConfig.github}/issues/385`} target="_blank" rel="noopener noreferrer">
          issue #385
        </a>{" "}
        on GitHub.
      </div>
    </div>
  );
}
