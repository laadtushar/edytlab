import Link from "next/link";
import { Github } from "lucide-react";

import { Button } from "@/components/ui/button";
import { siteConfig } from "@/lib/site";

export function SiteHeader() {
  return (
    <header className="fixed inset-x-0 top-0 z-40 border-b border-border/40 bg-background/70 backdrop-blur-md">
      <div className="container flex h-14 items-center justify-between">
        <Link href="/" className="flex items-center gap-2 font-semibold">
          <span className="size-6 rounded bg-gradient-to-br from-primary to-primary/40" />
          edytlab
        </Link>
        <nav className="hidden items-center gap-6 text-sm text-muted-foreground sm:flex">
          <Link
            href="/#features"
            className="transition-colors hover:text-foreground"
          >
            Features
          </Link>
          <Link
            href="/#how-it-works"
            className="transition-colors hover:text-foreground"
          >
            How it works
          </Link>
          <Link
            href="/#faq"
            className="transition-colors hover:text-foreground"
          >
            FAQ
          </Link>
        </nav>
        <Button asChild size="sm" variant="outline">
          <Link
            href={siteConfig.github}
            target="_blank"
            rel="noopener noreferrer"
          >
            <Github className="size-4" />
            GitHub
          </Link>
        </Button>
      </div>
    </header>
  );
}
