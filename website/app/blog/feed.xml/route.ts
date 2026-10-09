import { buildFeed } from "@/lib/feed";

// Built once at build time: the posts are source files, so the feed
// can only change with a deploy.
export const dynamic = "force-static";

export function GET() {
  return new Response(buildFeed(), {
    headers: { "Content-Type": "application/rss+xml; charset=utf-8" },
  });
}
