import type { Metadata } from "next";
import { notFound } from "next/navigation";
import Link from "next/link";
import { Calendar, Clock, ArrowLeft } from "lucide-react";

import { siteConfig } from "@/lib/site";
import {
  BLOG_AUTHOR,
  getPost,
  getAllSlugs,
  modifiedOf,
  type Block,
} from "@/lib/blog";
import {
  absoluteUrl,
  blogPostingJsonLd,
  breadcrumbJsonLd,
  pageAlternates,
  postBreadcrumbs,
  postPath,
  serializeJsonLd,
} from "@/lib/seo";
import { BlogCover } from "@/components/blog/cover";
import { InlineText } from "@/components/blog/inline";
import { SiteHeader } from "@/components/landing/site-header";
import { Footer } from "@/components/landing/footer";
import { Badge } from "@/components/ui/badge";
import { Cascade, LineDraw, Reveal, SplitHeading, Stagger } from "@/components/motion";

interface Props {
  params: Promise<{ slug: string }>;
}

export async function generateStaticParams() {
  return getAllSlugs().map((slug) => ({ slug }));
}

// The social image comes from `opengraph-image.tsx` and
// `twitter-image.tsx` in this folder: Next adds them to `openGraph`
// and `twitter` itself, so neither block names an image here.
export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { slug } = await params;
  const post = getPost(slug);
  if (!post) return {};

  return {
    title: post.title,
    description: post.description,
    keywords: post.tags,
    authors: [{ name: BLOG_AUTHOR.name, url: BLOG_AUTHOR.url }],
    alternates: pageAlternates(postPath(post.slug)),
    openGraph: {
      title: post.title,
      description: post.description,
      url: absoluteUrl(postPath(post.slug)),
      siteName: siteConfig.name,
      type: "article",
      publishedTime: post.date,
      modifiedTime: modifiedOf(post),
      authors: [BLOG_AUTHOR.name],
      tags: post.tags,
    },
    twitter: {
      card: "summary_large_image",
      title: post.title,
      description: post.description,
    },
  };
}

function formatDate(iso: string) {
  return new Date(iso).toLocaleDateString("en-US", {
    timeZone: "UTC", // an ISO date is UTC midnight; the reader's zone must not move it a day
    year: "numeric",
    month: "long",
    day: "numeric",
  });
}

function renderBlock(block: Block, i: number) {
  switch (block.type) {
    case "h2":
      return (
        <h2
          key={i}
          className="mt-10 text-2xl font-semibold tracking-tight text-foreground"
        >
          {block.text}
        </h2>
      );
    case "h3":
      return (
        <h3
          key={i}
          className="mt-7 text-lg font-semibold tracking-tight text-foreground"
        >
          {block.text}
        </h3>
      );
    case "p":
      return (
        <p key={i} className="mt-4 leading-7 text-muted-foreground">
          <InlineText text={block.text} />
        </p>
      );
    case "ul":
      return (
        <ul key={i} className="mt-4 space-y-2 pl-6">
          {block.items.map((item, j) => (
            <li
              key={j}
              className="relative text-muted-foreground leading-7 before:absolute before:-left-4 before:text-primary before:content-['–']"
            >
              <InlineText text={item} />
            </li>
          ))}
        </ul>
      );
    case "ol":
      return (
        <ol key={i} className="mt-4 list-decimal space-y-2 pl-6 marker:text-primary">
          {block.items.map((item, j) => (
            <li key={j} className="text-muted-foreground leading-7">
              <InlineText text={item} />
            </li>
          ))}
        </ol>
      );
    case "prompt":
      // A line to type into the chat: shown as typed, never parsed.
      return (
        <div
          key={i}
          className="mt-4 flex gap-3 rounded-lg border border-primary/30 bg-secondary/60 px-4 py-3 font-mono text-sm leading-6 text-foreground"
        >
          <span aria-hidden="true" className="select-none text-primary">
            &gt;
          </span>
          <span>
            <span className="sr-only">Prompt: </span>
            {block.text}
          </span>
        </div>
      );
    case "callout":
      return (
        <blockquote
          key={i}
          className="mt-6 rounded-lg border border-primary/30 bg-primary/5 px-5 py-4 text-sm leading-7 text-foreground"
        >
          <InlineText text={block.text} />
        </blockquote>
      );
  }
}

export default async function BlogPostPage({ params }: Props) {
  const { slug } = await params;
  const post = getPost(slug);
  if (!post) notFound();

  return (
    <>
      <script
        type="application/ld+json"
        dangerouslySetInnerHTML={{ __html: serializeJsonLd(blogPostingJsonLd(post)) }}
      />
      <script
        type="application/ld+json"
        dangerouslySetInnerHTML={{
          __html: serializeJsonLd(breadcrumbJsonLd(postBreadcrumbs(post))),
        }}
      />
      <SiteHeader />
      <main className="min-h-screen pt-20">
        <article className="py-12 md:py-16">
          <div className="container">
            <div className="mx-auto max-w-2xl">
              {/* The header settles piece by piece (the title rising
                  a word at a time instead of fading), its icons draw,
                  and the body cascades in block by block below. */}
              <LineDraw>
                <Stagger each={0.07} distance={14} selector=":scope > :not(h1)">
                  <Link
                    href="/blog"
                    data-nudge="-3"
                    className="mb-8 inline-flex items-center gap-1.5 text-sm text-muted-foreground transition-colors hover:text-foreground"
                  >
                    <ArrowLeft className="size-3.5" />
                    All posts
                  </Link>

                  <div className="flex flex-wrap gap-2 mb-4">
                    {post.tags.slice(0, 4).map((tag) => (
                      <Badge
                        key={tag}
                        variant="outline"
                        className="border-primary/30 bg-primary/5 text-primary text-xs"
                      >
                        {tag}
                      </Badge>
                    ))}
                  </div>

                  <SplitHeading
                    as="h1"
                    text={post.title}
                    delay={0.1}
                    className="text-3xl font-bold tracking-tight leading-snug sm:text-4xl"
                  />

                  <div className="mt-4 flex items-center gap-5 text-sm text-muted-foreground">
                    <span className="flex items-center gap-1.5">
                      <Calendar className="size-3.5" />
                      <time dateTime={post.date}>{formatDate(post.date)}</time>
                    </span>
                    <span className="flex items-center gap-1.5">
                      <Clock className="size-3.5" />
                      {post.readTime} min read
                    </span>
                  </div>

                  <div className="mt-8 aspect-[1200/630] overflow-hidden rounded-xl border border-border/60">
                    <BlogCover slug={post.slug} alt={post.coverAlt} />
                  </div>

                  <p className="mt-6 text-lg leading-relaxed text-muted-foreground border-l-2 border-primary/40 pl-4">
                    {post.excerpt}
                  </p>
                </Stagger>
              </LineDraw>

              <Cascade prose className="mt-8 border-t border-border/40 pt-8">
                {post.body.map((block, i) => renderBlock(block, i))}
              </Cascade>

              <Reveal className="mt-14 border-t border-border/40 pt-8" distance={12}>
                <p className="text-sm text-muted-foreground">
                  edytlab is an open-source, local-first AI audio editor.{" "}
                  <Link
                    href={siteConfig.releases}
                    className="text-primary underline underline-offset-4 hover:no-underline"
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    Download the latest release
                  </Link>{" "}
                  or{" "}
                  <Link
                    href={siteConfig.github}
                    className="text-primary underline underline-offset-4 hover:no-underline"
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    star it on GitHub
                  </Link>
                  .
                </p>
              </Reveal>
            </div>
          </div>
        </article>
      </main>
      <Footer />
    </>
  );
}
