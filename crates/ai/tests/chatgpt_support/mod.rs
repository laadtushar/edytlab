//! A fake OpenAI sign-in server for the ChatGPT session tests, and the
//! doubles the session takes: a clock the test moves, and an in-memory
//! refresh-token store standing in for the keychain slot.
//!
//! The server is `wiremock`, playing the documented endpoints under its own
//! address: discovery, the JWKS, the token endpoint (code and refresh
//! grants, with rotating refresh tokens that answer `refresh_token_reused`
//! when used twice) and revocation. ID tokens are signed with an RSA key
//! generated when the tests run (`oauth_support`), so nothing secret is
//! committed. The browser is a closure given to `sign_in` as its `open_url`:
//! it reads the authorization URL and makes a real GET to the loopback
//! callback.

// Each test binary uses a subset.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ai::chatgpt::session::{ChatGptSession, ChatGptStatus, Clock, SessionConfig, SignInError};
use ai::chatgpt::store::{RefreshTokenStore, SecretStoreError};
use ai::chatgpt::{Endpoints, SignInIntent};
use ai::oauth::SecretString;
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use crate::oauth_support::{jwt, TestRsaKey};

/// A hook the token endpoint runs before it answers a refresh, given the
/// refresh token sent.
pub type RefreshHook = Mutex<Option<Box<dyn Fn(&str) + Send + Sync>>>;
/// A hook the token endpoint runs before it answers a code exchange.
pub type ExchangeHook = Mutex<Option<Box<dyn Fn() + Send + Sync>>>;

pub const ISSUED_CLIENT: &str = "oaiapp_test_client";
pub const SUBJECT: &str = "user-subject-1";
pub const EMAIL: &str = "person@example.com";
pub const CODE: &str = "the-authorization-code";
pub const FULL_SCOPE: &str =
    "chatgpt.tokens.use.direct email offline_access openid profile resource.invoke";

pub fn key() -> &'static TestRsaKey {
    static KEY: OnceLock<TestRsaKey> = OnceLock::new();
    KEY.get_or_init(TestRsaKey::generate)
}

// --- The clock ------------------------------------------------------------------

#[derive(Debug)]
pub struct TestClock(Mutex<SystemTime>);

impl TestClock {
    pub fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new(SystemTime::now())))
    }

    pub fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }

    pub fn unix(&self) -> u64 {
        self.now().duration_since(UNIX_EPOCH).unwrap().as_secs()
    }
}

impl Clock for TestClock {
    fn now(&self) -> SystemTime {
        *self.0.lock().unwrap()
    }
}

// --- The keychain double -----------------------------------------------------------

/// The refresh-token slot, in memory. Shared by two sessions, it is the
/// keychain both processes of a cross-process test see.
#[derive(Default)]
pub struct MemoryStore {
    token: Mutex<Option<String>>,
    /// Returned by every `save` while set.
    pub save_error: Mutex<Option<SecretStoreError>>,
    /// Returned by every `load` while set.
    pub load_error: Mutex<Option<SecretStoreError>>,
    /// "save <token>", "delete" and "load", in order.
    pub log: Mutex<Vec<String>>,
}

impl MemoryStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn get(&self) -> Option<String> {
        self.token.lock().unwrap().clone()
    }

    pub fn set(&self, token: Option<&str>) {
        *self.token.lock().unwrap() = token.map(str::to_string);
    }

    pub fn saves(&self) -> Vec<String> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.starts_with("save "))
            .cloned()
            .collect()
    }
}

impl RefreshTokenStore for MemoryStore {
    fn load(&self) -> Result<Option<SecretString>, SecretStoreError> {
        self.log.lock().unwrap().push("load".into());
        if let Some(e) = self.load_error.lock().unwrap().clone() {
            return Err(e);
        }
        Ok(self.get().map(SecretString::new))
    }

    fn save(&self, token: &SecretString) -> Result<(), SecretStoreError> {
        if let Some(e) = self.save_error.lock().unwrap().clone() {
            return Err(e);
        }
        self.log
            .lock()
            .unwrap()
            .push(format!("save {}", token.expose_secret()));
        self.set(Some(token.expose_secret()));
        Ok(())
    }

    fn delete(&self) -> Result<(), SecretStoreError> {
        self.log.lock().unwrap().push("delete".into());
        self.set(None);
        Ok(())
    }
}

// --- The fake server ------------------------------------------------------------------

/// What the fake server does; tests change it between steps.
pub struct Knobs {
    pub subject: String,
    /// The ID token's `aud`; `None` for the client id it was issued to.
    pub audience: Option<String>,
    pub azp: Option<String>,
    pub scope: Option<String>,
    pub issue_refresh_token: bool,
    /// Refresh responses carry an ID token.
    pub refresh_id_token: bool,
    /// ...for this subject (`None`: the account's).
    pub refresh_subject: Option<String>,
    /// ...carrying the sign-in's nonce, as some issuers do.
    pub refresh_nonce: bool,
    /// ...signed under this `kid`, which the JWKS does not list.
    pub refresh_kid: Option<String>,
    /// `earliest_refresh_at`, this many seconds after issue.
    pub earliest_refresh_after: Option<u64>,
    pub expires_in: u64,
    /// Every refresh answers this.
    pub refresh_error: Option<(u16, Value)>,
    /// Every code exchange answers this.
    pub exchange_error: Option<(u16, Value)>,
    /// Revocation answers these in turn, then 200.
    pub revoke_statuses: Vec<u16>,
    /// Discovery's `authorization_response_iss_parameter_supported`.
    pub iss_parameter_supported: bool,
    /// Discovery's `revocation_endpoint`; `None` for the issuer's own.
    pub revocation_endpoint: Option<String>,
    /// How long discovery takes to answer.
    pub discovery_delay: Duration,
}

impl Default for Knobs {
    fn default() -> Self {
        Self {
            subject: SUBJECT.into(),
            audience: None,
            azp: None,
            scope: Some(FULL_SCOPE.into()),
            issue_refresh_token: true,
            refresh_id_token: false,
            refresh_subject: None,
            refresh_nonce: false,
            refresh_kid: None,
            earliest_refresh_after: None,
            expires_in: 3600,
            refresh_error: None,
            exchange_error: None,
            revoke_statuses: Vec::new(),
            iss_parameter_supported: false,
            revocation_endpoint: None,
            discovery_delay: Duration::ZERO,
        }
    }
}

#[derive(Default)]
struct Tokens {
    generation: u32,
    /// The one refresh token that works: rotation spends it.
    live_refresh: Option<String>,
    spent_refresh: Vec<String>,
    /// The nonce of the latest authorization request.
    nonce: Option<String>,
    /// When the user last authenticated: a refreshed ID token's
    /// `auth_time` is the original's (OpenID Connect Core §12.2).
    auth_time: Option<u64>,
}

pub struct Fake {
    pub server: MockServer,
    pub clock: Arc<TestClock>,
    pub knobs: Mutex<Knobs>,
    tokens: Mutex<Tokens>,
    /// The query of every authorization URL the browser opened.
    pub authorizations: Mutex<Vec<BTreeMap<String, String>>>,
    /// The form of every token-endpoint request.
    pub token_requests: Mutex<Vec<BTreeMap<String, String>>>,
    /// The form of every revocation request.
    pub revocations: Mutex<Vec<BTreeMap<String, String>>>,
    /// Runs inside the token endpoint before it answers a refresh, with the
    /// refresh token sent: a test's way to act "while the request is in
    /// flight".
    pub on_refresh: RefreshHook,
    /// Runs inside the token endpoint before it answers a code exchange.
    pub on_exchange: ExchangeHook,
}

pub fn form(body: &[u8]) -> BTreeMap<String, String> {
    url::form_urlencoded::parse(body)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

impl Fake {
    pub async fn start() -> Arc<Self> {
        let fake = Arc::new(Self {
            server: MockServer::start().await,
            clock: TestClock::new(),
            knobs: Mutex::new(Knobs::default()),
            tokens: Mutex::new(Tokens::default()),
            authorizations: Mutex::new(Vec::new()),
            token_requests: Mutex::new(Vec::new()),
            revocations: Mutex::new(Vec::new()),
            on_refresh: Mutex::new(None),
            on_exchange: Mutex::new(None),
        });
        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(Responder(fake.clone(), Endpoint::Discovery))
            .mount(&fake.server)
            .await;
        Mock::given(method("GET"))
            .and(path("/.well-known/jwks.json"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "keys": [key().jwk("k1")] })),
            )
            .mount(&fake.server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/accounts/oauth/token"))
            .respond_with(Responder(fake.clone(), Endpoint::Token))
            .mount(&fake.server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/accounts/oauth/revoke"))
            .respond_with(Responder(fake.clone(), Endpoint::Revoke))
            .mount(&fake.server)
            .await;
        fake
    }

    pub fn issuer(&self) -> String {
        self.server.uri()
    }

    pub fn knobs(&self) -> std::sync::MutexGuard<'_, Knobs> {
        self.knobs.lock().unwrap()
    }

    /// A session config pointed at this server, with its files in `dir`.
    pub fn config(&self, app_config_dir: &Path) -> SessionConfig {
        SessionConfig::new(app_config_dir)
            .with_endpoints(Endpoints::loopback_for_tests(&self.issuer()).unwrap())
            .with_clock(self.clock.clone())
            .with_preferred_port(None)
            .with_sign_in_timeout(Duration::from_secs(20))
            .with_lock_timeout(Duration::from_secs(10))
            .with_revoke_retry_delays(vec![Duration::from_millis(10); 3])
    }

    pub fn session(&self, app_config_dir: &Path, store: Arc<MemoryStore>) -> ChatGptSession {
        ChatGptSession::open(self.config(app_config_dir), store).unwrap()
    }

    /// Plays another writer that refreshed first: spends the live refresh
    /// token and returns the one that replaced it.
    pub fn rotate_elsewhere(&self) -> String {
        let body = self.mint(ISSUED_CLIENT, None);
        body["refresh_token"].as_str().unwrap().to_string()
    }

    /// The refresh token that works right now.
    pub fn live_refresh_token(&self) -> Option<String> {
        self.tokens.lock().unwrap().live_refresh.clone()
    }

    pub fn refresh_requests(&self) -> Vec<BTreeMap<String, String>> {
        self.token_requests
            .lock()
            .unwrap()
            .iter()
            .filter(|f| f.get("grant_type").map(String::as_str) == Some("refresh_token"))
            .cloned()
            .collect()
    }

    pub fn exchange_requests(&self) -> Vec<BTreeMap<String, String>> {
        self.token_requests
            .lock()
            .unwrap()
            .iter()
            .filter(|f| f.get("grant_type").map(String::as_str) == Some("authorization_code"))
            .cloned()
            .collect()
    }

    pub fn id_token(&self, client_id: &str, nonce: Option<&str>, subject: &str) -> String {
        self.id_token_with_kid(client_id, nonce, subject, "k1")
    }

    pub fn id_token_with_kid(
        &self,
        client_id: &str,
        nonce: Option<&str>,
        subject: &str,
        kid: &str,
    ) -> String {
        let auth_time = self.tokens.lock().unwrap().auth_time;
        let knobs = self.knobs();
        let now = self.clock.unix();
        let mut claims = json!({
            "iss": self.issuer(),
            "sub": subject,
            "aud": knobs.audience.clone().unwrap_or_else(|| client_id.to_string()),
            "exp": now + 3600,
            "iat": now,
            "auth_time": auth_time.unwrap_or(now),
            "email": EMAIL,
            "email_verified": true,
        });
        if let Some(nonce) = nonce {
            claims["nonce"] = nonce.into();
        }
        if let Some(azp) = &knobs.azp {
            claims["azp"] = azp.clone().into();
        }
        jwt(
            &json!({ "alg": "RS256", "typ": "JWT", "kid": kid }),
            &claims,
            |m| key().sign_rs256(m),
        )
    }

    fn mint(&self, client_id: &str, id_token: Option<String>) -> Value {
        let knobs = self.knobs();
        let mut tokens = self.tokens.lock().unwrap();
        tokens.generation += 1;
        let n = tokens.generation;
        let mut body = json!({
            "access_token": format!("AT-{n}"),
            "token_type": "Bearer",
            "expires_in": knobs.expires_in,
        });
        if let Some(scope) = &knobs.scope {
            body["scope"] = scope.clone().into();
        }
        if knobs.issue_refresh_token {
            let rt = format!("RT-{n}");
            if let Some(old) = tokens.live_refresh.replace(rt.clone()) {
                tokens.spent_refresh.push(old);
            }
            body["refresh_token"] = rt.into();
        }
        if let Some(after) = knobs.earliest_refresh_after {
            body["earliest_refresh_at"] = (self.clock.unix() + after).into();
        }
        if let Some(id_token) = id_token {
            body["id_token"] = id_token.into();
        }
        let _ = client_id;
        body
    }

    fn token_endpoint(&self, request: &Request) -> ResponseTemplate {
        let form = form(&request.body);
        self.token_requests.lock().unwrap().push(form.clone());
        let client_id = form.get("client_id").cloned().unwrap_or_default();
        match form.get("grant_type").map(String::as_str) {
            Some("authorization_code") => {
                if let Some(hook) = self.on_exchange.lock().unwrap().as_ref() {
                    hook();
                }
                if let Some((status, body)) = self.knobs().exchange_error.clone() {
                    return ResponseTemplate::new(status).set_body_json(body);
                }
                if form.get("code").map(String::as_str) != Some(CODE) {
                    return ResponseTemplate::new(400)
                        .set_body_json(json!({ "error": "invalid_grant" }));
                }
                let nonce = {
                    let mut tokens = self.tokens.lock().unwrap();
                    tokens.auth_time = Some(self.clock.unix());
                    tokens.nonce.clone()
                };
                let subject = self.knobs().subject.clone();
                let id_token = self.id_token(&client_id, nonce.as_deref(), &subject);
                ResponseTemplate::new(200).set_body_json(self.mint(&client_id, Some(id_token)))
            }
            Some("refresh_token") => {
                let sent = form.get("refresh_token").cloned().unwrap_or_default();
                if let Some(hook) = self.on_refresh.lock().unwrap().as_ref() {
                    hook(&sent);
                }
                if let Some((status, body)) = self.knobs().refresh_error.clone() {
                    return ResponseTemplate::new(status).set_body_json(body);
                }
                let (live, spent) = {
                    let t = self.tokens.lock().unwrap();
                    (
                        t.live_refresh.as_deref() == Some(sent.as_str()),
                        t.spent_refresh.contains(&sent),
                    )
                };
                if spent {
                    // The wrapped shape, which Codex CLI also reads.
                    return ResponseTemplate::new(401).set_body_json(json!({
                        "error": { "code": "refresh_token_reused", "message": "Already used." }
                    }));
                }
                if !live {
                    return ResponseTemplate::new(400)
                        .set_body_json(json!({ "error": "invalid_grant" }));
                }
                let id_token = {
                    let knobs = self.knobs();
                    knobs.refresh_id_token.then(|| {
                        (
                            knobs
                                .refresh_subject
                                .clone()
                                .unwrap_or_else(|| knobs.subject.clone()),
                            knobs.refresh_nonce,
                            knobs.refresh_kid.clone().unwrap_or_else(|| "k1".into()),
                        )
                    })
                }
                .map(|(subject, with_nonce, kid)| {
                    let nonce = with_nonce
                        .then(|| self.tokens.lock().unwrap().nonce.clone())
                        .flatten();
                    self.id_token_with_kid(&client_id, nonce.as_deref(), &subject, &kid)
                });
                ResponseTemplate::new(200).set_body_json(self.mint(&client_id, id_token))
            }
            _ => ResponseTemplate::new(400)
                .set_body_json(json!({ "error": "unsupported_grant_type" })),
        }
    }

    fn revoke_endpoint(&self, request: &Request) -> ResponseTemplate {
        self.revocations.lock().unwrap().push(form(&request.body));
        let mut knobs = self.knobs();
        if knobs.revoke_statuses.is_empty() {
            ResponseTemplate::new(200)
        } else {
            ResponseTemplate::new(knobs.revoke_statuses.remove(0))
        }
    }

    fn discovery(&self) -> ResponseTemplate {
        let uri = self.issuer();
        let knobs = self.knobs();
        ResponseTemplate::new(200)
            .set_delay(knobs.discovery_delay)
            .set_body_json(json!({
                "issuer": uri,
                "authorization_endpoint": format!("{uri}/api/accounts/authorize"),
                "token_endpoint": format!("{uri}/api/accounts/oauth/token"),
                "jwks_uri": format!("{uri}/.well-known/jwks.json"),
                "revocation_endpoint": knobs
                    .revocation_endpoint
                    .clone()
                    .unwrap_or_else(|| format!("{uri}/api/accounts/oauth/revoke")),
                "id_token_signing_alg_values_supported": ["RS256"],
                "response_types_supported": ["code"],
                "authorization_response_iss_parameter_supported": knobs.iss_parameter_supported,
            }))
    }
}

enum Endpoint {
    Discovery,
    Token,
    Revoke,
}

struct Responder(Arc<Fake>, Endpoint);

impl Respond for Responder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        match self.1 {
            Endpoint::Discovery => self.0.discovery(),
            Endpoint::Token => self.0.token_endpoint(request),
            Endpoint::Revoke => self.0.revoke_endpoint(request),
        }
    }
}

// --- The browser ---------------------------------------------------------------------

/// What the browser's redirect back to the app carries.
#[derive(Clone)]
pub struct Browser {
    /// `client_id` on the callback. `Default` sends the issued one on a
    /// registration and none on a returning sign-in.
    pub client_id: CallbackClient,
    pub error: Option<String>,
    /// `iss` on the callback; `None` leaves it out.
    pub iss: Option<String>,
    /// `state` on the callback; `None` echoes the request's.
    pub state: Option<String>,
}

#[derive(Clone)]
pub enum CallbackClient {
    Default,
    Omit,
    Send(String),
}

impl Default for Browser {
    fn default() -> Self {
        Self {
            client_id: CallbackClient::Default,
            error: None,
            iss: None,
            state: None,
        }
    }
}

pub fn query(url: &str) -> BTreeMap<String, String> {
    let url = url::Url::parse(url).unwrap();
    let mut map = BTreeMap::new();
    for (k, v) in url.query_pairs() {
        assert!(
            map.insert(k.to_string(), v.into_owned()).is_none(),
            "{k} repeated in {url}"
        );
    }
    map
}

impl Fake {
    /// Signs in through the fake browser.
    pub async fn sign_in(
        self: &Arc<Self>,
        session: &ChatGptSession,
        browser: Browser,
        intent: SignInIntent,
    ) -> Result<ChatGptStatus, SignInError> {
        let fake = self.clone();
        session
            .sign_in(
                move |url: &str| -> Result<(), String> {
                    let q = query(url);
                    fake.authorizations.lock().unwrap().push(q.clone());
                    fake.tokens.lock().unwrap().nonce = q.get("nonce").cloned();
                    let registering = q["client_id"] == "dynamic_agent_client";
                    let mut callback = url::Url::parse(&q["redirect_uri"]).unwrap();
                    {
                        let mut pairs = callback.query_pairs_mut();
                        match &browser.error {
                            Some(error) => pairs.append_pair("error", error),
                            None => pairs.append_pair("code", CODE),
                        };
                        pairs.append_pair("state", browser.state.as_deref().unwrap_or(&q["state"]));
                        match &browser.client_id {
                            CallbackClient::Default if registering => {
                                pairs.append_pair("client_id", ISSUED_CLIENT);
                            }
                            CallbackClient::Send(id) => {
                                pairs.append_pair("client_id", id);
                            }
                            CallbackClient::Default | CallbackClient::Omit => {}
                        }
                        pairs.append_pair("scope", FULL_SCOPE);
                        if let Some(iss) = &browser.iss {
                            pairs.append_pair("iss", iss);
                        }
                    }
                    tokio::spawn(async move {
                        let http = reqwest::Client::builder()
                            .no_proxy()
                            .redirect(reqwest::redirect::Policy::none())
                            .build()
                            .unwrap();
                        let _ = http.get(callback).send().await;
                    });
                    Ok(())
                },
                intent,
            )
            .await
    }

    /// A first sign-in that succeeds.
    pub async fn signed_in(
        self: &Arc<Self>,
        dir: &Path,
        store: Arc<MemoryStore>,
    ) -> ChatGptSession {
        let session = self.session(dir, store);
        self.sign_in(&session, Browser::default(), SignInIntent::SignIn)
            .await
            .expect("the first sign-in succeeds");
        session
    }
}
