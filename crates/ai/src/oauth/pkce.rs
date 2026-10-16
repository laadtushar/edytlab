//! PKCE (RFC 7636) and the per-attempt random values, `state` and `nonce`.
//!
//! All randomness comes from the operating system's generator through
//! `ring::rand::SystemRandom` (`getrandom(2)`, `BCryptGenRandom`,
//! `getentropy`), never from a user-space PRNG. If the OS cannot supply it,
//! generation fails with [`RandomnessError`] rather than falling back.

use std::fmt;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ring::rand::{SecureRandom, SystemRandom};
use subtle::ConstantTimeEq;

use super::SecretString;

/// Bytes of entropy in a generated verifier: 64, which base64url-encodes to
/// 86 characters, inside RFC 7636's 43 to 128.
const VERIFIER_BYTES: usize = 64;
/// Bytes of entropy in `state` and `nonce`: 256 bits, 43 characters.
const RANDOM_VALUE_BYTES: usize = 32;

const VERIFIER_MIN_LEN: usize = 43;
const VERIFIER_MAX_LEN: usize = 128;

/// The operating system's random generator failed.
#[derive(Debug, thiserror::Error)]
#[error("the operating system's random number generator failed")]
pub struct RandomnessError;

/// A code verifier that breaks RFC 7636 §4.1.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VerifierError {
    #[error("a PKCE code verifier must be 43 to 128 characters long, not {0}")]
    Length(usize),
    #[error("a PKCE code verifier may use only A-Z, a-z, 0-9, '-', '.', '_' and '~'")]
    Charset,
}

fn random_base64url(bytes: usize) -> Result<String, RandomnessError> {
    let mut buf = vec![0u8; bytes];
    SystemRandom::new()
        .fill(&mut buf)
        .map_err(|_| RandomnessError)?;
    Ok(URL_SAFE_NO_PAD.encode(&buf))
}

fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
}

/// Compares two strings in time that depends on their lengths only, not on
/// where they first differ.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

/// The PKCE code verifier: secret until the token exchange sends it.
#[derive(Clone)]
pub struct PkceVerifier(SecretString);

impl PkceVerifier {
    /// A fresh verifier of 86 unreserved characters (64 random bytes).
    pub fn generate() -> Result<Self, RandomnessError> {
        Ok(Self(SecretString::new(random_base64url(VERIFIER_BYTES)?)))
    }

    /// A verifier from elsewhere, checked against RFC 7636 §4.1.
    pub fn new(value: SecretString) -> Result<Self, VerifierError> {
        let bytes = value.expose_secret().as_bytes();
        if !bytes.iter().all(|&b| is_unreserved(b)) {
            return Err(VerifierError::Charset);
        }
        if !(VERIFIER_MIN_LEN..=VERIFIER_MAX_LEN).contains(&bytes.len()) {
            return Err(VerifierError::Length(bytes.len()));
        }
        Ok(Self(value))
    }

    /// `BASE64URL(SHA256(ASCII(code_verifier)))`, unpadded (RFC 7636 §4.2).
    pub fn challenge(&self) -> PkceChallenge {
        let digest = ring::digest::digest(&ring::digest::SHA256, self.0.expose_secret().as_bytes());
        PkceChallenge(URL_SAFE_NO_PAD.encode(digest.as_ref()))
    }

    pub fn expose_secret(&self) -> &str {
        self.0.expose_secret()
    }
}

impl fmt::Debug for PkceVerifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PkceVerifier(<redacted>)")
    }
}

/// The S256 code challenge. Public: it goes in the authorization URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PkceChallenge(String);

impl PkceChallenge {
    /// The only method this client sends; `plain` is never used.
    pub const METHOD: &'static str = "S256";

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

macro_rules! random_value {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone)]
        pub struct $name(String);

        impl $name {
            /// 256 random bits, base64url-encoded (43 characters).
            pub fn generate() -> Result<Self, RandomnessError> {
                Ok(Self(random_base64url(RANDOM_VALUE_BYTES)?))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Whether `other` is this value, compared in constant time.
            pub fn matches(&self, other: &str) -> bool {
                constant_time_eq(&self.0, other)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($name), "(<redacted>)"))
            }
        }
    };
}

random_value!(
    /// The `state` of one authorization attempt (RFC 6749 §10.12): the
    /// callback must return it unchanged.
    State
);

random_value!(
    /// The `nonce` of one OpenID Connect sign-in: the ID token must carry it.
    Nonce
);
