# edytlab — marketing website

Standalone Next.js 16 app for the edytlab marketing site. Lives outside the
pnpm workspace on purpose so Vercel can build it without monorepo gymnastics.

## Stack

- Next.js 16 (App Router) · React 18 · TypeScript 5
- Tailwind CSS 3 · shadcn/ui (locally vendored primitives)
- framer-motion for subtle entrance animations
- lucide-react icons

## Local development

```bash
cd website
pnpm install --ignore-workspace
pnpm dev
```

> The repo root has a pnpm workspace for the desktop app. `website/` is
> intentionally **not** part of that workspace — pass `--ignore-workspace` so
> pnpm resolves dependencies into `website/node_modules/` instead of hoisting
> them to the repo root. Vercel's build runs from the `website/` root and
> handles this automatically.

Open http://localhost:3000.

## Production build

```bash
pnpm build
pnpm start
```

## Quality gates

```bash
pnpm exec tsc --noEmit   # type-check
pnpm exec next lint      # lint
pnpm build               # production build
```

## Deployment (Vercel)

This folder is **not** part of the pnpm workspace, so Vercel can build it as a
standalone project:

1. Create a new Vercel project pointing at the `laadtushar/edytlab` repo.
2. Set the **Root Directory** to `website/`.
3. Framework preset is detected automatically (Next.js).
4. No environment variables are required.
5. Set the canonical domain (e.g. `edytlab.app`) under **Settings → Domains**;
   `metadataBase` in `lib/site.ts` should match it.

Pushes to `main` trigger production deploys; PRs get preview URLs
automatically.

## Editing copy

Headline / subtitle / keywords live in `lib/site.ts`. Page sections are split
under `components/landing/`. Each section is a server component that pulls in
small client-only motion wrappers where animation is needed.
