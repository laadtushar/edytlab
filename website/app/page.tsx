import type { Metadata } from "next";

import { JsonLd } from "@/components/json-ld";
import { Comparison } from "@/components/landing/comparison";
import { CTA } from "@/components/landing/cta";
import { FAQ } from "@/components/landing/faq";
import { FeatureGrid } from "@/components/landing/feature-grid";
import { Footer } from "@/components/landing/footer";
import { ScrollStory } from "@/components/story/scroll-story";
import { LatestPosts } from "@/components/landing/latest-posts";
import { ProviderCards } from "@/components/landing/provider-cards";
import { Parallax } from "@/components/motion";
import { RealDemos } from "@/components/landing/real-demos";
import { SiteHeader } from "@/components/landing/site-header";
import { StatsStrip } from "@/components/landing/stats-strip";
import { ToolCatalogue } from "@/components/landing/tool-catalogue";
import { UiShowcase } from "@/components/landing/ui-showcase";
import { getLatestRelease } from "@/lib/releases";
import { organizationJsonLd, pageAlternates, softwareJsonLd, websiteJsonLd } from "@/lib/seo";

export const metadata: Metadata = { alternates: pageAlternates("/") };

export default async function Home() {
  const release = await getLatestRelease();
  // The version the page offers for download, when GitHub answered. The
  // placeholder used when it did not is no version of anything, so it
  // is left out rather than published.
  const jsonLd = softwareJsonLd(release.isFallback ? undefined : release.version);
  return (
    <>
      <JsonLd data={organizationJsonLd()} />
      <JsonLd data={websiteJsonLd()} />
      <JsonLd data={jsonLd} />
      <SiteHeader />
      <main className="relative">
        {/* One section per idea, in the order a reader asks:
            what is it (the story), how much (the numbers), is it real
            (recordings of the app), what does it do today and not yet
            (the table), what else does it do (features, then the tools
            by name), can I do it by hand (the interface), which model
            (providers), the leftovers (FAQ), and what to read next
            (the newest posts) — then the download.

            Cut as repeats: a scripted chat mock that told the story
            the hero already tells, while the real recordings show it
            for real; a "problem" band that said DAWs are hard and AI
            tools shallow, which nothing backed; and a three-step "how it works" that was the
            hero's story again, down to the feature grid's own example
            prompt. */}
        <ScrollStory release={release} />
        <StatsStrip />
        <RealDemos />
        <Comparison />
        <FeatureGrid />
        <ToolCatalogue />
        <UiShowcase />
        <ProviderCards />
        <FAQ />
        <LatestPosts />
        <CTA release={release} />
        {/* One slow colour wash behind the page, so the fold is not a
            flat sheet of near-black. It drifts against the scroll — a
            little slower than the content, which reads as depth. Last
            in the markup (it is absolutely placed, so order is not
            layout) so its scroll range is measured after the story's
            pin has added its height. */}
        <Parallax className="aurora" distance={-160} />
      </main>
      <Footer />
    </>
  );
}
