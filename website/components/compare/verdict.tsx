import type { ReactNode } from "react";

/**
 * The page's bottom line, as two lists: when to choose the other
 * product, then when to choose edytlab. The other product comes first,
 * as it does everywhere on a comparison page. No score, no winner: a
 * reader decides by which sentence describes their job.
 */
export function Verdict({
  competitor,
  theirs,
  ours,
  closing,
}: {
  competitor: string;
  theirs: ReactNode[];
  ours: ReactNode[];
  /** A last line after the two lists, such as using both. */
  closing?: ReactNode;
}) {
  const card = "rounded-xl border border-border/60 bg-card p-5";
  const heading = "!mt-0 text-base font-semibold";
  const list = "!mt-3 space-y-2 text-sm";
  return (
    <section aria-labelledby="verdict">
      <h2 id="verdict">Verdict</h2>
      <div className="mt-6 grid gap-4 sm:grid-cols-2">
        <div className={card}>
          <h3 className={heading}>Choose {competitor} if</h3>
          <ul className={list}>
            {theirs.map((item, i) => (
              <li key={i}>{item}</li>
            ))}
          </ul>
        </div>
        <div className={card}>
          <h3 className={heading}>Choose edytlab if</h3>
          <ul className={list}>
            {ours.map((item, i) => (
              <li key={i}>{item}</li>
            ))}
          </ul>
        </div>
      </div>
      {closing ? <div className="mt-6">{closing}</div> : null}
    </section>
  );
}
