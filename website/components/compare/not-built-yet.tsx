/**
 * The standing note on every comparison page: what edytlab has not
 * built. It edits audio files offline, so it is not a live DJ tool and
 * not a DAW: no MIDI, no VST or CLAP plugin hosting. (Transcription
 * shipped in v0.8.0, #384, and stem separation in v0.9.4, #385; neither
 * is listed here.) Other products do this; the note says edytlab does
 * not, in words that cannot be read as a promise.
 *
 * The page's other sentences lean on this one: any sentence that names
 * one of these capabilities has to deny it too (`competitors.test.ts`).
 */
export function NotBuiltYet() {
  return (
    <div role="note" aria-labelledby="not-built-yet" className="mt-8 mb-8 rounded-xl border border-primary/30 bg-primary/5 p-5 text-sm">
      <div id="not-built-yet" className="font-semibold">
        Not built yet
      </div>
      <div className="mt-2 leading-relaxed">
        edytlab edits audio files offline. It has no MIDI, no VST or CLAP plugin hosting, and no decks or controllers for live
        use, so it is not a DAW and not a live DJ tool. Its stems are estimates: each one carries a little of the others.
      </div>
    </div>
  );
}
