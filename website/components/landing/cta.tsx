import Link from "next/link";
import { Apple, Download } from "lucide-react";

import { Button } from "@/components/ui/button";
import { siteConfig } from "@/lib/site";

import { FadeIn } from "./fade-in";

export function CTA() {
  return (
    <section className="border-t border-border/50 bg-secondary/20 py-24 md:py-32">
      <div className="container">
        <FadeIn className="mx-auto max-w-2xl text-center">
          <h2 className="text-balance text-3xl font-bold tracking-tight sm:text-4xl md:text-5xl">
            Ready to edit differently?
          </h2>
          <p className="mx-auto mt-4 max-w-xl text-pretty text-lg text-muted-foreground">
            Free in BYO-key mode. Your audio stays on your machine. Download,
            plug in an API key, and describe your first edit.
          </p>
          <div className="mt-10 flex flex-col items-center justify-center gap-3 sm:flex-row">
            <Button asChild size="lg" className="glow w-full sm:w-auto">
              <Link href={siteConfig.releases}>
                <Apple className="size-4" />
                Download for Mac
              </Link>
            </Button>
            <Button
              asChild
              size="lg"
              variant="outline"
              className="w-full sm:w-auto"
            >
              <Link href={siteConfig.releases}>
                <Download className="size-4" />
                Download for Windows
              </Link>
            </Button>
          </div>
          <p className="mt-4 text-xs text-muted-foreground">
            Signed installers · Universal Mac (Apple Silicon + Intel) · Windows 10/11
          </p>
        </FadeIn>
      </div>
    </section>
  );
}
