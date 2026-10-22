import { siteConfig } from "@/lib/site";

/**
 * The standing note on every comparison page: what edytlab has not
 * shipped. Stem separation and transcription are wired in as tools that
 * return an error today (#385, #384), and everything that waits on them
 * is missing with them. Other products do these things; the note says
 * edytlab does not, in words that cannot be read as a promise.
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
        Stem separation and transcription have not shipped in edytlab. Their tools exist but return an error today, so edytlab does not offer vocal isolation, a transcript, text-based editing
        or filler-word removal. Where another product does these, it does them and edytlab does not. Neither is finished;
        progress on each is tracked on GitHub:{" "}
        <a href={`${siteConfig.github}/issues/385`} target="_blank" rel="noopener noreferrer">
          stem separation
        </a>{" "}
        and{" "}
        <a href={`${siteConfig.github}/issues/384`} target="_blank" rel="noopener noreferrer">
          transcription
        </a>
        .
      </div>
    </div>
  );
}
