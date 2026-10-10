# edytlab — marketing website

Standalone Next.js 16 app for the edytlab marketing site. Lives outside the
pnpm workspace on purpose so Vercel can build it without monorepo gymnastics.

## Stack

- Next.js 16 (App Router) · React 18 · TypeScript 5
- Tailwind CSS 3 · shadcn/ui (locally vendored primitives)
- GSAP (`gsap` + `@gsap/react`) for entrance and scroll animations, wrapped in `components/motion/`
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
pnpm test                # vitest (lib/*.test.ts)
pnpm typecheck           # tsc --noEmit
pnpm build               # production build
pnpm exec eslint .       # lint
```

CI's `website (test)` job runs `pnpm test` and `pnpm typecheck`. Lint is run with
`eslint` directly: Next.js 16 removed `next lint`, so the `pnpm lint` script
(`next lint`) no longer works.

## Deployment (Vercel)

This folder is **not** part of the pnpm workspace, so Vercel can build it as a
standalone project:

1. Create a new Vercel project pointing at the `laadtushar/edytlab` repo.
2. Set the **Root Directory** to `website/`.
3. Framework preset is detected automatically (Next.js).
4. No environment variables are required.
5. Set the canonical domain (`edytlab.com`, which redirects to `www.edytlab.com`)
   under **Settings → Domains**; `siteConfig.url` in `lib/site.ts` should match
   it, since `app/layout.tsx` builds `metadataBase` from it.

> Why the dashboard setting and not a root `vercel.json`: Vercel's Next.js
> framework detection runs against the `package.json` it finds at the project
> Root Directory, **before** `installCommand` executes. Pointing
> `outputDirectory` at `website/.next` from the repo root also disables the
> Next.js builder's SSR/ISR/API-route handling. Setting Root Directory to
> `website/` is the only path that supports the full Next.js feature set.

Pushes to `main` trigger production deploys; PRs get preview URLs
automatically. A commit that changes nothing under `website/` is not deployed:
`ignoreCommand` in `vercel.json` runs from the Root Directory, and
`git diff --quiet HEAD^ HEAD -- .` exits 0 (skip) when the last commit left
`website/` alone. The team is on Vercel's free plan, which allows 100
deployments a day, and most pushes to this repo only touch the app.

## Editing copy

Headline / subtitle / keywords live in `lib/site.ts`. Page sections are split
under `components/landing/`. Each section is a server component that pulls in
small client-only motion wrappers where animation is needed.
