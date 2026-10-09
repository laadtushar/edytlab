import { cn } from "@/lib/utils";

/**
 * A line of text split into words, ready for a word-by-word rise.
 *
 * Hand-split rather than with `SplitText`: the split is in the markup,
 * so the server renders exactly what the browser animates and nothing
 * re-splits on resize — which matters inside the scroll story, where a
 * timeline holds on to these nodes for the life of the page.
 *
 * Split by word, not by character. Per-character reveals look fine on a
 * three-word logotype and become unreadable on a sentence. The words
 * stay real text, so a screen reader reads the sentence and a search
 * engine indexes it; each `.word` is what an animation targets, and the
 * clipping wrapper around it makes a word rising from `yPercent: 110`
 * come up from behind the line rather than over the one above.
 *
 * No hooks: whoever renders the words owns the animation, and with no
 * animation they are simply the sentence.
 */
export function Words({ text, className }: { text: string; className?: string }) {
  const words = text.split(" ");
  return (
    <span>
      {words.map((w, i) => (
        <span key={`${w}-${i}`}>
          <span className="inline-block overflow-hidden py-[0.08em] align-bottom">
            <span className={cn("word inline-block", className)}>{w}</span>
          </span>
          {/* A real space as a text node, outside the inline-blocks, so
              the line sets at its normal word spacing and copies as a
              sentence. */}
          {i < words.length - 1 ? " " : null}
        </span>
      ))}
    </span>
  );
}
