import Link from "next/link";
import { Github } from "lucide-react";

import { Separator } from "@/components/ui/separator";
import { siteConfig } from "@/lib/site";

export function Footer() {
  return (
    <footer className="border-t border-border/50 bg-secondary/20">
      <div className="container py-12">
        <div className="flex flex-col items-start justify-between gap-6 md:flex-row md:items-center">
          <div>
            <div className="text-lg font-semibold">edytlab</div>
            <p className="mt-1 text-sm text-muted-foreground">
              Describe it. Get pro-grade audio edits.
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-6 text-sm">
            <Link
              href={siteConfig.github}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex items-center gap-1.5 text-muted-foreground transition-colors hover:text-foreground"
            >
              <Github className="size-4" /> GitHub
            </Link>
            <Link
              href={siteConfig.designSpec}
              target="_blank"
              rel="noopener noreferrer"
              className="text-muted-foreground transition-colors hover:text-foreground"
            >
              Design spec
            </Link>
            <Link
              href="/blog"
              className="text-muted-foreground transition-colors hover:text-foreground"
            >
              Blog
            </Link>
            <Link
              href="/docs"
              className="text-muted-foreground transition-colors hover:text-foreground"
            >
              Docs
            </Link>
            <Link
              href="/changelog"
              className="text-muted-foreground transition-colors hover:text-foreground"
            >
              Changelog
            </Link>
            <Link
              href="/privacy"
              className="text-muted-foreground transition-colors hover:text-foreground"
            >
              Privacy
            </Link>
            <Link
              href="/terms"
              className="text-muted-foreground transition-colors hover:text-foreground"
            >
              Terms
            </Link>
            {/* Social handles intentionally omitted until accounts exist:
            <Link href="https://twitter.com/edytlab">Twitter</Link>
            <Link href="https://www.linkedin.com/company/edytlab">LinkedIn</Link>
            */}
          </div>
        </div>
        <Separator className="my-8" />
        <p className="text-xs text-muted-foreground" suppressHydrationWarning>
          © {new Date().getFullYear()} edytlab. Audio stays on your machine.
        </p>
      </div>
    </footer>
  );
}
