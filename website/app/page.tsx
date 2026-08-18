import { Comparison } from "@/components/landing/comparison";
import { CTA } from "@/components/landing/cta";
import { DemoFrame } from "@/components/landing/demo-frame";
import { FAQ } from "@/components/landing/faq";
import { FeatureGrid } from "@/components/landing/feature-grid";
import { Footer } from "@/components/landing/footer";
import { ScrollStory } from "@/components/story/scroll-story";
import { HowItWorks } from "@/components/landing/how-it-works";
import { Problem } from "@/components/landing/problem";
import { ProviderCards } from "@/components/landing/provider-cards";
import { SiteHeader } from "@/components/landing/site-header";
import { StatsStrip } from "@/components/landing/stats-strip";
import { ToolCatalogue } from "@/components/landing/tool-catalogue";
import { UiShowcase } from "@/components/landing/ui-showcase";
import { siteConfig } from "@/lib/site";
import { getLatestRelease } from "@/lib/releases";

const jsonLd = {
  "@context": "https://schema.org",
  "@type": "SoftwareApplication",
  name: siteConfig.name,
  description: siteConfig.description,
  applicationCategory: "MultimediaApplication",
  operatingSystem: "macOS, Windows",
  url: siteConfig.url,
  downloadUrl: siteConfig.releases,
  offers: {
    "@type": "Offer",
    price: "0",
    priceCurrency: "USD",
  },
  aggregateRating: undefined,
  publisher: {
    "@type": "Organization",
    name: "edytlab",
    url: siteConfig.url,
  },
};

export default async function Home() {
  const release = await getLatestRelease();
  return (
    <>
      <script
        type="application/ld+json"
        dangerouslySetInnerHTML={{ __html: JSON.stringify(jsonLd) }}
      />
      <SiteHeader />
      <main className="relative">
        {/* One slow colour wash behind the top of the page, so the fold
            is not a flat sheet of near-black. */}
        <div aria-hidden className="aurora" />
        <ScrollStory release={release} />
        <StatsStrip />
        <Problem />
        <Comparison />
        <DemoFrame />
        <FeatureGrid />
        <UiShowcase />
        <ToolCatalogue />
        <HowItWorks />
        <ProviderCards />
        <FAQ />
        <CTA />
      </main>
      <Footer />
    </>
  );
}
