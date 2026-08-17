import Image from "next/image";
import Link from "next/link";
import { Github } from "lucide-react";

import { Button } from "@/components/ui/button";
import { siteConfig } from "@/lib/site";

export function SiteHeader() {
  return (
    <header className="fixed inset-x-0 top-0 z-40 border-b border-border/40 bg-background/70 backdrop-blur-md">
      <div className="container flex h-14 items-center justify-between">
        <Link href="/" className="flex items-center gap-2 font-semibold">
          <Image
            src="/logo.png"
            alt="edytlab logo"
            width={24}
            height={24}
            className="rounded"
            priority
          />
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
            href="/#interface"
            className="transition-colors hover:text-foreground"
          >
            Interface
          </Link>
          <Link
            href="/#tools"
            className="transition-colors hover:text-foreground"
          >
            Tools
          </Link>
          <Link
            href="/#how-it-works"
            className="transition-colors hover:text-foreground"
          >
            How it works
          </Link>
          <Link
            href="/blog"
            className="transition-colors hover:text-foreground"
          >
            Blog
          </Link>
          <Link
            href="/docs"
            className="transition-colors hover:text-foreground"
          >
            Docs
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
