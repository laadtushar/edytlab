import type { Metadata } from "next";

import { LegalShell } from "@/components/landing/legal-shell";
import { pageSocial } from "@/lib/seo";

const title = "Terms of use: MIT licence, no warranty";
const description =
  "The terms for using edytlab, the free open-source desktop audio editor: the MIT licence, third-party LLM providers, the disclaimer and the limit of liability.";

export const metadata: Metadata = {
  title,
  description,
  alternates: { canonical: "/terms" },
  ...pageSocial("/terms", `${title} · edytlab`, description),
};

export default function TermsPage() {
  return (
    <LegalShell title="Terms of use" path="/terms" updated="2026-10-27">
      <p>
        These terms govern your use of the edytlab desktop application and this
        marketing website. By installing or using the app you agree to these
        terms. They are short on purpose.
      </p>

      <h2>Licence</h2>
      <p>
        edytlab is released under the MIT licence; the{" "}
        <code>LICENSE</code> file in the repository is the authoritative
        version. You may use, copy, and modify the source for any purpose,
        subject to retaining the copyright notice.
      </p>

      <h2>Third-party services</h2>
      <p>
        When you connect an LLM provider (Anthropic, OpenAI, Google Gemini, Groq, OpenRouter, or a
        local model) you are subject to that provider&apos;s terms. The same
        goes for the ChatGPT plan: you sign in to OpenAI, and
        OpenAI&apos;s terms and the terms of your ChatGPT plan apply to the
        usage, which counts toward that plan. edytlab is
        not a party to that relationship and is not responsible for charges
        you incur with that provider.
      </p>
      <p>
        The speech model you can download from Settings is Whisper base, which
        OpenAI publishes under the Apache-2.0 licence (its code is MIT). edytlab
        does not include or redistribute it: when you click Download, the app
        fetches the files from Hugging Face, whose terms apply to that
        download, and they are kept on your computer. See{" "}
        <a href="https://huggingface.co/openai/whisper-base">the model card</a>
        {" "}and the <a href="/privacy">privacy page</a>.
      </p>

      <h2>Third-party licences</h2>
      <p>
        edytlab is built from open source libraries, fonts and code, each under
        its own licence. Their licence texts and copyright notices ship with
        every installer, are shown in the app under Settings, Account, Open
        source licences, and are in the repository as{" "}
        <a href="https://github.com/laadtushar/edytlab/blob/main/apps/desktop/src-tauri/licenses/THIRD_PARTY_NOTICES.txt">
          THIRD_PARTY_NOTICES.txt
        </a>
        . The app&apos;s Geist and Geist Mono fonts (&copy; 2024 The Geist
        Project Authors) and this website&apos;s Inter font (&copy; 2016 The
        Inter Project Authors, from{" "}
        <a href="https://github.com/rsms/inter/blob/master/LICENSE.txt">its licence</a>)
        are under the SIL Open Font License 1.1.
      </p>

      <h2>Disclaimer</h2>
      <p>
        The software is provided &ldquo;as is&rdquo;, without warranty of any
        kind, express or implied. We do not warrant that audio output will
        match any particular reference, that ML models will produce specific
        results, or that the app is free of defects.
      </p>

      <h2>Limitation of liability</h2>
      <p>
        To the fullest extent permitted by law, the authors and contributors
        are not liable for any indirect, incidental, special, or consequential
        damages arising out of your use of the software, including loss of
        audio data. Keep backups.
      </p>
    </LegalShell>
  );
}
