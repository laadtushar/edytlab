# Sign in with ChatGPT: running edytlab's agent on a ChatGPT subscription

- **Date:** 2026-10-10
- **Status:** Research and implementation plan. No code has changed.
- **Question:** Can a user sign in with their ChatGPT account (Plus, Pro, Team) and have edytlab's agent bill its LLM requests to that subscription instead of an API key? If so, exactly how?

## Verdict

**Yes, through an official, documented path, with limits.** OpenAI's *Sign in with ChatGPT* has a "ChatGPT plan usage" capability for open-source, locally run apps. It is in preview and was announced at DevDay on 29 September 2026. The app registers itself during the user's first sign-in, with no client secret, no partner key and no approval step. It gets an OAuth access token scoped to `chatgpt.tokens.use.direct` and sends `POST https://api.openai.com/v1/responses` with it. That usage counts against the user's Plus or Pro allowance. edytlab qualifies today: it is MIT-licensed, public, runs on the user's machine and charges nothing. The limits:

- **Personal Plus and Pro only.** Team, Business, Enterprise and Edu are not listed.
- **Responses API only, under preview rules.** The existing `openai` provider speaks Chat Completions and cannot carry these tokens.
- **Becoming a paid or hosted app changes the track.** That would mean joining OpenAI's waitlist first.
- **Codex's own route is off limits.** Borrowing Codex CLI's sign-in or ChatGPT's `backend-api` endpoints is not the documented path, and this plan does not use it.

The rest of this document gives the evidence (§1–§2), the protocol (§3), the fit with edytlab's code (§4), alternatives for users who are not eligible (§5) and what to watch (§6).

> **How to read the citations.** Each factual claim carries a quote and a source tag, and the tags resolve to URLs in [Sources](#sources). The developers.openai.com pages were read in full from their Markdown versions. openai.com and help.openai.com answered every direct request from this environment with HTTP 403. Quotes from those pages, marked **†**, are excerpts as indexed by web search, not a direct read. Check them before relying on exact wording. Secondary press coverage was used only to find the primary sources, never as evidence.

---

## 1. What OpenAI announced, and when

| Claim | Evidence |
|---|---|
| Announced at DevDay 2026, starting with 16 partners. | "Sign in with ChatGPT: eligible subscription usage in third-party products, starting with 16 partners." [RECAP]† (the recap is dated 29 September 2026 in the search index) |
| Sign-in and plan usage are separate permissions. | "Sign-in access is separate from permission to use your ChatGPT plan allowance." [NOTES]† |
| Open-source apps get plan usage. Identity-only sign-in is a limited trial for commercial partners. | "Sign in with ChatGPT is currently available to selected commercial partners through a limited trial. ChatGPT plan usage is available to all open-source partners and selected private clients." [QS] |
| No API key is needed. | "Let users run AI workloads in your tools with their ChatGPT plan, without requiring them to provide an API key. The open-source sign-in flow registers your client and issues OAuth credentials for eligible Responses API requests, without a client secret or partner API key." [QS] |
| Who it is for at launch. | "**At launch**, ChatGPT plan usage is available to open-source projects, personal projects that run locally, and selected private apps." [COOK] |
| It is a preview. | The open-source docs end with a "Preview limitations" page: "These limits apply to ChatGPT plan usage through Sign in with ChatGPT and `https://api.openai.com/v1`". [PREV] |

Secondary coverage says an identity-only sign-in beta ran from July 2026. No primary source I could read confirms that, and it does not affect this plan.

---

## 2. The mechanism, eligibility and terms

### 2.1 Three integrations, one of which fits edytlab

OpenAI offers three integrations [QS]:

| Integration | Fit for edytlab |
|---|---|
| "Sign-in on your website": identity only, for commercial partners on a trial ("Sign in with ChatGPT is currently offered to a select group of commercial partners." [CID]) | No. edytlab has no accounts to sign in to. |
| "Sign-in for your ChatGPT plugin" | No. |
| "ChatGPT plan usage in your open-source app": "Eligible ChatGPT Plus and Pro users can use their ChatGPT plan for AI requests in your product." | **Yes.** This is the mechanism. |

It is OAuth 2.0 authorization code with PKCE plus OpenID Connect. The client is **registered dynamically** on the user's first sign-in: "Start first-time registration with `client_id=dynamic_agent_client` … the callback returns its issued `client_id` … This direct flow needs neither a client secret nor a partner API key." [SIGNIN]

### 2.2 Which apps may use it

- **Open-source or locally hosted apps, as edytlab is today.** "These docs explain ChatGPT plan usage for open-source and locally hosted apps. If you're interested in offering it in a paid or remotely hosted app, complete the interest form." [OSS] The cookbook says the same: "The ChatGPT plan usage integration described here is available for open-source tools and personal projects that run locally. If you're building a paid or remotely hosted app, join the waitlist to request access before offering it to users." [COOK]
- **edytlab's status.** The repository is public and MIT-licensed (`LICENSE`; the site FAQ says "The desktop app and audio engine live in a public GitHub repository under the MIT License"). It runs on the user's machine, and "The app is free in BYO-key mode" (`website/components/landing/faq.tsx`).
- **Watch item.** The same FAQ says "A hosted subscription that bundles AI inference is on the roadmap". If that ships, edytlab becomes a paid app and has to go through the interest form before offering plan usage there.
- **No approval step is documented for the open-source flow.** Registration happens inside the user's own consent screen. The quickstart calls open-source integrators "open-source partners" but documents no enrolment for them.

### 2.3 Which users

| Question | Answer | Evidence |
|---|---|---|
| Plans | **Plus and Pro.** The developer docs name only these. | "Eligible ChatGPT Plus and Pro users can use their ChatGPT plan for AI requests in participating apps" [QS] |
| Go | Listed for *commercial* participating apps only. Unconfirmed for open-source clients. | "the option to use your ChatGPT plan in participating commercial apps and sites is only available with Go, Plus, and Pro." [HELP]† |
| Free | Can sign in, but has no plan usage. | "Anyone can sign in to participating apps and sites with ChatGPT, but the option to use your ChatGPT plan … is only available with Go, Plus, and Pro." [HELP]† |
| Team / Business / Enterprise / Edu | **Not listed anywhere, so treat them as unsupported.** Each client is bound to one workspace, and ineligible workspaces get a dedicated error. | "each issued `client_id` is bound to the authenticated user and the workspace selected during registration" [OSS]; `subscription_sharing_user_not_eligible` (403): "ChatGPT plan usage is unavailable for the selected user, workspace, or policy." [ERR] |
| Regions | No region list is published. A region policy can refuse a request. | `403`: "A policy or permission check, such as the permitted serving region, prevented admission." [ERR] |

The owner asked about Team specifically. On today's documentation a Team (Business) seat is **not** an eligible way to pay. A user who has both a personal Plus plan and a work seat must pick the personal workspace at sign-in.

### 2.4 Terms that govern it

- OpenAI's general terms apply. "Review OpenAI's terms and policies before distributing your integration." [COOK]
- **Users control spend.** Users set a weekly limit per app, and credits are opt-in. In the example ChatGPT settings shown on [SESS], each app has a "Manage limits action that opens a dialog where users can allow the app to use their ChatGPT plan, set the app's weekly usage limit from 10% to 100%, and choose whether it can use credits after reaching plan limits." Usage comes out of the plan the user already pays for: "Eligible AI requests count toward the ChatGPT Work and Codex usage included in your plan." [HELP]†
- **The app gets no access to conversations.** "Using a ChatGPT plan requires separate Responses API scopes that the user must authorize. It doesn't grant access to the user's API key or conversations." [QS]
- **Branding requirements.**
  - "Label the action **Continue with ChatGPT**. Place it with your other sign-in options, give it comparable visual prominence, and use approved OpenAI branding." [QS]
  - "Distinguish ChatGPT plan usage from your app's own subscription or charges." [UX]
  - OpenAI's marks are governed separately: "OpenAI trademarks remain subject to the OpenAI brand guidelines." [DEVKIT]
- **The DevKit's code is noncommercial.** OpenAI ships a reference SDK and example app, but its licence is "solely for Noncommercial Purposes", and anyone distributing modifications must "license every modification … under this same License" [DKLIC]. edytlab is MIT and its FAQ allows commercial reuse. **Do not copy DevKit code into edytlab.** Implement from the protocol docs, which this plan does in Rust anyway.

### 2.5 What is not allowed

1. **Calling ChatGPT's private backend with these tokens.** "**Supported endpoint:** Use the public Responses API endpoint above for this ChatGPT plan usage flow; do not point it at ChatGPT's `backend-api` endpoints." [INF]
2. **Borrowing Codex CLI's own sign-in.** OpenAI's Codex CLI signs in with ChatGPT through its own first-party client and a different scope set: `"openid profile email offline_access api.connectors.read api.connectors.invoke"` [CODEX-LOGIN]. It then talks to `pub const CHATGPT_CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";` [CODEX-PROV]. Some unofficial tools reuse that client id, or Codex's saved session tokens, to reach that backend from other apps. That is exactly the route [INF] rules out. OpenAI's terms also forbid users to "Automatically or programmatically extract data or Output" and to "Interfere with or disrupt our Services, including circumvent any rate limits or restrictions or bypass any protective measures or safety mitigations we put on our Services." [TOU]† **edytlab must not do this.** The documented open-source flow is the only route this plan uses.
3. **Using a route other than `POST /v1/responses`.** `subscription_sharing_route_not_supported` (403): "support for another client type or route is not permission to use it here." [ERR] That rules out Chat Completions, and Realtime is not documented for this flow.
4. **Silently falling back to other billing.** "ChatGPT plan usage errors stop inference. OpenAI does not silently switch the request to another billing path." [ERR] edytlab should behave the same way: never retry on a different provider without the user choosing it.
5. **Putting tokens where they leak.** "Keep tokens out of browser storage, source control, logs, analytics, and support transcripts. Never put access or refresh tokens in URLs." [SESS]

---

## 3. The protocol for a native desktop app

Everything below comes from OpenAI's open-source plan-usage docs, read on 2026-10-10.

### 3.1 Host ID: once per installation

"Generate and persist a stable `ext_agent_host_id` for each host separately. Choose and persist the host ID before its first sign-in. Use an opaque value, not an email, user ID, or other user-identifying value. A host ID is not an authentication credential." [OSS]

Accepted forms are `urn:ietf:params:oauth:jwk-thumbprint:…` (recommended), `urn:uuid:…` ("Supported alternative") and `did:key:<key>` [OSS]. **edytlab should use `urn:uuid:<v4>`.** It is the simplest form, and "A public-key-derived host ID is currently an identifier only. OpenAI does not verify possession of the private key" [OSS], so a key pair buys nothing today.

### 3.2 Authorization request

The app opens the **system browser** at `https://auth.openai.com/api/accounts/authorize` [SIGNIN] with these parameters:

| Parameter | Value |
|---|---|
| `client_id` | `dynamic_agent_client` on first registration, then the issued `oaiapp_…` id |
| `agent_name_hint` | `edytlab`, on first registration only ("Omit it from reauthorization with an issued client ID") |
| `ext_agent_host_id` | the host ID, always ("Required host identifier, distinct for each host") |
| `id_token_hint` / `login_hint` | optional, on reauthorization |
| `response_type` | `code` |
| `redirect_uri` | `http://127.0.0.1:<port>/auth/callback` |
| `scope` | `openid profile email offline_access resource.invoke chatgpt.tokens.use.direct` ("Identity scopes: `openid profile email`. ChatGPT plan usage scopes: `offline_access resource.invoke chatgpt.tokens.use.direct`.") |
| `resource` | `https://api.openai.com/v1` |
| `state`, `nonce` | fresh random values for each attempt |
| `code_challenge_method` / `code_challenge` | `S256` / "The base64url-encoded SHA-256 digest of the PKCE verifier, without padding." |

All values are from [SIGNIN].

### 3.3 Redirect: an HTTP loopback listener, not a custom scheme

"Use an HTTP loopback callback on `127.0.0.1` from initial registration onward, for example `http://127.0.0.1:1455/auth/callback`. Start the listener before opening the browser. Later sign-ins may use another available port … only the port may vary. Keep the scheme, host, and path unchanged … Do not substitute with `localhost`." [SIGNIN]

What this means for Tauri 2:

- **`tauri-plugin-deep-link` is not usable here.** It makes the app "the default handler for an URL" using registered schemes [TAURI-DL], and OpenAI accepts only the HTTP loopback redirect. The callback listener is a short-lived `tokio::net::TcpListener` bound to `127.0.0.1` in Rust. OpenAI's own Codex CLI does the same: `format!("http://127.0.0.1:{actual_port}/auth/callback")` with `const DEFAULT_PORT: u16 = 1455;` [CODEX-LOGIN].
- **Open the browser from Rust with `tauri-plugin-opener`.** Its Rust API is `app.opener().open_url("https://tauri.app", None::<&str>);` [TAURI-OPENER]. Tauri capabilities govern "the core exposure to the application frontend running in the system WebView" [TAURI-CAP]. A call made from Rust therefore needs no new webview permission, and the authorization URL, which can carry an `id_token_hint`, never passes through the webview. Today edytlab has "no opener plugin, no `shell:open` permission" (`apps/desktop/src/components/ChatMarkdown.tsx`). This keeps that true for the frontend.

### 3.4 Callback and code exchange

- "Validate `state` against the pending authorization attempt and handle an OAuth `error` before using the result." [SIGNIN]
- On `error=access_denied`, "stop the attempt, and do not exchange a code." [SIGNIN]
- New registration: "If a new-registration callback does not include an issued ID, treat registration as incomplete." On reauthorization: "If the callback supplies a different client ID, reject the result" [SIGNIN].
- Exchange: "POST a form-encoded `authorization_code` grant to `https://auth.openai.com/api/accounts/oauth/token` with the issued `client_id`, `code`, `code_verifier`, the same `redirect_uri`, and the same `resource`. No client secret is required." [SIGNIN]
- "If code exchange returns `invalid_grant`, discard that code and start a fresh authorization" [SIGNIN].

### 3.5 Validation

- **ID token:** "Verify its signature against OpenAI's published JWKS. Check the issuer, audience against the issued client ID, expiration, and the nonce saved for this attempt. Use its validated `sub` as the account identity." [SIGNIN]
- **Discovery:** "OpenAI publishes its production OpenID Connect discovery document at `https://auth.openai.com/.well-known/openid-configuration`. Load the `issuer`, `authorization_endpoint`, `token_endpoint`, and `jwks_uri` values from that document." The example configuration on that page gives "Authorization endpoint: https://auth.openai.com/api/accounts/authorize", "Token endpoint: https://auth.openai.com/api/accounts/oauth/token" and "JWKS URI: https://auth.openai.com/.well-known/jwks.json". These are the same endpoints the open-source flow names [WEB].
- **Plan permission:** "Check the token response's granted scopes for `chatgpt.tokens.use.direct` before proceeding to inference. A valid ID token alone does not authorize ChatGPT plan usage." [SIGNIN]

### 3.6 Tokens: lifetime, refresh and revocation

- **Token response:** "includes `access_token`, `refresh_token`, `id_token`, `token_type`, `expires_in`, `scope`, and `earliest_refresh_at`." [TOK]
- **Lifetimes:** "Access tokens are valid for one hour (`expires_in: 3600`). Refresh tokens are valid for 30 days. Each successful refresh returns a replacement refresh token with a fresh 30-day lifetime." [TOK]
- **Refresh:** "POST form-encoded `grant_type=refresh_token`, the issued `client_id` … the saved `refresh_token`, and `resource=https://api.openai.com/v1` to `https://auth.openai.com/api/accounts/oauth/token`; omit `scope` to retain the grant." And: "serialize refreshes for the same session so two processes do not race a rotating token." [SESS]
- **`earliest_refresh_at`:** returned but not defined further [TOK]. Treat it as the earliest time to refresh, and confirm against a real response.
- **Refresh failures:** on "`invalid_grant` on refresh, `invalid_refresh_token`, `token_expired`, `refresh_token_expired`, `refresh_token_invalidated`, or `refresh_token_reused`. Clear unusable tokens and repeat OAuth with the saved issued client ID." [ERR]
- **Sign-out:** "revoke the renewable session using the `revocation_endpoint` from `https://auth.openai.com/.well-known/openid-configuration`. Send a form-encoded `POST` with `token=<REFRESH_TOKEN>`, `token_type_hint=refresh_token`, and the issued `client_id`." Then: "Retain its account/client mapping and this host's ID for a later sign-in." [SESS]
- **Disconnects are silent:** "OpenAI does not currently notify your tool when a user disconnects the app in ChatGPT settings." [ERR]
- **Returning sign-in is light:** with an issued client ID, "the user sees the account selector screen, then redirects, with no workspace selection or consent screen." [SIGNIN]

### 3.7 Which endpoint accepts the token

"send the OAuth access token as `Authorization: Bearer <ACCESS_TOKEN>` to `POST https://api.openai.com/v1/responses` … **Set `store` to `false` and `stream` to `true` on each HTTP inference request in this flow.**" [INF] Chat Completions and Realtime are not accepted (§2.5).

Preview rules that matter for edytlab [PREV]:

- **Request shape:** "Send `input` as an array containing the context needed for each request. Use `instructions` or developer messages; explicit `{type: "message", role: "system"}` items are rejected."
- **Unsupported fields:** "Omit `background`, `conversation`, `max_output_tokens`, `max_tool_calls`, `metadata`, `moderation`, `multi_agent`, `prompt`, `prompt_cache_retention`, `safety_identifier`, `temperature`, `top_logprobs`, `top_p`, `truncation`, and `user`."
- **Conversation state:** "Omit `previous_response_id` over HTTP and send the required history in `input`." edytlab already resends history every round trip.
- **Tools:** "Group function/custom tools in namespaces or supply them through `additional_tools` input items." A namespace is `{"type": "namespace", "name": …, "description": …, "tools": [{"type": "function", …}]}` [FC].
- **Unsupported tools:** "Image generation, file search, Code Interpreter, native computer use, hosted MCP/connectors, and Responses `tool_search`." edytlab uses none of these. Its tools are client-side functions.
- **Inputs:** "Audio/video input, the Files upload API, and the transcription API are not supported by this flow." edytlab sends the model text only. Audio stays in the local engine.
- **Completion:** "Treat inference as successful only after receiving `response.completed` … A usage-limit error can arrive as `response.failed` with `subscription_sharing_usage_limit_exceeded` or `subscription_sharing_usage_unavailable` after streaming has begun." [INF]
- **Stream events for tool calls:** `response.output_item.added` carries a `function_call` item with `call_id` and `name`, then `response.function_call_arguments.delta` events, then `…arguments.done` and `response.output_item.done` [FC]. Tool results go back as `function_call_output` items with the same `call_id` [FC].

### 3.8 Models, limits and attribution

- **Models are per account.** "request the selected ChatGPT account's model catalog with the same access token you will use for inference: `GET https://api.openai.com/v1/models` … The model-list response contains a `models` array … Show `display_name` in your UI and pass the selected `slug` as `model`" [INF]. Keep entries with `visibility == "list"`. This envelope differs from the API-key catalogue's `{"data": [{"id": …}]}`, which `crates/ai/src/models.rs` parses today. No model should be hard-coded as the default.
- **Rate limits:** "For ChatGPT Plus users, the five-hour usage limit is shared across all apps where they use their ChatGPT plan, including private and open-source clients … The five-hour usage limit does not apply for Pro users." [SESS] Per-app weekly caps are user-set (§2.4). No numeric limits are published for this flow.
- **Attribution:** "Its issued `client_id` identifies that registration, its security boundaries, and its ChatGPT plan usage settings." "Send the stable, opaque `ext_agent_host_id` so ChatGPT plan usage can be associated with its host" [OSS]. Users review usage at ChatGPT Settings → Usage, and apps should "Link from your tool's usage-tracking pages to ChatGPT Settings → Usage" (`https://chatgpt.com/settings/usage`) [SESS].

### 3.9 Errors and how to recover

From [ERR]:

| Signal | Meaning | Recovery |
|---|---|---|
| Scopes lack `chatgpt.tokens.use.direct` | Signed in, plan use not granted | "retain the sign-in but mark ChatGPT plan usage as disabled … offer a clear choice: enable ChatGPT plan usage or configure another supported billing path, such as the user's own API key." Re-enabling repeats OAuth with the full scope set, using "`prompt=consent`" until OpenAI confirms `force_reconsent=true` for the integration. |
| `401` before the stream, or `subscription_sharing_invalid_user` | Identity or permission not accepted | Refresh once, then ask the user to sign in again "after confirmed revocation or a terminal refresh error". |
| `403` before the stream | Policy, for example region | "Surface the restriction and verify the integration." |
| `subscription_sharing_user_not_eligible` (403) | Plan or workspace not eligible | "Explain the restriction; do not repeat the same request or loop through OAuth." |
| `subscription_sharing_usage_limit_exceeded` (429) | Plan or per-app cap reached | "Pause new requests … link to ChatGPT settings → Usage. Do not assume the entire plan is empty or infer a reset time" |
| `subscription_sharing_usage_unavailable`, `…user_unavailable` (503), admission `503` | Temporary | "Preserve credentials and retry later with bounded backoff." |
| `subscription_sharing_unsupported_capability` (400) | A field, tool or model we sent is not allowed | "Inspect `error.param` … Do not retry the same invalid body." This is a bug on edytlab's side. |
| `chatpass_v2_scope_not_authorized`, `chatpass_v2_invalid_authorization_context` (403) | Grant misconfigured | "Check the client and grant configuration rather than retrying or changing billing." |
| Admission failures with a body like `{"detail":"..."}` | Not the standard error object | "Treat the message as diagnostic text, not a stable machine-readable code." |

---

## 4. How it fits edytlab

### 4.1 What exists today

| Area | Today | What blocks plan usage |
|---|---|---|
| `crates/ai/src/provider.rs` | `LlmProvider` owns auth, endpoint, serialisation and stream parsing. `OpenAIProvider::endpoint_path()` is `"/v1/chat/completions"`. `WireFormat` is `AnthropicMessages` or `ChatCompletions`. | Plan tokens are accepted only on `/v1/responses` (§2.5). A third wire format is needed. |
| `crates/ai/src/lib.rs` | `LlmConfig { provider, api_key: String, … }` is built once by `rebuild_agent` and held "for its lifetime" (`commands.rs`). | Access tokens expire hourly (§3.6), so the bearer must be fetched, and refreshed if needed, per request. |
| `crates/ai/src/agent_loop.rs` | Three request sites call `cfg.provider.apply_auth(req, &cfg.api_key)`. `one_shot_body` sends `"stream": false` for the classifier and plan. `extract_response_text` knows two reply shapes. | One-shot calls must stream (§3.7). `max_completion_tokens` and `max_tokens` must not be sent. The system prompt moves to `instructions`. |
| `crates/ai/src/keychain.rs` | Per-provider slots `<id>_api_key`, `<id>_model`, `<id>_base_url`, `<id>_effort`, plus `active_provider`. On Linux it is "the kernel keyring … which lives in memory and does not survive a reboot (#394)". | A rotating refresh token fits a slot. The **registration** (issued `client_id`, host ID) must survive reboots, or every Linux reboot registers a *new* client in the user's ChatGPT settings. |
| Windows Credential Manager via `keyring` 3.6.3 | Secrets are stored UTF-16: "Password strings are converted to UTF-16" [KEYRING]. The blob "cannot be larger than CRED_MAX_CREDENTIAL_BLOB_SIZE (5*512) bytes" [WINCRED]. | So a slot holds at most 1280 ASCII characters. Access and ID tokens are JWTs carrying `encrypted_auth_metadata` [TOK] and may not fit. |
| `crates/ai/src/models.rs` | Parses `{"data":[{"id":…}]}`. | The plan catalogue is `{"models":[{"slug","display_name","visibility"}]}` (§3.8). |
| `apps/desktop/src-tauri` | Capability `default.json` allows `core:default`, `dialog:default` and `dialog:allow-open`. The CSP `connect-src` is `'self' ipc: http://ipc.localhost asset: http://asset.localhost`. There is no opener plugin. | Nothing blocks. All HTTP happens in Rust, and the callback page renders in the system browser, so **the CSP and capabilities need no change** (§3.3). |
| `list_models_for(provider, api_key)` (Tauri) | The webview passes the key in. | For `chatgpt` the token must never reach the webview. The command reads it from the Rust session instead. |
| `Settings.tsx` | `PROVIDERS` table with `needsKey` and `keysUrl`, a key field, Test, Save and Clear. | Needs an OAuth card instead of a key field (§4.5). |

### 4.2 Decision: a new provider id `chatgpt`, not an auth mode on `openai`

Everything below the provider seam differs from the API-key `openai` provider:

- the endpoint and wire format (Responses, not Chat Completions);
- the request rules (`store: false`, `stream: true`, no token caps or temperature, `instructions`, namespaced tools);
- the model catalogue's shape;
- the credential lifecycle (OAuth, rotation, revocation);
- the error vocabulary.

An auth-mode flag would put an `if` in every one of those places. A separate id also keeps the existing per-provider keychain slots meaningful (`chatgpt_model`, `active_provider = "chatgpt"`), so switching between "OpenAI (API key)" and "ChatGPT plan" never mixes their models or settings.

Label it **"ChatGPT plan"** in the picker. Later, the API-key `openai` provider can also move to the Responses wire format and share `responses.rs`. That is a separate PR with its own reviewer focus.

### 4.3 Rust changes (`crates/ai`)

1. **`WireFormat::Responses` and `crates/ai/src/responses.rs` (new).**
   - `serialize_request`:
     - joined system blocks go to `instructions`;
     - user text becomes `{role:"user", content:[{type:"input_text"}]}`;
     - assistant text becomes `output_text`;
     - `ToolUse` becomes `{type:"function_call", call_id, name, arguments}`;
     - `ToolResult` becomes `{type:"function_call_output", call_id, output}`;
     - tools are wrapped in one `{"type":"namespace","name":"edytlab",…}`, and `tool_choice` is `"auto"` or `"none"`;
     - always `store: false` and `stream: true`, and none of the [PREV] unsupported fields;
     - Thinking blocks are dropped, as the Chat Completions path already does.
   - `parse_stream_chunk`: a state machine that maps `response.output_text.delta`, `response.output_item.added`, `response.function_call_arguments.delta`, `response.output_item.done`, `response.completed`, `response.failed`, `response.incomplete` and `error` to the canonical `StreamEvent`s. A stream that ends without `response.completed` is an error, not a success.
   - One-shot calls (classifier, plan) build a streamed body and collect text until `response.completed`. `one_shot_body` and `extract_response_text` gain a `Responses` arm, and the `a_provider_on_chat_completions_declares_it` test learns a third shape.
2. **`ChatGptProvider` in `provider.rs`.** It has `CHATGPT_ID = "chatgpt"`, base URL `https://api.openai.com`, `endpoint_path() = "/v1/responses"`, `wire_format() = Responses`, `supports_effort() = false`, and `list_models_path() = "/v1/models"`. Add it to `provider_from_id` and `SUPPORTED_PROVIDER_IDS`. Replace the boolean `requires_api_key()` with `fn credential_kind(&self) -> CredentialKind { ApiKey | None | ChatGptOAuth }`, and keep `requires_api_key()` as a default derived from it, so no existing call site changes meaning.
3. **Bearer per request.** Add `pub enum Credential { ApiKey(String), ChatGpt(Arc<ChatGptSession>) }` to `LlmConfig` next to `api_key`, or replacing it. Each request site calls `cfg.bearer().await?` and then `apply_auth(req, &token)`. An enum avoids pulling in async-trait. On a `401`, a `ChatGpt` credential forces one refresh and the request is retried **once**.
4. **`crates/ai/src/chatgpt/` (new), with no Tauri dependency:**
   - `oauth.rs`:
     - PKCE (S256), `state` and `nonce`;
     - the authorize-URL builder (§3.2);
     - a loopback listener on `127.0.0.1`: try 1455, then an ephemeral port, fixed path `/auth/callback`, one request, a timeout, a cancel handle, and a small "You can close this tab and return to edytlab" page;
     - the code exchange, refresh and revocation.
     - It takes `open_url: impl FnOnce(&str) -> Result<()>` from the caller, so Tauri passes the opener and tests pass a closure that drives the callback.
     - It uses the explicitly documented `…/api/accounts/authorize` and `…/api/accounts/oauth/token` endpoints. It uses the discovery document only for `jwks_uri` and `revocation_endpoint`, and the issuer is overridable for tests.
   - `id_token.rs`: fetch and cache the JWKS, and verify signature, `iss`, `aud` (the issued client id), `exp` with small skew, and `nonce`. A maintained JWT crate such as `jsonwebtoken` is a new dependency to vet.
   - `session.rs`: the access token, expiry, ID token and scopes in memory. `bearer()` refreshes within about five minutes of expiry, not before `earliest_refresh_at`, behind a `tokio::sync::Mutex` so concurrent agent, classifier and model-list calls refresh once. It writes the rotated refresh token back before returning.
   - `store.rs`, with this split:

     | Data | Where | Why |
     |---|---|---|
     | host ID, issued `client_id`, issuer, verified `sub`, email (as a label and `login_hint`), granted scopes, `saved_at` | `<app config dir>/chatgpt/registration.json`, atomic write, `0600` on Unix | Not secret: the client is "a public client without a secret" [SIGNIN], and "A host ID is not an authentication credential" [OSS]. It must survive reboots (#394), or each sign-in re-registers. |
     | refresh token | keychain slot `chatgpt_refresh_token` | Secret, rotated on every refresh. On save, check it fits the Windows limit; if `keyring` returns `TooLong`, keep the session in memory and tell the user they will sign in again next launch. |
     | access token, ID token | memory only | One-hour lifetime. Avoids the 1280-character slot limit. Reauthorization uses `login_hint` rather than a stored `id_token_hint`. |

     **On Linux, until #394 is fixed,** a reboot drops the refresh token but keeps the registration. Settings then shows "Session ended — Continue with ChatGPT", and the returning sign-in skips consent (§3.6). The same durable store #394 needs would remove even that step.
   - **One account to start.** The docs say "Your app needs to manage multiple account registrations", and also "Within your OSS app, you may choose to offer an account picker" [SESS]. v1 keeps one active account but keys stored registrations by (`sub`, `client_id`), so a picker is additive later.
5. **`models.rs`:** a `chatgpt` arm that calls `GET /v1/models` with the session bearer, filters `visibility == "list"`, keeps server order, and maps `slug` to `id` and `display_name` to `display_name`.
6. **`validate.rs`:** for `chatgpt`, "Test" is the models call plus a tiny streamed Responses request carrying one dummy tool, so `tools_ok` stays meaningful. It reports the mapped error, never the raw token.
7. **Error mapping:** a `ProviderError` or `StreamEvent::Error` kind per row of §3.9, so the UI can tell `UsageLimit`, `NotEligible`, `PlanDisabled`, `SessionExpired`, `Unavailable` (retry with backoff) and `UnsupportedRequest(param)` apart.

### 4.4 Tauri changes (`apps/desktop/src-tauri`)

- **`Cargo.toml`:** `tauri-plugin-opener` for its Rust API only. `lib.rs` registers `tauri_plugin_opener::init()`, and `capabilities/default.json` is **unchanged** because no `opener:` permission is granted to the webview.
- **New commands in `commands.rs`,** all returning `CmdResult<T>` and none returning a token:
  - `chatgpt_status() -> ChatGptStatusDto { state: "signed_out" | "signing_in" | "signed_in" | "plan_disabled" | "session_ended", email: Option<String>, first_use: bool }`
  - `chatgpt_sign_in() -> ChatGptStatusDto`: runs the flow, saves, makes `chatgpt` active, then calls `rebuild_agent`
  - `chatgpt_cancel_sign_in()`
  - `chatgpt_enable_plan_usage()`: reauthorizes with the full scope set and `prompt=consent`
  - `chatgpt_sign_out()`: revokes, clears tokens, keeps the registration, and reports whether revocation was confirmed
  - `open_chatgpt_usage()`: opens the fixed URL `https://chatgpt.com/settings/usage` and nothing else
- **Existing commands:** `provider_is_configured`, `rebuild_agent`, `set_active_provider` and `list_models_for` learn `CredentialKind::ChatGptOAuth`. For it, "configured" means "signed in with plan usage", and the models list reads the session in Rust and ignores any key from the webview.
- Keep the store and engine locks in separate scopes as usual. The OAuth wait (up to minutes in the browser) must hold **no** `AppState` lock.
- **`apps/desktop/e2e/backend.ts`:** an answer for each new command, taken from the command bodies, per the repo's e2e rule.

### 4.5 Settings UI (`apps/desktop/src/components/Settings.tsx`)

- **`PROVIDERS` entry** `{ id: "chatgpt", label: "ChatGPT plan", auth: "oauth" }`, plus `ProviderId` in `src/lib/tauri-bridge.ts` and bridge functions for the new commands.
- **States of the card that replaces the key field:**
  - **Signed out:** the title "Use your ChatGPT plan" and the text "Complete eligible AI requests in this app with usage included in your ChatGPT plan or credits balance" (the [UX] settings card), with a **Continue with ChatGPT** button. Add a line saying this uses your Plus or Pro allowance, and that edytlab itself charges nothing.
  - **Signing in:** "Finish signing in in your browser…" with **Cancel**.
  - **Signed in:** the account email, a **Using ChatGPT plan** badge, **Manage usage** (calls `open_chatgpt_usage`) and **Sign out**. The model picker is filled from the plan catalogue. There is no base-URL field and no effort control.
  - **Plan disabled:** "Signed in, but this app isn't allowed to use your ChatGPT plan", with **Enable ChatGPT plan usage** and **Use an API key instead**, which switches to the `openai` provider.
  - **Session ended** (refresh failed, or Linux reboot): "Your ChatGPT session ended" with **Continue with ChatGPT**.
- **First use:** a one-time modal "You're using your ChatGPT plan" with **Got it**, as [UX] says: "Show a welcome modal only the first time".
- **In use:** a "Using ChatGPT plan · Manage usage" line near the composer or model selector, per [UX]: "display **Using ChatGPT plan** near the composer or model selector".
- **Usage limit:** a "Usage limit reached" modal or compact card with **Manage usage** as the primary action, and no automatic fallback.
- **Branding:** use the approved button artwork only after checking the OpenAI brand guidelines (§4.8). Until then, ship a text-only **Continue with ChatGPT** button. Do not copy assets or components from the noncommercial DevKit.

### 4.6 Tests (no real credentials anywhere)

- **Rust unit tests (`crates/ai`):**
  - PKCE against the RFC 7636 appendix-B vector.
  - Authorize-URL contents: the exact scope string, `resource`, `127.0.0.1`, the `/auth/callback` path, `agent_name_hint` only on first registration, and `ext_agent_host_id` always.
  - Callback rules: state mismatch, `access_denied` (no exchange), missing `client_id` on registration, differing `client_id` on reauth.
  - `registration.json` written atomically with `0600`.
  - Error-code mapping for every row of §3.9.
- **Mock OAuth and API server (`wiremock`, already a dev-dependency):**
  - It serves discovery, a JWKS, the token endpoint (code and refresh grants, `invalid_grant`, `refresh_token_reused`), revocation, `/v1/models` and `/v1/responses` SSE.
  - ID tokens are signed at test time with a key pair generated inside the test, so nothing secret is committed. Wrong `iss`, `aud`, `exp` or `nonce` must each be rejected.
  - The loopback test drives the callback with a real `GET http://127.0.0.1:<port>/auth/callback?…` from the test's `open_url` closure.
  - Refresh: rotation replaces the stored refresh token, and two concurrent `bearer()` calls make exactly one token request.
  - Sign-out sends `token_type_hint=refresh_token` with the issued `client_id`.
- **Responses translation and parser:**
  - A request-body test asserts `store:false`, `stream:true`, `instructions`, the namespace wrapper, `function_call` and `function_call_output` items, and that **none** of the [PREV] unsupported field names appear.
  - SSE fixtures cover text, interleaved tool calls, `response.failed` with `subscription_sharing_usage_limit_exceeded`, and a stream cut off before `response.completed`.
- **Agent-loop integration (`wiremock`):** a full tool round trip on `chatgpt`; `401` → refresh → single retry; `429` usage limit surfaced with no retry and no provider fallback.
- **Frontend (vitest, plus `test:slow-scheduler`):** each Settings state, the welcome modal shown once, the usage-limit modal action, and that no bridge response or prop ever carries a token.
- **e2e (Playwright):** the Settings ChatGPT card renders from the fake backend, and sign-in switches to the signed-in state.
- **Manual pre-release check with a real Plus account** (not in CI): first registration, returning sign-in, refresh after an hour, sign-out and revocation, disconnect from ChatGPT settings, hitting a per-app weekly cap, and a Linux reboot.

### 4.7 Suggested PR sequence (one concern each)

1. `feat(ai): Responses wire format` (`responses.rs` with serializer, parser and tests; no user-visible change).
2. `feat(ai): ChatGPT OAuth session` (`chatgpt/` with oauth, id_token, session and store; mock-server tests).
3. `feat(ai): chatgpt provider` (the provider, `Credential`, agent-loop wiring, models, validate, error mapping).
4. `feat(desktop): ChatGPT sign-in commands` (Tauri commands, opener, e2e backend answers).
5. `feat(desktop): ChatGPT plan in Settings` (the card, indicator, welcome and usage-limit modals).
6. `docs:` updates to `api-reference.md`, `architecture.md`, `contributing.md` §7 and any site provider list. The site copy must still not overstate anything: plan usage is in preview, for Plus and Pro.

### 4.8 Risks and open questions

- **The preview can change under us.** Endpoints, required fields and the namespaced-tools rule are all marked preview [PREV]. Keep the protocol code in one module, and pin behaviour with the mock-server tests so a change shows up as one failing test.
- **Namespaced tools at edytlab's scale.** The agent offers about 93 tools (`docs/README.md`). The preview says to group them in namespaces, and `tool_search` (deferred loading) is unsupported. Verify early with a real account that one namespace of that size is accepted, and whether returned `function_call` items carry a `namespace` field that has to be echoed back.
- **Reasoning continuity with `store: false`.** The plan-usage pages do not say how a reasoning model's state carries across stateless turns. Test tool-heavy sessions, and check whether `include: ["reasoning.encrypted_content"]` is accepted, since it is not on the unsupported list.
- **Users' allowance.** Every agent round trip, and the mode classifier, spends the user's Plus or Pro allowance, which their Codex use also draws on [HELP]†. Consider skipping the classifier on `chatgpt`, and say plainly in the UI where usage goes.
- **Keychain limits.** The Windows 1280-character slot limit and Linux's non-durable keyring (#394) are handled by the split in §4.3. Measure real refresh-token lengths.
- **Brand assets.** openai.com/brand could not be read from this environment. Check the logo and button rules there before shipping the branded button.
- **If edytlab goes paid or hosted** (FAQ roadmap), file the interest form before offering plan usage in that product (§2.2).

---

## 5. Alternatives for users this does not cover

For Team, Business, Enterprise, Edu and Free users, users in an unserved region, or anyone who wants separate billing:

1. **An OpenAI API key.** This already works through the `openai` provider and is the fallback [ERR] itself recommends: "configure another supported billing path, such as the user's own API key."
2. **OpenRouter.** Already supported (`OPENROUTER_ID`), with one key for many vendors.
3. **OpenAI's own tools.** A user can run Codex with their ChatGPT plan directly. Apps can also drive `codex app-server` with a plan token: "configure it to send inference requests to the Responses API using an OAuth access token authorized for the user's ChatGPT plan" [APPSRV]. For edytlab that would mean replacing its own agent loop with Codex's and exposing the audio tools to it. That is far larger than §4 and not recommended.
4. **Ollama.** Local and keyless, already supported.

---

## 6. What to watch

- Changes to the [PREV] and [ERR] pages: unsupported fields, tool rules, and the `force_reconsent=true` rollout.
- Disconnect notifications, if OpenAI adds them ("OpenAI does not currently notify your tool" [ERR]).
- Plan eligibility widening to Go or Business for open-source clients, and any published numeric limits.
- Whether identity-only and plan usage for commercial or hosted apps opens beyond the waitlist ([QS], [CID]). This matters if edytlab's hosted subscription ships.

---

## Sources

Read on 2026-10-10. **†** marks pages that answered HTTP 403 to direct fetches from this environment; their quotes are search-index excerpts.

| Tag | Source |
|---|---|
| [QS] | https://developers.openai.com/siwc/quickstart |
| [OSS] | https://developers.openai.com/siwc/token-sharing-open-source |
| [SIGNIN] | https://developers.openai.com/siwc/token-sharing-open-source/sign-in |
| [SESS] | https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions |
| [TOK] | https://developers.openai.com/siwc/token-sharing-open-source/token-reference |
| [INF] | https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference |
| [PREV] | https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations |
| [ERR] | https://developers.openai.com/siwc/token-sharing-open-source/errors-and-recovery |
| [APPSRV] | https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server |
| [UX] | https://developers.openai.com/siwc/ui-ux-guidelines |
| [WEB] | https://developers.openai.com/siwc/website |
| [CID] | https://developers.openai.com/siwc/request-client-id |
| [COOK] | https://developers.openai.com/cookbook/articles/sign-in-with-chatgpt |
| [FC] | https://developers.openai.com/api/docs/guides/function-calling |
| [DEVKIT] | https://github.com/openai/sign-in-with-chatgpt-devkit (README) |
| [DKLIC] | https://github.com/openai/sign-in-with-chatgpt-devkit/blob/main/LICENSE |
| [CODEX-LOGIN] | https://github.com/openai/codex/blob/main/codex-rs/login/src/server.rs |
| [CODEX-PROV] | https://github.com/openai/codex/blob/main/codex-rs/model-provider-info/src/lib.rs |
| [RECAP] † | https://openai.com/index/devday-2026-recap/ |
| [NOTES] † | https://help.openai.com/en/articles/6825453-chatgpt-release-notes |
| [HELP] † | https://help.openai.com/en/articles/20001542-using-your-chatgpt-plan-in-other-apps-and-sites |
| [TOU] † | https://openai.com/policies/row-terms-of-use/ |
| [TAURI-OPENER] | https://v2.tauri.app/plugin/opener/ |
| [TAURI-DL] | https://v2.tauri.app/plugin/deep-linking/ |
| [TAURI-CAP] | https://v2.tauri.app/security/capabilities/ |
| [WINCRED] | https://learn.microsoft.com/en-us/windows/win32/api/wincred/ns-wincred-credentialw |
| [KEYRING] | https://github.com/open-source-cooperative/keyring-rs/blob/v3.6.3/src/windows.rs |

edytlab facts are cited by path in this repository at the commit this document was branched from.

**Not verified here:** no live sign-in or inference was attempted, since no credentials are available or appropriate. Everything in §3 is OpenAI's documented contract, not observed behaviour.
