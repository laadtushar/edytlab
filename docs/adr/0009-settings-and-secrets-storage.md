---
title: "0009. Secrets are one keychain item, and settings are a file"
sidebarTitle: "0009 Settings and secrets storage"
---

Status: Accepted, 2026-10-25.

## Context

`ai::keychain` kept every value as an item of its own under the service `app.edytlab.desktop`:
`active_provider`, and per provider `<provider>_api_key`, `<provider>_model`, `<provider>_base_url`
and `<provider>_effort`, plus `chatgpt_refresh_token`. A launch read several of them
(`try_load_api_key_at_startup`: the active provider, its key and its model; `rebuild_agent_into`: the
key again, the base URL and the effort; `restore_chatgpt_at_startup`: the token).

macOS asks for the login password once for each existing item an app reads when the item's access
list does not name that exact build. A person who answers **Allow** rather than **Always Allow** is
asked again at the next read of the same item. The installers are ad-hoc signed (#386, no
certificates), so every update is a new build and every item asks again. The owner reported it on
v0.9.0: four to five password prompts at first launch (#734). Signing with a Developer ID would make
Always Allow survive updates, but that waits on certificates; the number of prompts has to be small
whatever the signing. On Linux the same shape was one Secret Service unlock per locked collection a
read touched (#527).

Most of what was in the keychain is not a secret. A model name, a reasoning effort, a base URL and
the id of the active provider were there only because `active_provider` once was ("this module is
where per-provider settings live"), and the privacy page already says no key or prompt is in them.

## Decision

Two homes, split by whether the value is a secret.

- **Secrets are the entries of one keychain item**: service `app.edytlab.desktop`, account
  `secrets`, whose value is a JSON object from slot name to value:

  ```json
  { "anthropic_api_key": "…", "openai_api_key": "…", "chatgpt_refresh_token": "…" }
  ```

  The slot names are the per-slot accounts of earlier builds. The item is read once per process and
  cached. A save or a delete reads the item itself (not the cache), changes the map, writes the item
  and then replaces the cache, all under one mutex that also serialises every other keychain call,
  so a key save and a ChatGPT token refresh cannot lose each other and a read after a write never
  sees the older value. A write that fails leaves the cache as it was. The last delete removes the
  item. The item is written and read as UTF-8 bytes (`set_secret` / `get_secret`), not as a
  password. One entry, `__migrated`, is reserved (see Migration); no slot can have that name.
- **Everything else is a JSON file**, `provider-settings.json`, in the app's config directory:

  ```json
  { "version": 1,
    "active_provider": "anthropic",
    "providers": { "anthropic": { "model": "…", "effort": "high" },
                   "ollama": { "base_url": "http://localhost:11434/v1" } } }
  ```

  The desktop registers the directory once at startup with `ai::keychain::set_settings_dir` (an
  absolute path; the first call stands; the same shape as `ml_pipeline::runtime::set_bundle_dir`).
  The file is written as a temporary file beside it, flushed, then renamed over it, mode `0600`, and
  holds no secret. Fields a newer build wrote are kept when this one saves. A save of a setting makes
  no keychain call and answers `Persistence::Durable`, because it is a file.
- **No directory registered is an error.** Every function in `ai::keychain` then fails (a reader
  with a `KeychainError`, a writer with a `keyring::Error`) and touches neither a file nor the
  keychain. A guessed directory (the working directory, a temp folder) would put the settings
  somewhere the app never reads again, and an item read before the migration state is known would be
  a prompt for nothing. The CLI never reads the keychain; a test registers a temporary directory, or
  builds a `Store` over fake slots and a directory of its own, which is how `keychain_layout_tests.rs`
  counts calls.
- **Every public function keeps its signature**, including `{ persistent, reason }` on every write
  (#539), the Linux kernel-keyring fallback and its reporting (#394), `KeychainError::access_denied`,
  and `has_api_key` rejecting on an unreadable store. The `secrets` item goes through the same
  primary / kernel-keyring read, write and delete helpers each per-slot item did.

### Migration

A settings file that is there, with a `version`, means the migration has run. Without one, the first
call that needs anything:

1. reads every legacy per-slot item: for each id in `SUPPORTED_PROVIDER_IDS`, its key, model, base
   URL and effort (the ChatGPT plan has no key or base-URL slot, so none is read for it, and a stray
   one is never imported), plus `chatgpt_refresh_token` and `active_provider`. This is the last time
   their prompts appear. Nothing is moved between the Secret Service and the kernel keyring while
   reading;
2. writes the `secrets` item (merged over one that is already there, the per-slot items winning,
   since they are what the app was using, unless the item carries `"__migrated": "1"`: see 3) and
   reads it back;
3. writes the settings file and reads it back. It is written last, so its presence is the commit.
   Then it records the commit in the item, as the entry `"__migrated": "1"`. Readers look a slot up
   by name and nothing reads the marker. An item that carries it is authoritative: a later migration
   (the settings file went missing) never lets a per-slot item override it, and only deletes the
   leftovers (settings found per slot are still adopted: the file is gone and they are all there is). An item without it is an unfinished attempt, and per-slot still wins;
4. deletes the legacy items it found. A delete that fails leaves a stale item that nothing reads.

A failure at step 1 or 2 or 3 deletes what the attempt wrote (the new item, if the attempt created
it) and keeps every legacy item. The process then runs in the `Legacy` layout: each slot is its own
item again and a save goes to its item, exactly as the build before, so nothing is lost and nothing
diverges. The next launch tries again. Two cases are put off rather than run, because they would
lose data without failing:

- the Secret Service cannot be reached (`read_from` answers "nothing stored" after two looks, which
  on a machine with no service is right and during an outage is not: the keys it holds would be left
  behind and forgotten). Such a machine stays on the per-slot items until a service answers;
- the new item could only be saved to the kernel keyring (`SessionOnly`): the legacy items would be
  deleted for a copy a reboot erases.

A read the keychain refuses (locked, prompt dismissed) is the caller's answer, as for any read; the
migration stops there, reads nothing more, and is asked for again at the next call. A settings file
that does not parse is moved aside to `provider-settings.json.bad` and a new one is started (the
secrets are in the keychain; the model, effort, address and provider choices are lost). One that
cannot be read at all (a folder in its place, no permission) is an error from every read and write,
never a switch to the per-slot items: by then they are deleted, and a save to one would never be
read. It is asked for again at the next call.

A secrets item that does not parse is an error that quotes nothing (`serde_json` quotes the value in
a type error), and is not overwritten by a save.

## Considered and rejected

- **Keep one item per slot and ask macOS for Always Allow.** It is what a Developer ID signing makes
  work (#386), and it is not available. Without it every update asks for every item again.
- **Put everything, settings included, in the one item.** One prompt either way, but every change of
  a model or an effort would then be a keychain write, and a Settings save is one of the places a
  Secret Service unlock prompt used to come from (#527). The settings are not secrets.
- **Encrypt the settings file, or keep the secrets in a file with a key in the keychain.** Adds a
  scheme to maintain for no gain: the key would be an item, and the file is a second thing to lose.
- **A separate item per provider.** One prompt per provider read, which is what a person with three
  providers saw.
- **Migrate as a separate startup step, or in a command.** It would need a place in the startup
  order and something for every reader to wait on. The first call that needs anything is where it is
  needed, and it runs under the lock every call takes, so nothing can read a half-moved layout.
- **Delete the legacy items first and rely on the new ones.** A failure then loses keys. The
  migration deletes only after both new homes read back.

## Consequences

- A launch reads one item, once: the first prompt is the only one, and **Always Allow** keeps the
  rest away until an update. The first launch after this change also reads the legacy items (once
  each that exists), and each of those prompts is the last of its kind.
- A downgrade to a build before this one finds no keys, because the legacy items were deleted. The
  keys are in the `secrets` item; a person who goes back enters them again.
- A machine that has no Secret Service keeps the per-slot items in the kernel keyring and does not
  migrate until one answers. Its behaviour is the build before this one's.
- Rewriting the item on every save rewrites every secret in it. A token refresh therefore rewrites
  the API keys; a failed write leaves the old item, which the cache then matches.
- **Windows caps one credential at 2,560 bytes** (`CRED_MAX_CREDENTIAL_BLOB_SIZE`), and now every
  secret shares it. `keyring` writes a password as UTF-16, which would make that 1,280 characters,
  so the item is written with `set_secret` (the UTF-8 bytes, 2,560 of them) on every backend
  (macOS, Windows, Secret Service and the kernel keyring all implement it in `keyring` 3.6; this
  build was unreleased, so no shipped item has to be converted; the per-slot items are still read
  as passwords). Six hosted keys and a long refresh token still fit, and a save that no longer does
  fails with `keyring`'s `TooLong` and changes nothing; the window says the saved keys and sign-in
  does not fit and that removing a key you do not use may make room (the sign-in's own text says it
  is too long for the store when no key is saved too). A migration that does not fit stays on the
  per-slot items.
- **Two processes share one item.** Every save and delete reads the item (and the settings file)
  again before changing it, so a rotated ChatGPT token one process saved is not undone by a key the
  other saves from a copy it read earlier. That is one extra read per save, of an item this build
  created, so it raises no prompt.
- **A save during a Secret Service outage cannot wipe the other keys.** With the service
  unreachable the save goes to the kernel keyring, and that copy then holds only what the save could
  see. When the service answers again the kernel copy is merged into the durable item key by key
  (the kernel copy winning per key), never replacing it. A save made while the store cannot be
  reached writes only the kernel copy, so a service that comes back between the read and the write
  is not written from an incomplete map. A removal cannot be recorded in that state (the merge would
  bring the key back), so it fails with an error.
- **A Deny on macOS is remembered.** `keyring` reports it as `PlatformFailure`, which is not cached
  as an answer, and every later read in the launch would prompt again. A failed read of the item is
  now remembered for ten seconds and answered the same way, with no keychain call, as
  `keychain_refused` does in the desktop (#497), but not for good: the reads of one startup do not
  each prompt again, while a failure that was only a moment (the keychain after a sleep, a service
  still starting) does not hide the keys for the rest of the process. A save is the user's act and
  tries again at once.
- **Every save re-reads the item**, so a macOS user who answered **Allow** rather than **Always
  Allow** after an update may be prompted on a save, the ChatGPT token refresh included. The tests
  cannot see macOS's prompts: the owner checks it on a Mac.
- **Cross-process saves are narrowed, not atomic.** The re-read shrinks the window in which two
  processes can overwrite each other to the time between the read and the write; there is no lock
  across processes.
- **A marked item survives a settings file that cannot be written.** If the file cannot be written
  at launch but the item carries `__migrated`, the process stays on the current layout: the item
  serves the secrets, the settings are held in memory (seeded from whatever per-slot settings are
  left), and each settings save tries the file again and returns the error while it cannot be
  written. It never switches to the per-slot items, whose saves the next launch would delete.
- **An MCP server's `<keychain:slot>`** is not a provider's slot and was never in the item. It is the
  per-slot item `<slot>_api_key` that the user makes in the OS store, read on demand when the slot is
  not in the item, and never migrated or deleted. A provider's slot that is not set costs no such read.
- **A base URL is a setting, so it is in the plain settings file.** One that holds credentials
  (`https://user:pass@host`, or a key in its query string) was in a keychain item before and is a file
  with mode `0600` now. A key belongs in the key field; the base URL is for an address.
- What macOS asks for when an updated build rewrites an item the previous build created is not
  something the tests can see. The owner checks one launch on a Mac.

## How we will know

Tests over fake slots that count every call (`crates/ai/src/keychain_layout_tests.rs`): a launch's
reads of the active provider, a key, a model, a base URL, an effort and the ChatGPT token make one
read of `secrets` and no other call; saves of a model, effort, base URL and provider make none; a
legacy layout with keys for three providers, models, a base URL, an effort and a ChatGPT token
migrates to identical values, leaves no legacy item, and the next launch reads one item; a write
that fails during the migration (the item, the file) keeps every legacy item and serves their
values; a key save and a token save at the same moment both persist; a delete leaves the item and the
cache in agreement; an outage and a `SessionOnly` save change nothing; a save in an outage keeps the
other keys and a removal in one is an error; two stores over one keychain keep each other's saves;
an unreadable settings file is an error; a leftover per-slot item does not override a marked item; an
MCP slot resolves; a byte cap is met by the bytes; a failed read is remembered. Then the owner's check on a
Mac: at most one prompt at the first launch after the migration.
