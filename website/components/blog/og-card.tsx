import { ImageResponse } from "next/og";

import { getPost } from "@/lib/blog";
import { motifPath, seedOf } from "@/lib/og-motif";

/**
 * The social card for a post: its title over a waveform motif, as a PNG.
 *
 * It is a PNG on purpose. Link previews on social networks and in chat
 * apps are made by crawlers that do not draw SVG, so the SVG cover on
 * the page cannot serve as the preview image. The palette here is the
 * site's, as hex, because an image renderer has no CSS variables.
 *
 * Shared by `opengraph-image.tsx` and `twitter-image.tsx`, which must
 * each declare their own `size` and `contentType` for Next to find.
 */

export const OG_SIZE = { width: 1200, height: 630 } as const;

const BG = "#0a0a0c";
const PRIMARY = "#c084fc";
const MUTED = "#a1a1aa";

function titleSize(title: string): number {
  if (title.length <= 40) return 76;
  if (title.length <= 62) return 64;
  return 54;
}

export function renderPostOg(slug: string): ImageResponse {
  const post = getPost(slug);
  const title = post?.title ?? "edytlab Blog";
  const tags = post?.tags.slice(0, 3) ?? [];
  const seed = seedOf(slug);

  return new ImageResponse(
    (
      <div
        style={{
          width: "100%",
          height: "100%",
          display: "flex",
          flexDirection: "column",
          justifyContent: "space-between",
          background: `radial-gradient(ellipse at top left, #2a0c4a 0%, ${BG} 62%)`,
          color: "white",
          fontFamily: "sans-serif",
        }}
      >
        <div style={{ display: "flex", flexDirection: "column", padding: "64px 72px 0" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 16, fontSize: 28, color: MUTED }}>
            <div
              style={{
                width: 28,
                height: 28,
                borderRadius: 6,
                background: `linear-gradient(135deg, ${PRIMARY} 0%, #7c3aed 100%)`,
              }}
            />
            edytlab · Blog
          </div>
          <div
            style={{
              display: "flex",
              marginTop: 44,
              fontSize: titleSize(title),
              fontWeight: 700,
              letterSpacing: -1.5,
              lineHeight: 1.1,
              maxWidth: 1040,
            }}
          >
            {title}
          </div>
          {tags.length > 0 ? (
            <div style={{ display: "flex", marginTop: 30, fontSize: 24, color: MUTED }}>
              {tags.join("  ·  ")}
            </div>
          ) : null}
        </div>
        <svg width="1200" height="160" viewBox="0 0 1200 160" fill="none">
          <path d={motifPath(seed)} stroke={PRIMARY} strokeWidth="5" strokeLinecap="round" />
        </svg>
      </div>
    ),
    OG_SIZE,
  );
}
