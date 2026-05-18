# Plugin System & Pre-installed Skills Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship 8 pre-installed audio skills with the app and add a plugin manifest format + install-from-GitHub command, so users get immediately useful AI behaviors out of box and can extend edytlab with community plugins.

**Architecture:** Pre-installed skills are `.md` files bundled as Tauri resources and auto-copied to `~/.edytlab/skills/` on first launch (only if the directory is empty). Community plugins are GitHub repos with an `edytlab-plugin.json` manifest that lists skill files and optional MCP servers; a new Tauri command downloads and installs them.

**Tech Stack:** Rust (Tauri 2, crates/skills), React 19 + TypeScript (Settings UI), JSON (plugin manifest)

---

## Skill file format (reference for all tasks)

Each skill `.md` in `apps/desktop/src-tauri/resources/skills/` follows this format:

```markdown
---
name: <filename-stem>
description: <one line shown in skill list>
trigger: keywords
keywords: [word1, word2, word3]
enabled: true
---

## Skill Title

Body text...
```

- `name` MUST match the filename stem exactly
- `trigger`: `always` | `keywords` | `regex`; if omitted defaults to `always`
- `keywords`: required when `trigger: keywords`

---

## Task 1: Pre-installed skill files

**Files:**
- Create: `apps/desktop/src-tauri/resources/skills/podcast-cleanup.md`
- Create: `apps/desktop/src-tauri/resources/skills/music-mix.md`
- Create: `apps/desktop/src-tauri/resources/skills/vocal-chain.md`
- Create: `apps/desktop/src-tauri/resources/skills/silence-cleaner.md`
- Create: `apps/desktop/src-tauri/resources/skills/noise-reducer.md`
- Create: `apps/desktop/src-tauri/resources/skills/loudness-master.md`
- Create: `apps/desktop/src-tauri/resources/skills/dialog-enhancer.md`
- Create: `apps/desktop/src-tauri/resources/skills/export-guide.md`

- [ ] **Step 1: Create `apps/desktop/src-tauri/resources/skills/` directory**

```bash
mkdir -p apps/desktop/src-tauri/resources/skills
```

- [ ] **Step 2: Create `podcast-cleanup.md`**

```markdown
---
name: podcast-cleanup
description: Podcast and voice recording cleanup workflow
trigger: keywords
keywords: [podcast, voice, interview, speech, dialogue, narration, recording, vocal]
enabled: true
---

## Podcast Cleanup Workflow

For podcast or voice recordings, apply these steps in order:

1. **Noise reduction**: `noise_reduction` — removes background hiss/hum
2. **Noise gate**: `noise_gate` threshold=-50dB, attack_ms=10, release_ms=100 — silences gaps between speech
3. **EQ**: `eq` — boost 2–4 kHz for presence, cut 200–400 Hz for clarity, cut below 80 Hz for rumble
4. **De-esser**: `de_esser` frequency_hz=7000, threshold_db=-20 — tames harsh S sounds
5. **Compression**: `compressor` threshold=-20dB, ratio=3.0, attack_ms=10, release_ms=60 — consistent levels
6. **Normalize**: `normalize` target_db=-1.0 — broadcast-ready peak

Ask the user before applying each step. Suggest using `silence_finder` first to identify long silences.
```

- [ ] **Step 3: Create `music-mix.md`**

```markdown
---
name: music-mix
description: Music mixing and mastering workflow
trigger: keywords
keywords: [music, mix, mixing, master, mastering, song, track, beat, instrument, band]
enabled: true
---

## Music Mixing Workflow

For music tracks, suggested workflow:

1. **Gain staging**: `gain` — set each track so the mix peaks around -6 dBFS before processing
2. **EQ**: `eq` — cut competing frequencies between instruments; each instrument owns a frequency range
3. **Compression**: `compressor` — tighten transients; gentle settings (2:1 ratio) for glue compression
4. **Reverb**: `reverb` room_size=0.4–0.7, wet=0.15–0.3 — add space; keep wet low for upfront sounds
5. **Stereo width**: `stereo_widener` — widen pads/synths, keep kick/bass mono (width≤0.5 for low frequencies)
6. **Limiter**: `limiter` ceiling_db=-0.3 — prevent digital clipping on the final output

Use `mix_to_new_track` to commit a stem mix. Use `plot_spectrum` to compare frequency balance.
```

- [ ] **Step 4: Create `vocal-chain.md`**

```markdown
---
name: vocal-chain
description: Professional vocal processing chain
trigger: keywords
keywords: [vocal, vocals, singer, singing, lead, harmony, chorus, verse]
enabled: true
---

## Vocal Processing Chain

Standard professional vocal chain order:

1. **High-pass filter**: `high_pass_filter` cutoff_hz=100 — remove low-end rumble and plosives
2. **De-esser**: `de_esser` frequency_hz=7500, threshold_db=-18 — control sibilance before compression
3. **Compression**: `compressor` threshold=-18dB, ratio=4.0, attack_ms=5, release_ms=50 — control dynamics
4. **EQ**: `eq` — boost air (12–16 kHz), boost presence (3–5 kHz), cut mud (300–500 Hz)
5. **Saturation/warmth**: optional `distortion` with very low drive (0.1–0.2) for analog warmth
6. **Reverb**: `reverb` room_size=0.3, wet=0.2 — place vocal in the mix space
7. **Limiter**: `limiter` ceiling_db=-1.0 — catch peaks

Tune compression attack to let initial consonants through (slightly longer attack = more punch).
```

- [ ] **Step 5: Create `silence-cleaner.md`**

```markdown
---
name: silence-cleaner
description: Find and remove silent regions
trigger: keywords
keywords: [silence, silent, gaps, quiet, pause, pauses, dead air, remove silence, clean up]
enabled: true
---

## Silence Cleaning Workflow

To identify and remove silence:

1. **Find silences**: `silence_finder` threshold_db=-50, min_silence_ms=500 — lists all silent regions
2. **Review**: Show user the regions before removing
3. **Remove**: `truncate_silence` threshold_db=-50, min_silence_ms=500 — removes silent regions destructively

Adjust threshold_db based on the recording's noise floor:
- Clean studio recording: -60 dBFS
- Typical room noise: -50 dBFS  
- Noisy environment: -40 dBFS

For podcasts, prefer -50 dBFS with 300–500ms minimum to preserve natural pauses.
```

- [ ] **Step 6: Create `noise-reducer.md`**

```markdown
---
name: noise-reducer
description: Noise reduction and audio cleanup
trigger: keywords
keywords: [noise, hiss, hum, buzz, background, static, clean, cleanup, remove noise]
enabled: true
---

## Noise Reduction Workflow

1. **Analyze**: `silence_finder` — find a region that is pure noise (no signal) to understand the noise floor
2. **Reduce**: `noise_reduction` — applies spectral subtraction
3. **Gate**: `noise_gate` threshold_db=-55 — gates remaining noise below the signal

For severe noise:
- Apply noise_reduction twice with lower strength rather than once with high strength
- Avoid over-processing: artifacts (metallic/robotic sound) are worse than moderate noise

After noise reduction, apply EQ to restore any high-frequency detail that was attenuated.
```

- [ ] **Step 7: Create `loudness-master.md`**

```markdown
---
name: loudness-master
description: Loudness normalization to broadcast/streaming standards
trigger: keywords
keywords: [loudness, lufs, loud, quiet, normalize, volume, level, streaming, broadcast, spotify, youtube]
enabled: true
---

## Loudness Mastering

Platform target levels (integrated LUFS):
- **Streaming** (Spotify, Apple Music, YouTube Music): -14 LUFS
- **YouTube video**: -14 LUFS  
- **Podcast**: -16 LUFS
- **Broadcast TV**: -23 LUFS (EBU R128)
- **Film**: -24 LUFS

Workflow:
1. **Dynamic range check**: `plot_spectrum` — confirm the mix isn't over-compressed
2. **Normalize peak**: `normalize` target_db=-1.0 — set ceiling first
3. **Limit**: `limiter` ceiling_db=-1.0 — ensure true peak compliance
4. **Leveler**: `leveler` target_db=-14 — match RMS/perceived loudness to target

Note: `leveler` uses RMS windowing, not true LUFS measurement. For broadcast-critical work, verify with an external LUFS meter after export.
```

- [ ] **Step 8: Create `dialog-enhancer.md`**

```markdown
---
name: dialog-enhancer
description: Dialog and interview clarity enhancement
trigger: keywords
keywords: [dialog, dialogue, interview, clarity, intelligibility, speaker, speakers, conversation]
enabled: true
---

## Dialog Enhancement

For interview and dialog recordings:

1. **High-pass**: `high_pass_filter` cutoff_hz=120 — remove handling noise and low rumble
2. **Noise reduction**: `noise_reduction` — clean background
3. **Mid-frequency EQ**: `eq` — boost 1–4 kHz for intelligibility (where consonants live)
4. **Compression**: `compressor` threshold=-20dB, ratio=3.0, attack_ms=15 — even out speaker level differences
5. **Noise gate**: `noise_gate` threshold_db=-45 — clean between sentences

For multi-speaker interviews:
- Process each speaker track separately before mixing
- Use `stereo_to_mono` if needed for consistent panning
- Use `mix_to_new_track` to create the final combined track
```

- [ ] **Step 9: Create `export-guide.md`**

```markdown
---
name: export-guide
description: Audio export workflow and format guidance
trigger: keywords
keywords: [export, save, download, output, render, finish, done, final, wav, flac, mp3, format]
enabled: true
---

## Export Guide

When the user wants to export or save audio:

1. **Single track export**: `export_multiple` track_indices=[N], output_dir="<user's chosen dir>", format="wav"
2. **Mixed export**: `mix_to_new_track` first → then export the new mixed track
3. **Multiple tracks**: `export_multiple` with all desired track_indices

File format guidance:
- **WAV**: lossless, large files — best for archiving or further processing
- **WAV 24-bit**: preferred for professional delivery
- For lossy formats (MP3, AAC): export WAV first, convert with an external tool

Before export, recommend:
- `normalize` target_db=-1.0 for consistent peak level
- `limiter` ceiling_db=-0.3 for true peak compliance

Ask the user for their target platform to give specific loudness recommendations.
```

- [ ] **Step 10: Verify all 8 files exist**

```bash
ls apps/desktop/src-tauri/resources/skills/
```
Expected: 8 `.md` files.

- [ ] **Step 11: Commit**

```bash
git add apps/desktop/src-tauri/resources/skills/
git commit -m "feat(skills): 8 pre-installed audio skill files

podcast-cleanup, music-mix, vocal-chain, silence-cleaner, noise-reducer,
loudness-master, dialog-enhancer, export-guide

https://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 2: Bundle skills as Tauri resources + auto-install on first launch

**Files:**
- Modify: `apps/desktop/src-tauri/tauri.conf.json` (add resources entry)
- Modify: `apps/desktop/src-tauri/src/commands.rs` (add `install_bundled_skills` command)
- Modify: `apps/desktop/src-tauri/src/lib.rs` (register command)
- Modify: `apps/desktop/src/App.tsx` (call command on mount)
- Modify: `apps/desktop/src/tauri-bridge.ts` (add bridge function)

- [ ] **Step 1: Add resources entry to tauri.conf.json**

In `apps/desktop/src-tauri/tauri.conf.json`, find the `"resources"` object and add the skills entry:

Current:
```json
"resources": {
  "resources/templates/*": "templates/"
}
```

New:
```json
"resources": {
  "resources/templates/*": "templates/",
  "resources/skills/*": "bundled-skills/"
}
```

- [ ] **Step 2: Add `install_bundled_skills` Tauri command**

In `apps/desktop/src-tauri/src/commands.rs`, add this command (near other utility commands):

```rust
#[tauri::command]
pub fn install_bundled_skills(app: tauri::AppHandle) -> CmdResult<usize> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?;
    let bundled_dir = resource_dir.join("bundled-skills");

    let home = dirs::home_dir().ok_or("cannot locate home directory")?;
    let skills_dir = home.join(".edytlab").join("skills");

    // If skills dir already has .md files, don't overwrite — user may have customised.
    if skills_dir.exists() {
        let has_skills = std::fs::read_dir(&skills_dir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .any(|e| {
                e.path()
                    .extension()
                    .and_then(|s| s.to_str())
                    == Some("md")
            });
        if has_skills {
            return Ok(0);
        }
    }

    std::fs::create_dir_all(&skills_dir).map_err(|e| e.to_string())?;

    if !bundled_dir.exists() {
        return Ok(0); // dev mode: bundled-skills/ not present
    }

    let mut count = 0usize;
    for entry in std::fs::read_dir(&bundled_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let src = entry.path();
        if src.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let dst = skills_dir.join(entry.file_name());
        std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;
        count += 1;
    }
    Ok(count)
}
```

Note: `dirs` crate is already used for `~/.edytlab` paths elsewhere; check `Cargo.toml` for existing usage. If not present, add `dirs = "5"` to `apps/desktop/src-tauri/Cargo.toml`.

- [ ] **Step 3: Register command in lib.rs**

In `apps/desktop/src-tauri/src/lib.rs`, find the `.invoke_handler(tauri::generate_handler![...])` call and add `commands::install_bundled_skills`.

- [ ] **Step 4: Add bridge function to tauri-bridge.ts**

In `apps/desktop/src/tauri-bridge.ts` (or wherever Tauri invoke wrappers live), add:

```typescript
export async function installBundledSkills(): Promise<number> {
  return invoke<number>('install_bundled_skills');
}
```

- [ ] **Step 5: Call on app mount in App.tsx**

In `apps/desktop/src/App.tsx`, add to the initial `useEffect` (the one with `[]` deps):

```typescript
import { installBundledSkills } from './tauri-bridge';

// Inside the [] useEffect:
installBundledSkills().catch(() => {
  // Non-fatal: bundled skills not available in dev mode
});
```

- [ ] **Step 6: Build check**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo +1.88-x86_64-pc-windows-msvc build -p desktop 2>&1 | tail -5
pnpm --filter @edytlab/desktop exec tsc --noEmit
```

- [ ] **Step 7: Commit**

```
feat(app): auto-install 8 bundled skills to ~/.edytlab/skills/ on first launch

https://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd
```

---

## Task 3: Plugin manifest format + Rust parser

**Files:**
- Modify: `crates/skills/Cargo.toml` (add serde, serde_json)
- Create: `crates/skills/src/plugin.rs`
- Modify: `crates/skills/src/lib.rs` (add `pub mod plugin;`)

- [ ] **Step 1: Add deps to `crates/skills/Cargo.toml`**

```toml
[dependencies]
regex = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tracing = "0.1"
```

- [ ] **Step 2: Create `crates/skills/src/plugin.rs`**

```rust
//! Plugin manifest format: `edytlab-plugin.json`
//!
//! A plugin is a directory containing an `edytlab-plugin.json` manifest
//! that enumerates skill files, optional MCP server entries, and optional
//! agent profile files. The installer copies these components into the
//! appropriate user directories.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct PluginManifest {
    /// Kebab-case plugin id. Must be unique across installed plugins.
    pub name: String,
    /// Semver string.
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    /// Relative paths to skill `.md` files inside the plugin directory.
    #[serde(default)]
    pub skills: Vec<String>,
    /// MCP server entries (same format as `~/.edytlab/mcp.json`).
    #[serde(default, rename = "mcpServers")]
    pub mcp_servers: HashMap<String, Value>,
    /// Relative paths to agent profile `.md` files.
    #[serde(default)]
    pub agents: Vec<String>,
}

#[derive(Debug)]
pub struct InstallReport {
    pub name: String,
    pub version: String,
    pub skills_installed: Vec<PathBuf>,
    pub agents_installed: Vec<PathBuf>,
    pub mcp_keys: Vec<String>,
}

impl PluginManifest {
    /// Load and parse `edytlab-plugin.json` from `manifest_path`.
    pub fn load(manifest_path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(manifest_path)
            .map_err(|e| format!("read {}: {e}", manifest_path.display()))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("parse {}: {e}", manifest_path.display()))
    }

    /// Install skill files from `plugin_dir` into `skills_dir`.
    /// Returns list of destination paths written.
    pub fn install_skills(
        &self,
        plugin_dir: &Path,
        skills_dir: &Path,
    ) -> Result<Vec<PathBuf>, String> {
        std::fs::create_dir_all(skills_dir)
            .map_err(|e| format!("create skills dir: {e}"))?;

        let mut installed = Vec::new();
        for rel in &self.skills {
            let src = plugin_dir.join(rel);
            let filename = src
                .file_name()
                .ok_or_else(|| format!("no filename in skill path `{rel}`"))?;
            let dst = skills_dir.join(filename);
            std::fs::copy(&src, &dst)
                .map_err(|e| format!("copy {} → {}: {e}", src.display(), dst.display()))?;
            installed.push(dst);
        }
        Ok(installed)
    }

    /// Install agent profile files from `plugin_dir` into `agents_dir`.
    pub fn install_agents(
        &self,
        plugin_dir: &Path,
        agents_dir: &Path,
    ) -> Result<Vec<PathBuf>, String> {
        if self.agents.is_empty() {
            return Ok(vec![]);
        }
        std::fs::create_dir_all(agents_dir)
            .map_err(|e| format!("create agents dir: {e}"))?;

        let mut installed = Vec::new();
        for rel in &self.agents {
            let src = plugin_dir.join(rel);
            let filename = src
                .file_name()
                .ok_or_else(|| format!("no filename in agent path `{rel}`"))?;
            let dst = agents_dir.join(filename);
            std::fs::copy(&src, &dst)
                .map_err(|e| format!("copy {} → {}: {e}", src.display(), dst.display()))?;
            installed.push(dst);
        }
        Ok(installed)
    }
}

#[cfg(test)]
mod tests {
    use super::PluginManifest;
    use std::fs;
    use tempfile::TempDir;

    fn write_manifest(dir: &TempDir, content: &str) -> std::path::PathBuf {
        let path = dir.path().join("edytlab-plugin.json");
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn parse_minimal_manifest() {
        let dir = TempDir::new().unwrap();
        let path = write_manifest(
            &dir,
            r#"{"name":"test-plugin","version":"1.0.0"}"#,
        );
        let m = PluginManifest::load(&path).unwrap();
        assert_eq!(m.name, "test-plugin");
        assert_eq!(m.version, "1.0.0");
        assert!(m.skills.is_empty());
    }

    #[test]
    fn parse_full_manifest() {
        let dir = TempDir::new().unwrap();
        let path = write_manifest(
            &dir,
            r#"{
                "name":"podcast-toolkit",
                "version":"2.1.0",
                "description":"Podcast production skills",
                "skills":["skills/podcast-cleanup.md"],
                "mcpServers":{"whisper":{"command":"npx","args":["whisper-mcp"]}},
                "agents":["agents/podcast-producer.md"]
            }"#,
        );
        let m = PluginManifest::load(&path).unwrap();
        assert_eq!(m.name, "podcast-toolkit");
        assert_eq!(m.skills.len(), 1);
        assert_eq!(m.mcp_servers.len(), 1);
        assert_eq!(m.agents.len(), 1);
    }

    #[test]
    fn install_skills_copies_files() {
        let plugin_dir = TempDir::new().unwrap();
        let skills_src_dir = plugin_dir.path().join("skills");
        fs::create_dir_all(&skills_src_dir).unwrap();
        fs::write(
            skills_src_dir.join("my-skill.md"),
            "---\nname: my-skill\n---\nbody",
        ).unwrap();

        let manifest = PluginManifest {
            name: "test".into(),
            version: "1.0.0".into(),
            description: None,
            author: None,
            skills: vec!["skills/my-skill.md".into()],
            mcp_servers: Default::default(),
            agents: vec![],
        };

        let dst_dir = TempDir::new().unwrap();
        let installed = manifest
            .install_skills(plugin_dir.path(), dst_dir.path())
            .unwrap();
        assert_eq!(installed.len(), 1);
        assert!(dst_dir.path().join("my-skill.md").exists());
    }
}
```

- [ ] **Step 3: Add `pub mod plugin;` to `crates/skills/src/lib.rs`**

Add near the top of the file (after the module-level doc comment, before other use statements):

```rust
pub mod plugin;
```

- [ ] **Step 4: Run tests**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo +1.88-x86_64-pc-windows-msvc test --package skills -- --nocapture 2>&1 | tail -20
```

Expected: all tests pass including the 3 new plugin tests.

- [ ] **Step 5: Clippy check**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo +1.88-x86_64-pc-windows-msvc clippy --package skills --all-targets -- -D warnings
```

- [ ] **Step 6: Commit**

```
feat(skills): plugin manifest format — PluginManifest parser + installer

edytlab-plugin.json: name, version, skills[], mcpServers{}, agents[]
PluginManifest::load() + install_skills() + install_agents()
3 unit tests (parse minimal, parse full, install copies files)

https://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd
```

---

## Task 4: `install_plugin` Tauri command

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml` (add `reqwest` with `blocking` feature if not present)
- Modify: `apps/desktop/src-tauri/src/commands.rs` (add `install_plugin` command)
- Modify: `apps/desktop/src-tauri/src/lib.rs` (register command)
- Modify: `apps/desktop/src/tauri-bridge.ts` (add bridge)
- Test: `apps/desktop/src/__tests__/installPlugin.test.ts` (mock test)

- [ ] **Step 1: Check/add reqwest dep**

In `apps/desktop/src-tauri/Cargo.toml`, check if `reqwest` is present. If not, add:

```toml
reqwest = { version = "0.12", features = ["blocking", "json"] }
zip = "2"
```

Both are needed: `reqwest` for downloading, `zip` for extracting the GitHub archive.

- [ ] **Step 2: Add helper functions to commands.rs**

Add these private helpers before the `install_plugin` command:

```rust
fn edytlab_skills_dir() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("cannot locate home directory")?;
    Ok(home.join(".edytlab").join("skills"))
}

fn edytlab_agents_dir() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("cannot locate home directory")?;
    Ok(home.join(".edytlab").join("agents"))
}

fn download_github_archive(repo: &str) -> Result<std::path::PathBuf, String> {
    // repo = "org/name"
    let url = format!("https://github.com/{repo}/archive/refs/heads/main.zip");
    let response = reqwest::blocking::get(&url)
        .map_err(|e| format!("fetch {url}: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("HTTP {} fetching {url}", response.status()));
    }
    let bytes = response.bytes().map_err(|e| e.to_string())?;

    let tmp_dir = std::env::temp_dir().join(format!("edytlab-plugin-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;

    let zip_path = tmp_dir.join("plugin.zip");
    std::fs::write(&zip_path, &bytes).map_err(|e| e.to_string())?;

    let extract_dir = tmp_dir.join("extracted");
    std::fs::create_dir_all(&extract_dir).map_err(|e| e.to_string())?;

    let zip_file = std::fs::File::open(&zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(zip_file).map_err(|e| e.to_string())?;
    archive.extract(&extract_dir).map_err(|e| e.to_string())?;

    // GitHub zips extract to a single top-level dir: <repo-name>-main/
    let plugin_dir = std::fs::read_dir(&extract_dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .find(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .ok_or("extracted zip has no top-level directory")?;

    Ok(plugin_dir)
}
```

- [ ] **Step 3: Add `install_plugin` command**

```rust
#[tauri::command]
pub fn install_plugin(source: String) -> CmdResult<serde_json::Value> {
    let plugin_dir = if source.starts_with("github:") {
        let repo = source.trim_start_matches("github:");
        download_github_archive(repo)?
    } else if source.starts_with("local:") {
        std::path::PathBuf::from(source.trim_start_matches("local:"))
    } else {
        return Err(format!(
            "unknown source `{source}`. Use `github:org/repo` or `local:/path/to/dir`"
        ));
    };

    let manifest_path = plugin_dir.join("edytlab-plugin.json");
    if !manifest_path.exists() {
        return Err(format!(
            "no edytlab-plugin.json found in `{}`",
            plugin_dir.display()
        ));
    }

    let manifest = skills::plugin::PluginManifest::load(&manifest_path)?;

    let skills_installed = manifest.install_skills(&plugin_dir, &edytlab_skills_dir()?)?;
    let agents_installed = manifest.install_agents(&plugin_dir, &edytlab_agents_dir()?)?;
    let mcp_keys: Vec<String> = manifest.mcp_servers.keys().cloned().collect();

    let summary = format!(
        "Installed plugin '{}' v{}: {} skill(s), {} agent(s)",
        manifest.name,
        manifest.version,
        skills_installed.len(),
        agents_installed.len(),
    );

    Ok(serde_json::json!({
        "name": manifest.name,
        "version": manifest.version,
        "skills_installed": skills_installed.len(),
        "agents_installed": agents_installed.len(),
        "mcp_keys": mcp_keys,
        "summary": summary,
    }))
}
```

- [ ] **Step 4: Register in lib.rs**

Add `commands::install_plugin` to the `generate_handler![]` macro in `lib.rs`.

- [ ] **Step 5: Add bridge function to tauri-bridge.ts**

```typescript
export interface PluginInstallResult {
  name: string;
  version: string;
  skills_installed: number;
  agents_installed: number;
  mcp_keys: string[];
  summary: string;
}

export async function installPlugin(source: string): Promise<PluginInstallResult> {
  return invoke<PluginInstallResult>('install_plugin', { source });
}
```

- [ ] **Step 6: Build check**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo +1.88-x86_64-pc-windows-msvc build -p desktop 2>&1 | tail -10
pnpm --filter @edytlab/desktop exec tsc --noEmit
```

- [ ] **Step 7: Commit**

```
feat(app): install_plugin Tauri command — install from github:org/repo or local: path

Downloads GitHub zip, extracts, reads edytlab-plugin.json, copies skills + agents.
Uses skills::plugin::PluginManifest for parsing and file installation.

https://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd
```

---

## Task 5: Plugin management UI in Settings

**Files:**
- Modify: `apps/desktop/src/components/Settings.tsx` (add Plugins tab)
- Test: `apps/desktop/src/__tests__/Settings.plugins.test.tsx`

- [ ] **Step 1: Read Settings.tsx to understand existing tab structure**

Before making changes, read the file to understand the current tab implementation (look for existing tabs like "General", "MCP Servers", etc.).

- [ ] **Step 2: Add Plugins tab content**

Add a new tab panel in the Settings modal. The Plugins tab should show:

```tsx
// Plugins tab content — add to Settings.tsx

const [pluginSource, setPluginSource] = useState('');
const [pluginInstalling, setPluginInstalling] = useState(false);
const [pluginResult, setPluginResult] = useState<string | null>(null);

async function handleInstallPlugin() {
  if (!pluginSource.trim()) return;
  setPluginInstalling(true);
  setPluginResult(null);
  try {
    const result = await installPlugin(pluginSource.trim());
    setPluginResult(result.summary);
  } catch (e) {
    setPluginResult(`Error: ${e}`);
  } finally {
    setPluginInstalling(false);
  }
}

// JSX (adapt to match the existing Settings modal style):
<div data-testid="plugins-tab">
  <h3 className="text-sm font-semibold text-zinc-300 mb-2">Install Plugin</h3>
  <p className="text-xs text-zinc-500 mb-3">
    Source format: <code>github:org/repo</code> or <code>local:/path/to/dir</code>
  </p>
  <div className="flex gap-2">
    <input
      data-testid="plugin-source-input"
      className="flex-1 bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm text-zinc-100 placeholder-zinc-500 focus:outline-none focus:border-zinc-500"
      placeholder="github:edytlab-community/podcast-toolkit"
      value={pluginSource}
      onChange={e => setPluginSource(e.target.value)}
      onKeyDown={e => e.key === 'Enter' && handleInstallPlugin()}
    />
    <button
      data-testid="plugin-install-btn"
      className="px-4 py-2 bg-zinc-700 hover:bg-zinc-600 text-sm text-zinc-100 rounded disabled:opacity-50"
      onClick={handleInstallPlugin}
      disabled={pluginInstalling || !pluginSource.trim()}
    >
      {pluginInstalling ? 'Installing…' : 'Install'}
    </button>
  </div>
  {pluginResult && (
    <p data-testid="plugin-result" className="mt-2 text-xs text-zinc-400">
      {pluginResult}
    </p>
  )}
</div>
```

- [ ] **Step 3: Write test**

Create `apps/desktop/src/__tests__/Settings.plugins.test.tsx`:

```tsx
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { vi, describe, it, expect, beforeEach } from 'vitest';

// Mock tauri-bridge
vi.mock('../tauri-bridge', () => ({
  installPlugin: vi.fn(),
}));

import { installPlugin } from '../tauri-bridge';
import Settings from '../components/Settings';

describe('Settings plugins tab', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('shows plugin install input and button', () => {
    render(<Settings open={true} onClose={() => {}} />);
    // Navigate to plugins tab if tabbed — look for the tab button
    const pluginsTab = screen.queryByText(/plugins/i);
    if (pluginsTab) fireEvent.click(pluginsTab);
    expect(screen.getByTestId('plugin-source-input')).toBeInTheDocument();
    expect(screen.getByTestId('plugin-install-btn')).toBeInTheDocument();
  });

  it('calls installPlugin with source and shows result', async () => {
    (installPlugin as ReturnType<typeof vi.fn>).mockResolvedValue({
      name: 'test-plugin',
      version: '1.0.0',
      skills_installed: 2,
      agents_installed: 0,
      mcp_keys: [],
      summary: "Installed plugin 'test-plugin' v1.0.0: 2 skill(s), 0 agent(s)",
    });

    render(<Settings open={true} onClose={() => {}} />);
    const pluginsTab = screen.queryByText(/plugins/i);
    if (pluginsTab) fireEvent.click(pluginsTab);

    fireEvent.change(screen.getByTestId('plugin-source-input'), {
      target: { value: 'github:edytlab-community/test-plugin' },
    });
    fireEvent.click(screen.getByTestId('plugin-install-btn'));

    expect(installPlugin).toHaveBeenCalledWith('github:edytlab-community/test-plugin');

    await waitFor(() => {
      expect(screen.getByTestId('plugin-result')).toHaveTextContent('2 skill(s)');
    });
  });
});
```

- [ ] **Step 4: Run frontend tests**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; pnpm --filter @edytlab/desktop test -- --run 2>&1 | tail -20
```

- [ ] **Step 5: Type check**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; pnpm --filter @edytlab/desktop exec tsc --noEmit
```

- [ ] **Step 6: Commit**

```
feat(ui): Plugins tab in Settings — install from github:org/repo or local: path

https://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd
```
