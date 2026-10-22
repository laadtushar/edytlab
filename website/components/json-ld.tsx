import { serializeJsonLd } from "@/lib/seo-core";

/**
 * One block of structured data, as the `<script type="application/ld+json">`
 * a crawler reads. Rendered on the server, in the page's own HTML, so it
 * is there without scripts. `serializeJsonLd` escapes `<`, so no value in
 * `data` can end the tag early.
 */
export function JsonLd({ data }: { data: unknown }) {
  return (
    <script
      type="application/ld+json"
      dangerouslySetInnerHTML={{ __html: serializeJsonLd(data) }}
    />
  );
}
