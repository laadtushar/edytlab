# Curated lists

**Do not open these pull requests from this repository's tooling.** They are
the owner's to submit, from the owner's account, after reading each list's
current contributing rules. List formats and rules below were read from each
list's files on 2026-10-09 and can change.

**Links to use in every entry:** source <https://github.com/laadtushar/edytlab>,
site <https://edytlab.com>.

Entries are one line each. Do not add claims beyond them. In particular, no
mention of stem separation or transcription.

## Summary

| List | Section | Ready? |
|---|---|---|
| `ollama/ollama` README | Community Integrations, Productivity & Apps | Yes, after one end-to-end run with an Ollama model (see below) |
| `EndoTheDev/Awesome-Ollama` | Assistants (table) | Yes, same condition |
| `rust-unofficial/awesome-rust` | Applications, Audio and Music | **No.** Requires more than 50 stars or 2,000 crates.io downloads. The repo had 3 stars on 2026-10-09. |
| `tauri-apps/awesome-tauri` | none | **No fitting section.** Its README has Guides, Articles, Templates, Plugins and Integrations only. |
| `nodiscc/awesome-linuxaudio` | Software, Audio Editors | Optional. A partial fit: the list covers Linux audio production. Submissions go to the GitLab project. |

Before submitting to either Ollama list: run one real edit end to end against a
local model and write down which model and context size you used. Without
that, the claim "works with Ollama" rests only on Ollama being a supported
provider and on the Settings Test button; the one documented local run so far
failed on context size
([#395](https://github.com/laadtushar/edytlab/issues/395)).

---

## 1. ollama/ollama (README)

- **Where:** <https://github.com/ollama/ollama>, section "Community
  Integrations", subsection "Productivity & Apps".
- **Rule in the file:** "Want to add your project? Open a pull request."
- **Format:** `- [Name](url) - description`

Entry:

```markdown
- [edytlab](https://github.com/laadtushar/edytlab) - Open-source desktop audio editor where an agent edits through 93 tools; Ollama is a supported local provider
```

## 2. EndoTheDev/Awesome-Ollama

- **Where:** <https://github.com/EndoTheDev/Awesome-Ollama>, section
  "Assistants". Check the repository's contribution notes first; none were
  read.
- **Format:** a table row, `| Name/Link | Description | Install Type |`

Entry:

```markdown
| [edytlab](https://github.com/laadtushar/edytlab) | Open-source desktop audio editor driven by an LLM agent; Ollama is a supported local provider, so the chat can stay on your machine | Desktop installer (macOS, Windows, Linux) |
```

Keep the table's alphabetical order (the file lists AiLama, Amica and so on in
name order; place it accordingly).

## 3. rust-unofficial/awesome-rust (blocked until the popularity bar)

- **Where:** <https://github.com/rust-unofficial/awesome-rust>, "Applications",
  "Audio and Music". Not the "Libraries" section of the same name.
- **Rule** (`CONTRIBUTING.md`): "Accepted: `(stars > 50 | downloads > 2000)`".
  Sort alphabetically by the account/repo text. Add a CI badge after the
  description with the branch named. State the popularity metric in the PR.
- **Template:** `[ACCOUNT/REPO](https://github.com/ACCOUNT/REPO) - DESCRIPTION`
  (the project is not on crates.io, so no `[[CRATE](...)]` part).
- **Placement:** after `Glicol`, before `LargeModGames/spotatui` (alphabetical).
  Re-check against the file on the day.

Entry:

```markdown
* [laadtushar/edytlab](https://github.com/laadtushar/edytlab) - Tauri desktop audio editor driven by an LLM agent, with in-house pure-Rust DSP and a phase vocoder for time-stretch and pitch-shift [![build badge](https://github.com/laadtushar/edytlab/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/laadtushar/edytlab/actions)
```

## 4. tauri-apps/awesome-tauri (nothing to submit)

The README on the `dev` branch on 2026-10-09 contains Guides & Tutorials,
Articles, Templates, Plugins and Integrations. It has no applications section,
so there is no honest place for an app entry. Revisit if one returns, and read
its contributing rules then. A Tauri community showcase, if there is a current
one, is a better place; it was not checked.

## 5. nodiscc/awesome-linuxaudio (optional)

- **Where:** the GitLab project <https://gitlab.com/nodiscc/awesome-linuxaudio>
  is the primary; the GitHub repository is a mirror. Their CONTRIBUTING asks
  for a merge request editing `README.md`. Section: "Software", "Audio Editors".
- **Syntax** (from their CONTRIBUTING): `* [projectname](homepage) - Short
  project description, less than 250 characters`. The Debian-package marker is
  optional and does not apply (there is a `.deb` on GitHub, but it is not in
  Debian).
- **Fit:** it is a list of Linux audio production software, and edytlab is a
  cross-platform editor with Linux builds (`.deb`, AppImage). Linux caveat to
  know before submitting: API keys do not survive a reboot
  ([#394](https://github.com/laadtushar/edytlab/issues/394)).

Entry:

```markdown
* [edytlab](https://edytlab.com/) - Cross-platform audio editor you drive by chatting with an AI agent; the audio processing is local pure-Rust DSP. MIT, with .deb and AppImage builds
```

## Lists not checked

A generic "awesome audio" list with recent activity was not found by search,
and general LLM-application lists were not read. Do not submit to a list
without reading its current README and contributing rules first.
