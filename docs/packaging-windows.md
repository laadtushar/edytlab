# Windows packaging notes (M15)

## What's wired up already

- **`apps/desktop/src-tauri/tauri.conf.json`** has `bundle.windows.webviewInstallMode: { "type": "embedBootstrapper" }` (added in M14 prep). This bundles Microsoft's WebView2 bootstrapper into the `.msi`, so first-run install works offline. Bundle size is +~1.7 MB.
- **`.github/workflows/release-signed.yml`** handles Authenticode signing via `signtool` (SHA256, DigiCert timestamp) after the bundle step, and verifies via `signtool verify /pa /v`. It then uploads the signed files itself. This ordering is the whole point: the earlier `release-win.yml` let tauri-action upload at build time and signed the leftover local copies, so the published assets were unsigned while `signtool verify` still passed against the runner's disk.
- **`embedBootstrapper`** is the deliberate choice over `downloadBootstrapper`. Trade-off:
  - `embedBootstrapper`: +1.7 MB to .msi, install works offline, no first-run network dependency.
  - `downloadBootstrapper`: smaller .msi but installer must reach `aka.ms/...` at install time.
  Phase 1 ships `embedBootstrapper` for predictable install UX.

## Required GitHub secrets for the Windows leg of `release-signed.yml`

| Secret | Description |
|---|---|
| `WINDOWS_CERTIFICATE` | Base64-encoded Authenticode `.pfx` |
| `WINDOWS_CERTIFICATE_PASSWORD` | Password for the `.pfx` |

Until these are configured the workflow fails fast at "Verify required secrets".

## SmartScreen reputation

Per the M02 / M15 plan: SmartScreen reputation is **earned through downloads**, NOT bypassed by signing alone (this changed in March 2024). Signed binaries from a brand-new code-signing certificate will still show the "Windows protected your PC" warning until the certificate accrues reputation through legitimate downloads.

**Plan**: ~2 weeks of beta distribution to private testers before public launch, with an explicit README note on the ‑signed warning so testers know to click "More info → Run anyway".

This is documented because no amount of signing config can shortcut it.

## Local verification checklist

What can be verified outside a real Windows runner:

- [x] `tauri.conf.json` parses (CI will compile)
- [x] `webviewInstallMode: embedBootstrapper` declared
- [x] `release-signed.yml` references the secrets and signtool path
- [x] `actionlint` (with shellcheck) passes on the workflow

What requires a real Windows runner with signing secrets:

- [ ] `pnpm tauri build` produces a signed `.msi`
- [ ] `signtool verify /pa /v <msi>` returns "Successfully verified"
- [ ] `.msi` installs cleanly on Windows 11 22H2 with no pre-installed WebView2
- [ ] App launches and the "edytlab" window appears

The Windows leg of `release-signed.yml` is the gate. Verify `signtool verify /pa` against a **downloaded release asset**, not against a file on the runner — that distinction is exactly what the old workflow got wrong.

## Known gotchas

- **WebView2 attribution in Volume Mixer**: outgoing audio shows up under "Microsoft Edge WebView2" rather than "edytlab" (Tauri issue #11113). Cosmetic only, not in scope to fix here.
- **fixed-runtime detection bugs**: some Windows WebView2 install configurations confuse Tauri's auto-detection (issue #13817). Mitigated by always shipping the bootstrapper.
- **WAV file paths from drag-drop** on Windows: Tauri's drag-drop event emits Windows-style paths (`C:\Users\...`). The frontend must NOT path-normalize before passing to Rust commands; Rust's `Path` handles both separators.
