import type { Metadata } from "next";
import Link from "next/link";
import { Calendar, Clock, ArrowRight, Rss } from "lucide-react";

import { siteConfig } from "@/lib/site";
import { postsNewestFirst } from "@/lib/blog";
import {
  BLOG_TITLE,
  DEFAULT_OG_IMAGE,
  FEED_PATH,
  absoluteUrl,
  breadcrumbJsonLd,
  pageAlternates,
  postPath,
  serializeJsonLd,
} from "@/lib/seo";
import { BlogCover } from "@/components/blog/cover";
import { Cascade, LineDraw, Reveal, SplitHeading } from "@/components/motion";
import { SiteHeader } from "@/components/landing/site-header";
import { Footer } from "@/components/landing/footer";
import { Badge } from "@/components/ui/badge";

const DESCRIPTION =
  "Practical guides to editing audio by asking an AI agent: DJ transitions, choosing a model, the session graph, and local-first audio editing.";

export const metadata: Metadata = {
  title: "Blog: guides to editing audio with an AI agent",
  description: DESCRIPTION,
  openGraph: {
    title: `${BLOG_TITLE}: guides to editing audio with an AI agent`,
    description: DESCRIPTION,
    url: absoluteUrl("/blog"),
    siteName: siteConfig.name,
    type: "website",
    images: [DEFAULT_OG_IMAGE],
  },
  twitter: {
    card: "summary_large_image",
    title: `${BLOG_TITLE}: guides to editing audio with an AI agent`,
    description: DESCRIPTION,
    images: [DEFAULT_OG_IMAGE.url],
  },
  alternates: pageAlternates("/blog"),
};

function formatDate(iso: string) {
  return new Date(iso).toLocaleDateString("en-US", {
    timeZone: "UTC", // an ISO date is UTC midnight; the reader's zone must not move it a day
    year: "numeric",
    month: "long",
    day: "numeric",
  });
}

export default function BlogPage() {
  const sorted = postsNewestFirst();

  return (
    <>
      <script
        type="application/ld+json"
        dangerouslySetInnerHTML={{
          __html: serializeJsonLd(
            breadcrumbJsonLd([
              { name: "Home", path: "/" },
              { name: "Blog", path: "/blog" },
            ]),
          ),
        }}
      />
      <SiteHeader />
      <main className="min-h-screen pt-20">
        <section className="border-b border-border/50 py-16 md:py-20">
          <div className="container">
            <div className="mx-auto max-w-3xl">
              <SplitHeading
                as="h1"
                text="The edytlab blog"
                className="text-4xl font-bold tracking-tight sm:text-5xl"
              />
              <Reveal distance={10} delay={0.15}>
                <p className="mt-4 text-lg text-muted-foreground">
                  Practical guides to editing audio by describing what you
                  want: workflows you can copy sentence by sentence, how the
                  session graph keeps every take, and how to choose the
                  language model that drives it.
                </p>
                <p className="mt-4 text-sm text-muted-foreground">
                  <a
                    href={FEED_PATH}
                    className="inline-flex items-center gap-1.5 text-primary underline underline-offset-4 hover:no-underline"
                  >
                    <Rss className="size-3.5" />
                    Subscribe by RSS
                  </a>
                </p>
              </Reveal>
            </div>
          </div>
        </section>

        <section className="py-12">
          <div className="container">
            {/* Posts cascade in as they scroll up, their date and
                time icons drawing themselves; a post lifts under the
                pointer. */}
            <LineDraw>
              <Cascade className="mx-auto max-w-3xl space-y-10" each={0.08}>
                {sorted.map((post) => (
                  <article
                    key={post.slug}
                    className="group border-b border-border/40 pb-10 last:border-0"
                  >
                    <Link
                      href={postPath(post.slug)}
                      data-lift
                      className="grid gap-5 sm:grid-cols-[minmax(0,17rem)_1fr] sm:gap-6"
                    >
                      <div className="aspect-[1200/630] overflow-hidden rounded-lg border border-border/60">
                        <BlogCover slug={post.slug} alt={post.coverAlt} />
                      </div>
                      <div>
                        <div className="flex flex-wrap gap-2 mb-3">
                          {post.tags.slice(0, 3).map((tag) => (
                            <Badge
                              key={tag}
                              variant="outline"
                              className="border-primary/30 bg-primary/5 text-primary text-xs"
                            >
                              {tag}
                            </Badge>
                          ))}
                        </div>
                        <h2 className="text-xl font-semibold tracking-tight leading-snug group-hover:text-primary transition-colors">
                          {post.title}
                        </h2>
                        <p className="mt-2 text-muted-foreground leading-relaxed">
                          {post.description}
                        </p>
                        <div className="mt-4 flex flex-wrap items-center gap-x-5 gap-y-2 text-sm text-muted-foreground">
                          <span className="flex items-center gap-1.5">
                            <Calendar className="size-3.5" />
                            <time dateTime={post.date}>{formatDate(post.date)}</time>
                          </span>
                          <span className="flex items-center gap-1.5">
                            <Clock className="size-3.5" />
                            {post.readTime} min read
                          </span>
                          <span className="ml-auto flex items-center gap-1 font-medium text-primary opacity-0 group-hover:opacity-100 transition-opacity">
                            Read more <ArrowRight className="size-3.5" />
                          </span>
                        </div>
                      </div>
                    </Link>
                  </article>
                ))}
              </Cascade>
            </LineDraw>
          </div>
        </section>
      </main>
      <Footer />
    </>
  );
}
