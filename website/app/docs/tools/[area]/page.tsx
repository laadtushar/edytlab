import type { Metadata } from "next";
import { notFound } from "next/navigation";

import { DocShell } from "@/components/docs/doc-shell";
import {
  AreaJump,
  AreaOutlineAside,
  AreaPager,
  ToolSection,
} from "@/components/docs/reference-parts";
import { areaByPath, areaPath } from "@/lib/docs/reference";
import { TOOLS_BASE, toolsReference } from "@/lib/docs/references";
import { toolAreas } from "@/lib/docs/tools-data";
import { pageAlternates, pageSocial } from "@/lib/seo-core";

interface Props {
  params: Promise<{ area: string }>;
}

// One page per area of `toolAreas`; a slug that is not one is a 404.
export const dynamicParams = false;

export function generateStaticParams() {
  return toolAreas.map((a) => ({ area: a.slug }));
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { area: slug } = await params;
  const area = areaByPath(toolsReference, slug);
  if (!area) return {};
  const path = areaPath(TOOLS_BASE, area.slug);
  const title = `${area.title}: edytlab agent tools`;
  const description = `${area.summary} ${area.countLabel}, each with an example prompt and what it returns.`;
  return {
    title,
    description,
    alternates: pageAlternates(path),
    ...pageSocial(path, `${title} · edytlab`, description),
  };
}

export default async function ToolsAreaPage({ params }: Props) {
  const { area: slug } = await params;
  const data = toolAreas.find((a) => a.slug === slug);
  const outline = areaByPath(toolsReference, slug);
  if (!data || !outline) notFound();

  const showHeadings = data.sections.length > 1;

  return (
    <DocShell
      title={data.title}
      description={data.summary}
      parent={{ name: toolsReference.name, path: TOOLS_BASE }}
      aside={<AreaOutlineAside reference={toolsReference} area={outline} />}
    >
      <AreaJump reference={toolsReference} area={outline} />

      {data.sections.map((section) => (
        <ToolSection
          key={section.title}
          title={section.title}
          tools={section.tools}
          showHeading={showHeadings}
        />
      ))}

      <AreaPager reference={toolsReference} area={outline} />
    </DocShell>
  );
}
