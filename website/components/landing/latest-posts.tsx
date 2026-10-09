import Link from "next/link";
import { ArrowRight } from "lucide-react";

import { BlogCover } from "@/components/blog/cover";
import { LineDraw, Stagger } from "@/components/motion";
import { SectionHeading } from "@/components/motion/section-heading";
import { postsNewestFirst } from "@/lib/blog";
import { postPath } from "@/lib/seo";

/**
 * The newest three posts, under the FAQ and above the download: a
 * reader who has just had their questions answered meets the how-to
 * posts before the call to action. The posts are workflows with
 * sentences to type, which the page above only has room to claim.
 */
export function LatestPosts() {
  const latest = postsNewestFirst().slice(0, 3);

  return (
    <section id="blog" className="py-20 md:py-28">
      <div className="container">
        <SectionHeading
          eyebrow="From the blog"
          title="Workflows you can copy, sentence by sentence."
          lead="How a DJ transition is built, how to choose the model behind the chat, and how undo keeps every take."
        />
        <LineDraw>
          <Stagger
            className="mx-auto grid max-w-5xl gap-4 md:grid-cols-3"
            each={0.06}
            distance={20}
          >
            {latest.map((post) => (
              <Link
                key={post.slug}
                href={postPath(post.slug)}
                data-lift
                className="ring-hover group flex h-full flex-col overflow-hidden rounded-xl border border-border/60 bg-card/60 hover:bg-card"
              >
                <div className="aspect-[1200/630] border-b border-border/60">
                  <BlogCover slug={post.slug} alt={post.coverAlt} />
                </div>
                <div className="flex flex-1 flex-col gap-2 p-5">
                  <h3 className="text-base font-semibold leading-snug group-hover:text-primary">
                    {post.title}
                  </h3>
                  <p className="text-sm text-muted-foreground">{post.description}</p>
                  <div className="mt-auto inline-flex items-center gap-1 pt-3 text-xs font-medium text-primary">
                    Read the post
                    <ArrowRight className="size-3.5 transition-transform duration-300 group-hover:translate-x-0.5" />
                  </div>
                </div>
              </Link>
            ))}
          </Stagger>
        </LineDraw>
        <p className="mt-8 text-center text-sm text-muted-foreground">
          <Link
            href="/blog"
            className="text-primary underline underline-offset-4 hover:no-underline"
          >
            All posts
          </Link>
        </p>
      </div>
    </section>
  );
}
