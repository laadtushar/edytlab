import { getAllSlugs } from "@/lib/blog";
import { OG_SIZE, renderPostOg } from "@/components/blog/og-card";

// The Twitter card's image: the same card as `opengraph-image.tsx`.
// Next wires `twitter:image` only from a file named `twitter-image`,
// so the card needs its own file even though the picture is shared.
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
