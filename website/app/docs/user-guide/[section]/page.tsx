import type { Metadata } from "next";
import { notFound } from "next/navigation";

import { DocShell } from "@/components/docs/doc-shell";
import {
  AreaJump,
  AreaOutlineAside,
  AreaPager,
} from "@/components/docs/reference-parts";
import { guideContent } from "@/components/docs/user-guide";
import { areaByPath, areaPath } from "@/lib/docs/reference";
import { USER_GUIDE_BASE, userGuideReference } from "@/lib/docs/references";
import { guidePages } from "@/lib/docs/user-guide-data";
import { pageAlternates, pageSocial } from "@/lib/seo-core";

interface Props {
  params: Promise<{ section: string }>;
}

// One page per entry of `guidePages`; a slug that is not one is a 404.
export const dynamicParams = false;

export function generateStaticParams() {
  return guidePages.map((p) => ({ section: p.slug }));
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { section: slug } = await params;
  const page = guidePages.find((p) => p.slug === slug);
  if (!page) return {};
  const path = areaPath(USER_GUIDE_BASE, page.slug);
  const title = `${page.title}: edytlab user guide`;
  return {
    title,
    description: page.summary,
    alternates: pageAlternates(path),
    ...pageSocial(path, `${title} · edytlab`, page.summary),
  };
}

export default async function UserGuideSectionPage({ params }: Props) {
  const { section: slug } = await params;
  const page = guidePages.find((p) => p.slug === slug);
  const outline = areaByPath(userGuideReference, slug);
  const Content = guideContent[slug];
  if (!page || !outline || !Content) notFound();

  return (
    <DocShell
      title={page.title}
      description={page.summary}
      parent={{ name: userGuideReference.name, path: USER_GUIDE_BASE }}
      aside={<AreaOutlineAside reference={userGuideReference} area={outline} />}
    >
      <AreaJump reference={userGuideReference} area={outline} />

      <Content />

      <AreaPager reference={userGuideReference} area={outline} />
    </DocShell>
  );
}
