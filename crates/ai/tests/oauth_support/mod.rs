//! Test-only RSA keys and JWT signing for the ID-token tests.
//!
//! The key is generated when the test runs, so no private key is committed.
//! `ring`, which `ai::oauth::oidc` verifies with, cannot generate RSA keys, so
//! this does it with `num-bigint`: two random primes of half the size (Miller-Rabin),
//! `e = 65537`, and PKCS#1 v1.5 signing with SHA-256 by CRT. It is a fixture,
//! not crypto anyone relies on: a signature it gets wrong fails `ring`'s
//! verification, which the accept-a-good-token test would show.

// Each test binary uses a subset.
#![allow(dead_code)]

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use num_bigint::BigUint;
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::{json, Value};

pub struct TestRsaKey {
    n: BigUint,
    e: BigUint,
    p: BigUint,
    q: BigUint,
    dp: BigUint,
    dq: BigUint,
    qinv: BigUint,
}

/// The DER prefix of a SHA-256 `DigestInfo` (RFC 8017 §9.2, note 1).
const SHA256_DIGEST_INFO: [u8; 19] = [
    0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05,
    0x00, 0x04, 0x20,
];

impl TestRsaKey {
    /// A fresh 2048-bit key.
    pub fn generate() -> Self {
        Self::generate_bits(2048)
    }

    /// A fresh key with a modulus of exactly `bits` bits (even).
    pub fn generate_bits(bits: u64) -> Self {
        let rng = SystemRandom::new();
        let e = BigUint::from(65_537u32);
        let one = BigUint::from(1u32);
        loop {
            let p = random_prime(&rng, bits / 2);
            let q = random_prime(&rng, bits / 2);
            if p == q {
                continue;
            }
            let n = &p * &q;
            if n.bits() != bits {
                continue;
            }
            let phi = (&p - &one) * (&q - &one);
            let Some(d) = e.modinv(&phi) else { continue };
            let dp = &d % (&p - &one);
            let dq = &d % (&q - &one);
            let Some(qinv) = q.modinv(&p) else { continue };
            return Self {
                n,
                e,
                p,
                q,
                dp,
                dq,
                qinv,
            };
        }
    }

    /// The public key as a JWK.
    pub fn jwk(&self, kid: &str) -> Value {
        json!({
            "kty": "RSA",
            "kid": kid,
            "use": "sig",
            "alg": "RS256",
            "n": URL_SAFE_NO_PAD.encode(self.n.to_bytes_be()),
            "e": URL_SAFE_NO_PAD.encode(self.e.to_bytes_be()),
        })
    }

    /// The modulus, big-endian, for the HS256 key-confusion test.
    pub fn modulus_bytes(&self) -> Vec<u8> {
        self.n.to_bytes_be()
    }

    /// RSASSA-PKCS1-v1_5 with SHA-256 (RFC 8017 §8.2.1).
    pub fn sign_rs256(&self, message: &[u8]) -> Vec<u8> {
        let k = self.n.bits().div_ceil(8) as usize;
        let hash = ring::digest::digest(&ring::digest::SHA256, message);
        let mut t = SHA256_DIGEST_INFO.to_vec();
        t.extend_from_slice(hash.as_ref());
        let mut em = vec![0x00, 0x01];
        em.resize(k - t.len() - 1, 0xff);
        em.push(0x00);
        em.extend_from_slice(&t);
        let m = BigUint::from_bytes_be(&em);
        // CRT: s = s2 + q * (qinv * (s1 - s2) mod p).
        let s1 = m.modpow(&self.dp, &self.p);
        let s2 = m.modpow(&self.dq, &self.q);
        let diff = (&s1 + &self.p - (&s2 % &self.p)) % &self.p;
        let h = (&self.qinv * diff) % &self.p;
        let s = s2 + h * &self.q;
        let mut out = s.to_bytes_be();
        while out.len() < k {
            out.insert(0, 0);
        }
        out
    }
}

/// A compact JWS: `b64(header).b64(claims).b64(signature)`.
pub fn jwt(header: &Value, claims: &Value, sign: impl FnOnce(&[u8]) -> Vec<u8>) -> String {
    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(header).unwrap()),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap())
    );
    let signature = sign(signing_input.as_bytes());
    format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(signature))
}

/// A compact JWS from raw JSON text, for headers and claims a `Value`
/// cannot hold, such as a repeated key.
pub fn jwt_raw(header: &str, claims: &str, sign: impl FnOnce(&[u8]) -> Vec<u8>) -> String {
    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header),
        URL_SAFE_NO_PAD.encode(claims)
    );
    let signature = sign(signing_input.as_bytes());
    format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(signature))
}

fn random_biguint(rng: &SystemRandom, bits: u64) -> BigUint {
    let mut bytes = vec![0u8; bits.div_ceil(8) as usize];
    rng.fill(&mut bytes).expect("OS random generator");
    BigUint::from_bytes_be(&bytes)
}

const SMALL_PRIMES: [u32; 54] = [
    3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83, 89, 97,
    101, 103, 107, 109, 113, 127, 131, 137, 139, 149, 151, 157, 163, 167, 173, 179, 181, 191, 193,
    197, 199, 211, 223, 227, 229, 233, 239, 241, 251, 257,
];

fn random_prime(rng: &SystemRandom, bits: u64) -> BigUint {
    loop {
        let mut candidate = random_biguint(rng, bits);
        // Top two bits set, so p * q has exactly 2 * bits bits; odd.
        candidate.set_bit(bits - 1, true);
        candidate.set_bit(bits - 2, true);
        candidate.set_bit(0, true);
        if SMALL_PRIMES
            .iter()
            .any(|&sp| (&candidate % sp) == BigUint::from(0u32))
        {
            continue;
        }
        if is_probable_prime(rng, &candidate, 24) {
            return candidate;
        }
    }
}

fn is_probable_prime(rng: &SystemRandom, n: &BigUint, rounds: usize) -> bool {
    let one = BigUint::from(1u32);
    let two = BigUint::from(2u32);
    let n_minus_one = n - &one;
    let mut d = n_minus_one.clone();
    let mut s = 0u32;
    while !d.bit(0) {
        d >>= 1;
        s += 1;
    }
    'witness: for round in 0..rounds {
        // Base 2 first: it rejects almost every composite in one modpow.
        let a = if round == 0 {
            two.clone()
        } else {
            two.clone() + random_biguint(rng, n.bits()) % (n - BigUint::from(3u32))
        };
        let mut x = a.modpow(&d, n);
        if x == one || x == n_minus_one {
            continue;
        }
        for _ in 1..s {
            x = x.modpow(&two, n);
            if x == n_minus_one {
                continue 'witness;
            }
        }
        return false;
    }
    true
}
