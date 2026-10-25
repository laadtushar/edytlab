# Architecture decision records

One file per decision that a future change could otherwise reopen by accident. Each says what was
decided, why, what was considered and rejected, and how we will know it worked. A record is never
edited to say something else: a later decision that replaces it is a new record that names the old
one as superseded.

They came out of the architecture review of 2026-10-22, which looked for shallow modules (an
interface nearly as wide as the code behind it) in the parts of the code that kept breaking. The
review's vocabulary: a **module** has an **interface** and an **implementation**; it is **deep**
when a small interface hides a lot; a **seam** is where one module can be swapped for another
(an **adapter**); **locality** means the rules for one concept live in one place; **leverage**
means one interface serves many callers.

| # | Decision | Status |
|---|---|---|
| [0001](0001-session-state-in-the-window.md) | The window changes the session through one module, built on TanStack Query | Accepted |
| [0002](0002-plan-gate-is-a-oneshot.md) | The plan gate is one value built on a oneshot channel | Accepted |
| [0003](0003-rendering-a-node.md) | One module renders a node and returns what it left out | Accepted |
| [0004](0004-time-edits.md) | A time edit is a pure function over the session state | Accepted |
| [0005](0005-provider-settings.md) | Provider settings are saved in one call by one module | Accepted |
| [0006](0006-the-open-project.md) | The open project is one module that owns its locks | Accepted |
| [0007](0007-a-turn.md) | A turn is split at the model exchange, behind a port with two adapters | Accepted |
| [0008](0008-project-audio-files.md) | The project's own audio files are one content-addressed store | Accepted, deferred |
| [0009](0009-settings-and-secrets-storage.md) | Secrets are one keychain item, and settings are a file | Accepted |

Order of work: 0001 (its bug fix first), 0002, 0003, 0004, 0005, 0006 in stages, 0007 after 0002,
0008 when the stem writers (#385) need it. Terms are defined in [`GLOSSARY.md`](../../GLOSSARY.md).
