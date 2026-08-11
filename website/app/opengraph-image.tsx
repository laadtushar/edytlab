import { ImageResponse } from "next/og";

export const runtime = "edge";
export const alt = "edytlab — Describe it. Get pro-grade audio edits.";
export const size = { width: 1200, height: 630 };
export const contentType = "image/png";

export default function OpengraphImage() {
  return new ImageResponse(
    (
      <div
        style={{
          width: "100%",
          height: "100%",
          display: "flex",
          flexDirection: "column",
          justifyContent: "center",
          padding: 80,
          background:
            "radial-gradient(ellipse at top, #2a0c4a 0%, #0a0a0c 60%)",
          color: "white",
          fontFamily: "sans-serif",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 16,
            fontSize: 28,
            opacity: 0.7,
          }}
        >
          <div
            style={{
              width: 28,
              height: 28,
              borderRadius: 6,
              background:
                "linear-gradient(135deg, #c084fc 0%, #7c3aed 100%)",
            }}
          />
          edytlab
        </div>
        <div
          style={{
            fontSize: 84,
            fontWeight: 700,
            letterSpacing: -2,
            lineHeight: 1.05,
            marginTop: 40,
            display: "flex",
            flexDirection: "column",
          }}
        >
          <span>Describe it.</span>
          <span
            style={{
              background:
                "linear-gradient(135deg, #c084fc 0%, #d946ef 100%)",
              backgroundClip: "text",
              color: "transparent",
            }}
          >
            Get pro-grade audio edits.
          </span>
        </div>
        <div
          style={{
            fontSize: 30,
            marginTop: 36,
            opacity: 0.7,
            maxWidth: 900,
          }}
        >
          Desktop audio editor where you chat with an AI to load, cut, mix,
          transcribe, and render. Pure-Rust DSP, local-first, BYO LLM key.
        </div>
        <div
          style={{
            display: "flex",
            gap: 12,
            marginTop: 48,
            fontSize: 22,
            opacity: 0.6,
          }}
        >
          <span>macOS</span>
          <span>·</span>
          <span>Windows</span>
          <span>·</span>
          <span>Anthropic / OpenAI / Gemini / Groq / OpenRouter</span>
        </div>
      </div>
    ),
    size,
  );
}
