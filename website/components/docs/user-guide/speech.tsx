import Link from "next/link";

/** The "speech" page of the user guide. */
export function Speech() {
  return (
    <>
      <h2 id="stem-separation">Stem Separation</h2>
      <p>
        <strong>Not available in this build.</strong> Stem separation is meant
        to split a track into vocals, drums, bass and other with Demucs, on
        your device, but inference ships as a stub, so asking the agent to{" "}
        <code>separate the stems on track 1</code> returns an error. There is
        no stem model to download and no environment variable that changes
        this: the decoder is what is missing, not the model file.
      </p>

      <h2 id="transcription">Transcription</h2>
      <p>
        Ask the agent to <code>transcribe track 1</code> and it
        writes what was said, with the start and end time of every word, as a
        new version of the session. It runs on your CPU with the Whisper base
        speech model. No audio is sent anywhere, and transcribing makes no
        network request.
      </p>
      <p>
        The model is not installed with the app. The first time, open Settings
        → Account and press <strong>Download</strong> beside the Speech model
        row (about 139 MB; see{" "}
        <Link href="/docs/user-guide/providers">LLM Provider and Model</Link>). Until it says <code>Ready</code>, a
        transcript request shows a card saying what is missing, with a button
        for Settings. The agent cannot download the model: only your click
        does. A build from source also needs the ONNX Runtime library, which
        the installers carry.
      </p>
      <ul>
        <li>
          <strong>What it transcribes.</strong> A track as it sits on the
          timeline (word times are session seconds), or an audio file outside
          the session, by path (times are the file&rsquo;s). A new transcription
          replaces the earlier one.
        </li>
        <li>
          <strong>Language.</strong> The model detects it, or ask for one by
          two-letter code (<code>en</code>, <code>de</code>, <code>fr</code>,{" "}
          <code>es</code>). The model is multilingual Whisper base, checked
          only on English: a short synthetic recording and about 16 minutes of
          audiobook speech, where 4% to 7% of the words were wrong. Read the
          words before you cut on them.
        </li>
        <li>
          <strong>Speed and Cancel.</strong> A progress strip counts the
          30-second windows, and Cancel stops it without changing the session.
          Speed depends on your CPU: on our test machine seven seconds of audio
          took about three seconds the first time, and the same audio again is
          answered from a cache. The whole recording is decoded into memory
          first: about 1.3 GB for an hour of 44.1 kHz stereo.
        </li>
        <li>
          <strong>The Transcript tab.</strong> It shows the words with their
          times. Select words and press Delete to cut that audio and close the
          gap. The tab cuts only the track the transcript is of, and says why
          when it will not.
        </li>
        <li>
          <strong>Editing by words.</strong> Ask the agent to cut a passage
          (<code>cut_words</code>), find and remove fillers such as um and uh
          (<code>remove_fillers</code>, which changes nothing until you say to
          apply it), or select a phrase (<code>select_region</code>). Each edit
          is one undoable step.
        </li>
        <li>
          <strong>What reaches your AI provider.</strong> The words come back as
          a tool result, so the text of what was said (a bounded amount per
          call, never the audio) goes to the provider you chose, and stays on
          your computer with Ollama.
        </li>
      </ul>
      <p>The rest of the editor works without a transcript or the model.</p>

      <h2 id="transcript-export-and-markers">Transcript Export and Markers</h2>
      <p>
        After transcribing, ask <code>export the transcript to captions.srt</code>{" "}
        to write an SRT file, or name <code>vtt</code> for WebVTT or{" "}
        <code>txt</code> for plain text. The <code>export_transcript</code> tool
        needs <code>out_path</code>, relative to the project folder or absolute.
        It writes the file atomically, protects project audio from being
        overwritten, and changes no audio or session history.
      </p>
      <p>
        Captions split at sentence-ending punctuation and at a pause of 0.75
        seconds by default, and normally stay within six seconds and two lines
        of 42 Unicode scalar values each (not display columns). Ask for different <code>pause_sec</code> (0 or more),{" "}
        <code>max_cue_sec</code> (greater than 0) or{" "}
        <code>max_chars_per_line</code> (an integer from 1 to 200). A single
        stored word may need more lines or exceed the duration limit, because
        the export keeps its recorded times. TXT keeps each segment on one
        unwrapped line. All three formats use UTF-8 and LF line endings.
        WebVTT escapes markup characters; SRT keeps raw text, which readers
        may interpret as markup.
      </p>
      <p>
        Ask <code>add a marker at each transcript sentence</code> to use{" "}
        <code>transcript_to_markers</code>. It adds point markers in one
        undoable step, keeps existing annotations, and changes no audio.
        Choose <code>mode: segment</code> (the default caption grouping) or{" "}
        <code>mode: sentence</code> (sentence-ending punctuation). Labels use the first six words, capped at
        80 Unicode characters.
      </p>
      <p>
        Both tools use stored word times, including after later audio edits.
        A track transcript keeps recorded session seconds; a file transcript
        keeps file seconds; an older transcript may have an unknown source.
        Results report that time basis and source, with a stable track ID
        and its current index when available. For markers from a file or an
        unknown source, explicitly supply a signed <code>offset_sec</code> to
        align those times to the timeline, even if the offset is 0. Every
        resulting time must be finite and at least 0. A removed source track
        is refused even with an offset; transcribe an existing track again.
      </p>
    </>
  );
}
