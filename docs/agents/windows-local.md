# Windows local machine

The owner's Windows 11 checkout. Read before running git, pnpm or cargo there. A Linux cloud session follows "Cloud sessions" in `CLAUDE.md` instead.

This repo lives at `C:\Users\tusha\Work\Playground\Edytlab\edytlab` on Windows 11.

- **Always use the PowerShell tool for git commands** (`git`, `pnpm`, `cargo`). The Bash tool's git binary cannot resolve `/c/...` paths reliably on this machine.
- In PowerShell, prefix every command with `cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab";` or chain with `;`.
- The Bash tool works fine for read-only file operations (`find`, `ls`, path inspection) using `/c/Users/tusha/...` Unix-style paths, but **not for git or pnpm**.
- `pnpm` is not on the Bash PATH — always invoke it via PowerShell.
