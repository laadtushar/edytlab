import Link from "next/link";

/** The "providers" page of the user guide: moved from the one long page, unchanged. */
export function Providers() {
  return (
    <>
      <p>
        Switch providers or models in Settings → Account, with no restart. Your
        conversation history carries over.
      </p>
      <ul>
        <li>
          <strong>Anthropic Claude Sonnet</strong> — the default; best for
          complex multi-track arrangements and long planning chains.
        </li>
        <li>
          <strong>Anthropic Claude Haiku</strong> — fastest and cheapest; good
          for simple edits.
        </li>
        <li>
          <strong>OpenRouter</strong> — access to 50+ models including
          open-weight options at lower cost.
        </li>
        <li>
          <strong>OpenAI</strong> — reliable tool use; the default is GPT-6
          Luna.
        </li>
        <li>
          <strong>Google Gemini</strong> and <strong>Groq</strong> — hosted
          alternatives, each with its own key.
        </li>
        <li>
          <strong>Ollama</strong> — a local model on your own machine; no key,
          and not even the chat leaves the computer.
        </li>
        <li>
          <strong>ChatGPT plan</strong> (a preview) — no key:
          the assistant runs on the usage included in your ChatGPT Plus or Pro
          plan. See{" "}
          <Link href="/docs/user-guide/chatgpt-plan">Using your ChatGPT plan</Link>.
        </li>
      </ul>
      <p>
        Your keys, and the ChatGPT sign-in, are kept together in one entry of
        your OS keychain, so macOS asks for your login password at most once
        per launch. Your provider, model, base URL and reasoning effort are not
        secret: they are saved in a small settings file in the app&apos;s
        config folder, which never holds a key. When you update from an
        earlier version, your saved keys move into the single entry on the
        first launch, which may ask once for each old entry.
      </p>
      <p>
        With Anthropic selected, Settings → Account also has a{" "}
        <strong>Reasoning effort</strong> setting: Default, Low, Medium, High,
        Extra high or Max. Higher effort makes Claude think longer, which is
        slower and uses more tokens; Default leaves it to the model. The
        ChatGPT plan has its own, described in{" "}
        <Link href="/docs/user-guide/chatgpt-plan">Using your ChatGPT plan</Link>; other providers have none.
      </p>
      <p>
        At the foot of Settings → Account, one line names the{" "}
        <strong>ONNX Runtime</strong> library the app found, for example{" "}
        <code>ONNX Runtime: 1.23.2 (bundled)</code>, with the file and the
        places it looked below it. It says{" "}
        <code>not installed in this build</code> when there is none, or why the
        one it found cannot be used, such as a macOS older than 13.4 (only the
        library needs it; the app runs on macOS 11 and later). An installed app says{" "}
        <code>bundled</code>; a build from source says{" "}
        <code>not installed in this build</code>. It is a status line: it
        installs nothing and does not mean any on-device audio model is
        available. No model comes with the runtime, and stem separation still
        returns an error.
      </p>
      <p>
        The{" "}
        <strong>Speech model</strong> row below it offers Whisper base (about
        139 MB, from Hugging Face). <strong>Download</strong> fetches its three
        files from <code>huggingface.co</code>, with a progress bar and a{" "}
        <strong>Cancel</strong> button, checks each against its SHA-256 hash,
        and keeps them in the <code>models</code> folder of the app&rsquo;s
        local data folder (<code>app.edytlab.desktop</code>; on Windows under{" "}
        <code>%LOCALAPPDATA%</code>, so it does not roam). Cancel keeps what
        has arrived and Download carries on from there; a failure shows why,
        with <strong>Retry</strong>. When the row says <code>Ready</code> it
        offers <strong>Remove</strong>, which asks first and deletes only that
        model&rsquo;s files. Nothing downloads unless you press Download: not
        at launch, not when Settings opens, not when the assistant asks. The
        requests carry your IP address, the file name, and a User-Agent of the
        form <code>edytlab/&lt;version&gt;</code>, so Hugging Face learns that
        edytlab is asking and which version, and nothing about your audio (see
        the <a href="/privacy">privacy page</a>).
      </p>
      <p>
        The <strong>Speaker model</strong> row below it is the same on a
        smaller model (about 33 MB, from Hugging Face): the pyannote
        segmentation network and the WeSpeaker voiceprint network, which
        together find who speaks when. <strong>Download</strong> fetches its
        two files from <code>huggingface.co</code>, with the same progress bar,{" "}
        <strong>Cancel</strong>, <strong>Retry</strong> and{" "}
        <strong>Remove</strong>, the same checks, and the same promise:
        nothing downloads unless you press Download, and the requests carry no
        audio. Until it says <code>Ready</code> the assistant has no speaker
        model to use.
      </p>
      <p>
        The runtime&rsquo;s licence and notices are installed with it, in{" "}
        <code>licenses/onnxruntime/</code>: inside{" "}
        <code>edytlab.app/Contents/Resources</code> on macOS, in{" "}
        <code>/usr/lib/edytlab</code> on Linux, and in the install folder on
        Windows, which also has the Visual C++ runtime the library needs and
        its notice in <code>licenses/vc-runtime/</code>.
      </p>
      <p>
        The speech model has no notice in the install folder, since it is not
        installed with the app. Whisper base is published by OpenAI under the
        Apache-2.0 licence (the Whisper code is MIT); the files are an ONNX
        export by the onnx-community on Hugging Face, pinned to one commit. The
        model cards are{" "}
        <a href="https://huggingface.co/openai/whisper-base">
          openai/whisper-base
        </a>{" "}
        and{" "}
        <a href="https://huggingface.co/onnx-community/whisper-base_timestamped">
          onnx-community/whisper-base_timestamped
        </a>
        , the second of which states no licence of its own. edytlab fetches the
        files when you click Download and does not redistribute them.
      </p>
      <p>
        The speaker model is two files, and each has its own licence. The
        segmentation network is pyannote&rsquo;s segmentation-3.0 (MIT licence,
        &copy; CNRS), as an ONNX export by the onnx-community:{" "}
        <a href="https://huggingface.co/onnx-community/pyannote-segmentation-3.0">
          onnx-community/pyannote-segmentation-3.0
        </a>
        . The voiceprint network is WeSpeaker ResNet34-LM, published by the
        WeSpeaker authors under CC BY 4.0:{" "}
        <a href="https://huggingface.co/Wespeaker/wespeaker-voxceleb-resnet34-LM">
          Wespeaker/wespeaker-voxceleb-resnet34-LM
        </a>
        . Both are pinned to one commit, fetched when you click Download and
        not redistributed. The credit is on the <a href="/terms">Terms page</a>.
      </p>
    </>
  );
}
