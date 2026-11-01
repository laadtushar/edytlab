import Link from "next/link";

/** The "speech" page of the user guide: moved from the one long page, unchanged. */
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

      <h2 id="speakers">Who Speaks When</h2>
      <p>
        Ask the agent <code>who speaks when on track 1</code> and it runs the
        speaker model on your computer (<code>diarize</code>) and answers with
        the speaker turns: start and end times, numbered{" "}
        <code>speaker_1</code>, <code>speaker_2</code> in the order they first
        speak. It changes nothing in the session. To give each voice its own
        track, ask it to split by speaker; that step (<code>split_by_speaker</code>)
        is an edit like any other and waits for your approval.
      </p>
      <p>
        The model is not installed with the app. Open Settings → Account and
        press <strong>Download</strong> beside the Speaker model row (about 33
        MB in two files; see{" "}
        <Link href="/docs/user-guide/providers">LLM Provider and Model</Link>).
        Nothing downloads unless you press it, and the agent cannot download
        it. The files come from <code>huggingface.co</code>, which sees your IP
        address and that edytlab is asking, and nothing about your audio.
      </p>
      <ul>
        <li>
          <strong>What it works on.</strong> A track as it sits on the timeline
          (turn times are session seconds), or an audio file by path (times are
          the file&rsquo;s). If you know how many people speak, say so; otherwise
          the count is estimated.
        </li>
        <li>
          <strong>Limits.</strong> Voices that sound alike can merge into one
          speaker, and where people talk over each other the audio goes to one
          of them. Check the turns before you split. A very long conversation
          is listed only in part, and the result says so.
        </li>
        <li>
          <strong>What reaches your AI provider.</strong> The audio never leaves
          your computer. The turns (times and speaker numbers) come back as a
          tool result, so they go to the provider you chose, and stay on your
          computer with Ollama.
        </li>
        <li>
          <strong>Speed and Cancel.</strong> A progress strip shows the work,
          and Cancel stops it without changing the session.
        </li>
      </ul>
      <p>
        The segmentation network is pyannote&rsquo;s (MIT licence, &copy; CNRS)
        and the voiceprint network is WeSpeaker&rsquo;s (CC BY 4.0); the
        credits are on the <Link href="/terms">Terms page</Link>.
      </p>
    </>
  );
}
