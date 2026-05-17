import type { Metadata } from "next";
import Link from "next/link";
import { Calendar, Clock, ArrowRight } from "lucide-react";

import { siteConfig } from "@/lib/site";
import { posts } from "@/lib/blog";
import { SiteHeader } from "@/components/landing/site-header";
import { Footer } from "@/components/landing/footer";
import { Badge } from "@/components/ui/badge";

export const metadata: Metadata = {
  title: "Blog",
  description:
    "Tutorials, deep dives, and opinions on AI audio editing, stem separation, podcast production, and local-first software.",
  openGraph: {
    title: "edytlab Blog — AI Audio Editing Guides & Tutorials",
    description:
      "Tutorials, deep dives, and opinions on AI audio editing, stem separation, podcast production, and local-first software.",
    url: `${siteConfig.url}/blog`,
    type: "website",
  },
  alternates: {
    canonical: "/blog",
  },
};

function formatDate(iso: string) {
  return new Date(iso).toLocaleDateString("en-US", {
    year: "numeric",
    month: "long",
    day: "numeric",
  });
}

export default function BlogPage() {
  const sorted = [...posts].sort(
    (a, b) => new Date(b.date).getTime() - new Date(a.date).getTime(),
  );

  return (
    <>
      <SiteHeader />
      <main className="min-h-screen pt-20">
        <section className="border-b border-border/50 py-16 md:py-20">
          <div className="container">
            <div className="mx-auto max-w-2xl">
              <h1 className="text-4xl font-bold tracking-tight sm:text-5xl">
                Blog
              </h1>
              <p className="mt-4 text-lg text-muted-foreground">
                Guides, deep dives, and opinions on AI audio editing, stem
                separation, podcast production, and why local-first software
                matters.
              </p>
            </div>
          </div>
        </section>

        <section className="py-12">
          <div className="container">
            <div className="mx-auto max-w-2xl space-y-10">
              {sorted.map((post) => (
                <article
                  key={post.slug}
                  className="group border-b border-border/40 pb-10 last:border-0"
                >
                  <Link href={`/blog/${post.slug}`} className="block">
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
                    <h2 className="text-xl font-semibold tracking-tight leading-snug group-hover:text-primary transition-colors sm:text-2xl">
                      {post.title}
                    </h2>
                    <p className="mt-2 text-muted-foreground leading-relaxed">
                      {post.excerpt}
                    </p>
                    <div className="mt-4 flex items-center gap-5 text-sm text-muted-foreground">
                      <span className="flex items-center gap-1.5">
                        <Calendar className="size-3.5" />
                        {formatDate(post.date)}
                      </span>
                      <span className="flex items-center gap-1.5">
                        <Clock className="size-3.5" />
                        {post.readTime} min read
                      </span>
                      <span className="ml-auto flex items-center gap-1 font-medium text-primary opacity-0 group-hover:opacity-100 transition-opacity">
                        Read more <ArrowRight className="size-3.5" />
                      </span>
                    </div>
                  </Link>
                </article>
              ))}
            </div>
          </div>
        </section>
      </main>
      <Footer />
    </>
  );
}
