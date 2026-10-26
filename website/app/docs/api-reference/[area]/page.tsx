import type { Metadata } from "next";
import { notFound } from "next/navigation";

import { DocShell } from "@/components/docs/doc-shell";
import {
  AreaJump,
  AreaOutlineAside,
  AreaPager,
  CommandSection,
  EventSection,
  TypesSection,
} from "@/components/docs/reference-parts";
import { apiAreas } from "@/lib/docs/api-data";
import { areaByPath, areaPath } from "@/lib/docs/reference";
import { API_BASE, apiReference } from "@/lib/docs/references";
import { pageAlternates, pageSocial } from "@/lib/seo-core";

interface Props {
  params: Promise<{ area: string }>;
}

// One page per area of `apiAreas`; a slug that is not one is a 404.
export const dynamicParams = false;

export function generateStaticParams() {
  return apiAreas.map((a) => ({ area: a.slug }));
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { area: slug } = await params;
  const area = areaByPath(apiReference, slug);
  if (!area) return {};
  const path = areaPath(API_BASE, area.slug);
  const title = `${area.title}: edytlab API reference`;
  const description = `${area.summary} ${area.countLabel[0].toUpperCase()}${area.countLabel.slice(1)}, with their return types and behaviour.`;
  return {
    title,
    description,
    alternates: pageAlternates(path),
    ...pageSocial(path, `${title} · edytlab`, description),
  };
}

export default async function ApiAreaPage({ params }: Props) {
  const { area: slug } = await params;
  const data = apiAreas.find((a) => a.slug === slug);
  const outline = areaByPath(apiReference, slug);
  if (!data || !outline) notFound();

  const showHeadings = data.sections.length > 1;

  return (
    <DocShell
      title={data.title}
      description={data.summary}
      parent={{ name: apiReference.name, path: API_BASE }}
      aside={<AreaOutlineAside reference={apiReference} area={outline} />}
    >
      <AreaJump reference={apiReference} area={outline} />

      {data.sections.some((s) => s.kind === "commands") ? (
        <p>
          Called through <code>tauri-bridge.ts</code>. Each returns <code>Promise&lt;T&gt;</code> and throws a
          string on Rust <code>Err(_)</code>.
        </p>
      ) : null}

      {data.sections.map((section) => {
        switch (section.kind) {
          case "commands":
            return (
              <CommandSection
                key={section.title}
                title={section.title}
                items={section.items}
                showHeading={showHeadings}
              />
            );
          case "events":
            return (
              <EventSection
                key={section.title}
                title={section.title}
                items={section.items}
                showHeading={showHeadings}
              />
            );
          case "types":
            return (
              <TypesSection
                key={section.title}
                title={section.title}
                code={section.code}
                showHeading={showHeadings}
              />
            );
        }
      })}

      <AreaPager reference={apiReference} area={outline} />
    </DocShell>
  );
}
