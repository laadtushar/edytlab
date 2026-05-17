import type { Metadata } from "next";
import { notFound } from "next/navigation";
import Link from "next/link";
import { Calendar, Clock, ArrowLeft } from "lucide-react";

import { siteConfig } from "@/lib/site";
import { getPost, getAllSlugs, type Block } from "@/lib/blog";
import { SiteHeader } from "@/components/landing/site-header";
import { Footer } from "@/components/landing/footer";
import { Badge } from "@/components/ui/badge";

interface Props {
  params: Promise<{ slug: string }>;
}

export async function generateStaticParams() {
  return getAllSlugs().map((slug) => ({ slug }));
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { slug } = await params;
  const post = getPost(slug);
  if (!post) return {};

  return {
    title: post.title,
    description: post.excerpt,
    keywords: post.tags,
    authors: [{ name: "edytlab" }],
    openGraph: {
      title: post.title,
      description: post.excerpt,
      url: `${siteConfig.url}/blog/${post.slug}`,
      type: "article",
      publishedTime: post.date,
      tags: post.tags,
    },
    twitter: {
      card: "summary_large_image",
      title: post.title,
      description: post.excerpt,
    },
    alternates: {
      canonical: `/blog/${post.slug}`,
    },
  };
}

function formatDate(iso: string) {
  return new Date(iso).toLocaleDateString("en-US", {
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
          {block.text}
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
              {item}
            </li>
          ))}
        </ul>
      );
    case "callout":
      return (
        <blockquote
          key={i}
          className="mt-6 rounded-lg border border-primary/30 bg-primary/5 px-5 py-4 text-sm leading-7 text-foreground"
        >
          {block.text}
        </blockquote>
      );
  }
}

export default async function BlogPostPage({ params }: Props) {
  const { slug } = await params;
  const post = getPost(slug);
  if (!post) notFound();

  const jsonLd = {
    "@context": "https://schema.org",
    "@type": "Article",
    headline: post.title,
    description: post.excerpt,
    datePublished: post.date,
    author: { "@type": "Organization", name: "edytlab", url: siteConfig.url },
    publisher: {
      "@type": "Organization",
      name: "edytlab",
      logo: { "@type": "ImageObject", url: `${siteConfig.url}/logo.png` },
    },
    url: `${siteConfig.url}/blog/${post.slug}`,
    keywords: post.tags.join(", "),
  };

  return (
    <>
      <script
        type="application/ld+json"
        dangerouslySetInnerHTML={{ __html: JSON.stringify(jsonLd) }}
      />
      <SiteHeader />
      <main className="min-h-screen pt-20">
        <article className="py-12 md:py-16">
          <div className="container">
            <div className="mx-auto max-w-2xl">
              <Link
                href="/blog"
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

              <h1 className="text-3xl font-bold tracking-tight leading-snug sm:text-4xl">
                {post.title}
              </h1>

              <div className="mt-4 flex items-center gap-5 text-sm text-muted-foreground">
                <span className="flex items-center gap-1.5">
                  <Calendar className="size-3.5" />
                  {formatDate(post.date)}
                </span>
                <span className="flex items-center gap-1.5">
                  <Clock className="size-3.5" />
                  {post.readTime} min read
                </span>
              </div>

              <p className="mt-6 text-lg leading-relaxed text-muted-foreground border-l-2 border-primary/40 pl-4">
                {post.excerpt}
              </p>

              <div className="mt-8 border-t border-border/40 pt-8">
                {post.body.map((block, i) => renderBlock(block, i))}
              </div>

              <div className="mt-14 border-t border-border/40 pt-8">
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
              </div>
            </div>
          </div>
        </article>
      </main>
      <Footer />
    </>
  );
}
