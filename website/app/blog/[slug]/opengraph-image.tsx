import { getAllSlugs } from "@/lib/blog";
import { OG_SIZE, renderPostOg } from "@/components/blog/og-card";

// One 1200×630 PNG per post, drawn from its title. `twitter-image.tsx`
// beside this file draws the same card, so Open Graph and the Twitter
// card both point at a real image.
export const alt = "edytlab blog post";
export const size = OG_SIZE;
export const contentType = "image/png";

export function generateStaticParams() {
  return getAllSlugs().map((slug) => ({ slug }));
}

export default async function Image({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  return renderPostOg(slug);
}
