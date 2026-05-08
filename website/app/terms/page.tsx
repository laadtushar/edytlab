import type { Metadata } from "next";

import { LegalShell } from "@/components/landing/legal-shell";

export const metadata: Metadata = {
  title: "Terms",
  description: "Terms of use for edytlab.",
};

export default function TermsPage() {
  return (
    <LegalShell title="Terms of use" updated="2026-05-08">
      <p>
        These terms govern your use of the edytlab desktop application and this
        marketing website. By installing or using the app you agree to these
        terms. They are short on purpose.
      </p>

      <h2>Licence</h2>
      <p>
        edytlab is currently distributed in source form under an MIT-shaped
        permissive licence (a formal LICENSE file in the repository is the
        authoritative version once published). You may use, copy, and modify
        the source for any purpose, subject to retaining the copyright notice.
      </p>

      <h2>Third-party services</h2>
      <p>
        When you connect an LLM provider (Anthropic, OpenRouter, OpenAI, or a
        local model) you are subject to that provider&apos;s terms. edytlab is
        not a party to that relationship and is not responsible for charges
        you incur with that provider.
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
