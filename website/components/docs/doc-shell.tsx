"use client";

import { type ReactNode } from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";

import { SiteHeader } from "@/components/landing/site-header";
import { Footer } from "@/components/landing/footer";

const navGroups = [
  {
    label: "Getting Started",
    links: [
      { href: "/docs", label: "Overview" },
      { href: "/docs/getting-started", label: "Installation & Setup" },
    ],
  },
  {
    label: "User Guide",
    links: [
      { href: "/docs/user-guide", label: "Using edytlab" },
      { href: "/docs/tools", label: "Audio Tools Reference" },
    ],
  },
  {
    label: "Developer",
    links: [
      { href: "/docs/developer-guide", label: "Development Guide" },
      { href: "/docs/api-reference", label: "API Reference" },
    ],
  },
];

export function DocShell({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: ReactNode;
}) {
  const pathname = usePathname();

  return (
    <>
      <SiteHeader />
      <div className="min-h-screen pt-14">
        <div className="container mx-auto flex gap-0 lg:gap-8 xl:gap-12">
          {/* Sidebar */}
          <aside className="hidden w-56 shrink-0 lg:block">
            <div className="sticky top-20 overflow-y-auto pb-12 pt-8">
              {navGroups.map((group) => (
                <div key={group.label} className="mb-6">
                  <p className="mb-2 text-xs font-semibold uppercase tracking-widest text-muted-foreground">
                    {group.label}
                  </p>
                  <ul className="space-y-0.5">
                    {group.links.map((link) => {
                      const active = pathname === link.href;
                      return (
                        <li key={link.href}>
                          <Link
                            href={link.href}
                            className={`block rounded-md px-3 py-1.5 text-sm transition-colors ${
                              active
                                ? "bg-primary/10 font-medium text-primary"
                                : "text-muted-foreground hover:bg-secondary/60 hover:text-foreground"
                            }`}
                          >
                            {link.label}
                          </Link>
                        </li>
                      );
                    })}
                  </ul>
                </div>
              ))}
            </div>
          </aside>

          {/* Content */}
          <main className="min-w-0 flex-1 py-8 pb-24">
            <div className="mx-auto max-w-3xl">
              <h1 className="text-3xl font-bold tracking-tight sm:text-4xl">
                {title}
              </h1>
              {description && (
                <p className="mt-3 text-lg text-muted-foreground">
                  {description}
                </p>
              )}
              <div className="mt-8 space-y-0 text-foreground/90 [&_blockquote]:my-4 [&_blockquote]:rounded-lg [&_blockquote]:border [&_blockquote]:border-primary/30 [&_blockquote]:bg-primary/5 [&_blockquote]:px-5 [&_blockquote]:py-4 [&_blockquote]:text-sm [&_code]:rounded [&_code]:bg-secondary [&_code]:px-1.5 [&_code]:py-0.5 [&_code]:text-sm [&_code]:font-mono [&_h2]:mb-3 [&_h2]:mt-10 [&_h2]:text-xl [&_h2]:font-semibold [&_h3]:mb-2 [&_h3]:mt-6 [&_h3]:text-base [&_h3]:font-semibold [&_li]:mt-1 [&_ol]:mt-3 [&_ol]:list-decimal [&_ol]:pl-6 [&_p]:mt-4 [&_p]:leading-7 [&_pre]:my-4 [&_pre]:overflow-x-auto [&_pre]:rounded-lg [&_pre]:bg-secondary [&_pre]:p-4 [&_pre_code]:bg-transparent [&_pre_code]:p-0 [&_table]:my-4 [&_table]:w-full [&_table]:border-collapse [&_td]:border [&_td]:border-border/50 [&_td]:px-3 [&_td]:py-2 [&_td]:text-sm [&_th]:border [&_th]:border-border/50 [&_th]:bg-secondary/60 [&_th]:px-3 [&_th]:py-2 [&_th]:text-left [&_th]:text-sm [&_th]:font-semibold [&_ul]:mt-3 [&_ul]:list-disc [&_ul]:pl-6 [&_a]:text-primary [&_a]:underline [&_a]:underline-offset-4 [&_a:hover]:no-underline">
                {children}
              </div>
            </div>
          </main>
        </div>
      </div>
      <Footer />
    </>
  );
}
