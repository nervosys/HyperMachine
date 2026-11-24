//! Signatures over shields: ML-DSA-87 (FIPS 204), from IronCrypto.
//!
//! What is signed is the shield's JSON exactly as it is carried, so there is
//! no canonical form to get wrong: the verifier checks the bytes it was given
//! and then parses those same bytes. The signature is bound to its purpose by
//! FIPS 204's context string, so a signature made for anything else does not
//! verify as a shield's.
//!
//! A [`Keyring`] is the list of signers a node trusts, and what it has
//! revoked: signers, and shields by digest. A revoked shield is refused when
//! it is loaded, and removed from a node that is running it when the keyring
//! is next applied.

use std::collections::{BTreeMap, BTreeSet};

use ic_mldsa::sign87 as mldsa;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::spec::ShieldSpec;
use crate::{from_hex, sha256, to_hex, Result, ShieldError};

/// FIPS 204 context for a shield's signature.
pub const SHIELD_CONTEXT: &[u8] = b"hypermachine shield v1";

/// FIPS 204 context for a directive's signature.
pub const DIRECTIVE_CONTEXT: &[u8] = b"hypermachine shield directive v1";

/// How long a directive may be applied after it was issued. A node refuses
/// one it has applied before, for as long as it could still be applied.
pub const DIRECTIVE_WINDOW_SECS: u64 = 600;

/// How far ahead of a node's clock a directive may say it was issued.
pub const DIRECTIVE_SKEW_SECS: u64 = 60;

/// An ML-DSA-87 signing key, kept as its 32-byte FIPS 204 seed.
pub struct SigningKey {
    seed: Zeroizing<[u8; 32]>,
    public: PublicKey,
}

impl std::fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SigningKey")
            .field("id", &self.public.id())
            .finish_non_exhaustive()
    }
}

impl SigningKey {
    /// A new key from the operating system's randomness.
    ///
    /// # Errors
    ///
    /// If there is no randomness, or the key fails FIPS 204's pairwise check.
    pub fn generate() -> Result<Self> {
        let mut seed = Zeroizing::new([0u8; 32]);
        let mut rng = ic_drbg::Rng::from_os().map_err(|_| key_error("no randomness"))?;
        rng.fill(&mut seed[..])
            .map_err(|_| key_error("no randomness"))?;
        Self::from_seed(&seed)
    }

    /// The key whose seed this is.
    ///
    /// # Errors
    ///
    /// If the key fails FIPS 204's pairwise check.
    pub fn from_seed(seed: &[u8; 32]) -> Result<Self> {
        let mut pk = [0u8; mldsa::PUBLIC_KEY_LEN];
        let mut sk = Zeroizing::new([0u8; mldsa::SECRET_KEY_LEN]);
        if !mldsa::keygen(seed, &mut pk, &mut sk) {
            return Err(key_error("the key failed its pairwise consistency test"));
        }
        Ok(Self {
            seed: Zeroizing::new(*seed),
            public: PublicKey(pk.to_vec()),
        })
    }

    /// The key from its seed in hex, as [`SigningKey::seed_hex`] writes it.
    ///
    /// # Errors
    ///
    /// If it is not 64 hex digits.
    pub fn from_seed_hex(text: &str) -> Result<Self> {
        let bytes = Zeroizing::new(from_hex(text.trim()).ok_or_else(|| key_error("not hex"))?);
        let seed: &[u8; 32] = bytes[..]
            .try_into()
            .map_err(|_| key_error("a seed is 32 bytes"))?;
        Self::from_seed(seed)
    }

    /// The seed in hex: the whole secret.
    pub fn seed_hex(&self) -> Zeroizing<String> {
        Zeroizing::new(to_hex(&self.seed[..]))
    }

    pub fn public_key(&self) -> &PublicKey {
        &self.public
    }

    /// Sign `message` under `context`, hedged with fresh randomness.
    ///
    /// # Errors
    ///
    /// If there is no randomness, or the context is over 255 bytes.
    pub fn sign(&self, message: &[u8], context: &[u8]) -> Result<Vec<u8>> {
        let mut pk = [0u8; mldsa::PUBLIC_KEY_LEN];
        let mut sk = Zeroizing::new([0u8; mldsa::SECRET_KEY_LEN]);
        if !mldsa::keygen(&self.seed, &mut pk, &mut sk) {
            return Err(key_error("the key failed its pairwise consistency test"));
        }
        let mut rnd = Zeroizing::new([0u8; 32]);
        let mut rng = ic_drbg::Rng::from_os().map_err(|_| key_error("no randomness"))?;
        rng.fill(&mut rnd[..])
            .map_err(|_| key_error("no randomness"))?;
        let mut signature = [0u8; mldsa::SIGNATURE_LEN];
        if !mldsa::sign(&sk, message, context, &rnd, &mut signature) {
            return Err(key_error("signing failed"));
        }
        Ok(signature.to_vec())
    }
}

fn key_error(why: &str) -> ShieldError {
    ShieldError::Key(why.to_string())
}

/// An ML-DSA-87 public key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKey(Vec<u8>);

impl PublicKey {
    /// The key from its bytes in hex.
    ///
    /// # Errors
    ///
    /// If it is not an ML-DSA-87 public key's length in hex.
    pub fn from_hex(text: &str) -> Result<Self> {
        let bytes = from_hex(text.trim()).ok_or_else(|| key_error("not hex"))?;
        if bytes.len() != mldsa::PUBLIC_KEY_LEN {
            return Err(key_error("not an ML-DSA-87 public key"));
        }
        Ok(Self(bytes))
    }

    pub fn to_hex(&self) -> String {
        to_hex(&self.0)
    }

    /// The first 8 bytes of the key's SHA-256, in hex: how it is named.
    pub fn id(&self) -> String {
        to_hex(&sha256(&self.0)[..8])
    }

    /// Whether `signature` is this key's over `message` under `context`.
    pub fn verify(&self, message: &[u8], context: &[u8], signature: &[u8]) -> bool {
        let Ok(pk) = <&[u8; mldsa::PUBLIC_KEY_LEN]>::try_from(&self.0[..]) else {
            return false;
        };
        let Ok(sig) = <&[u8; mldsa::SIGNATURE_LEN]>::try_from(signature) else {
            return false;
        };
        mldsa::verify(pk, message, context, sig)
    }
}

/// A shield as it travels: its JSON, who signed it, and the signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SignedShield {
    /// The shield's JSON: the exact bytes signed.
    pub spec: String,
    /// The signer's key ID ([`PublicKey::id`]).
    pub signer: String,
    /// The ML-DSA-87 signature, in hex.
    pub signature: String,
}

impl SignedShield {
    /// Sign a shield's JSON. The JSON is checked first: nothing that would be
    /// refused is signed.
    ///
    /// # Errors
    ///
    /// If the shield is not valid, or signing fails.
    pub fn sign(spec_json: &str, key: &SigningKey) -> Result<Self> {
        ShieldSpec::parse(spec_json)?;
        let signature = key.sign(spec_json.as_bytes(), SHIELD_CONTEXT)?;
        Ok(Self {
            spec: spec_json.to_string(),
            signer: key.public_key().id(),
            signature: to_hex(&signature),
        })
    }

    /// SHA-256 of the shield's JSON: what names this version of it.
    pub fn digest(&self) -> [u8; 32] {
        sha256(self.spec.as_bytes())
    }

    /// Check the signature against `keyring`, then parse the bytes it covers.
    ///
    /// # Errors
    ///
    /// If the signer is not trusted or revoked, the signature does not
    /// verify, the shield is revoked, or it is not a valid shield.
    pub fn verify(&self, keyring: &Keyring) -> Result<ShieldSpec> {
        keyring.check(
            &self.signer,
            self.spec.as_bytes(),
            SHIELD_CONTEXT,
            &self.signature,
        )?;
        let digest = to_hex(&self.digest());
        if keyring.revoked_shields.contains(&digest) {
            return Err(ShieldError::Signature(format!(
                "shield {digest} is revoked"
            )));
        }
        ShieldSpec::parse(&self.spec)
    }
}

/// What an operator tells a node to do with a loaded shield.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    /// Refuse what it matches.
    Enforce,
    /// Record what it matches, and refuse nothing.
    Monitor,
    /// Take it out of force.
    Remove,
}

/// A signed instruction about one shield, named by its whole digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Directive {
    pub action: Action,
    /// The shield's SHA-256, all 64 hex digits.
    pub shield: String,
    /// Seconds since the epoch: a directive is good for
    /// [`DIRECTIVE_WINDOW_SECS`] from then.
    pub issued_at: u64,
}

/// A directive as it travels: its JSON, who signed it, and the signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SignedDirective {
    /// The directive's JSON: the exact bytes signed.
    pub directive: String,
    pub signer: String,
    pub signature: String,
}

impl SignedDirective {
    /// Sign a directive.
    ///
    /// # Errors
    ///
    /// If it names no whole digest, or signing fails.
    pub fn sign(directive: &Directive, key: &SigningKey) -> Result<Self> {
        check_digest(&directive.shield)?;
        let json = serde_json::to_string(directive)
            .map_err(|e| ShieldError::Invalid(format!("directive: {e}")))?;
        let signature = key.sign(json.as_bytes(), DIRECTIVE_CONTEXT)?;
        Ok(Self {
            directive: json,
            signer: key.public_key().id(),
            signature: to_hex(&signature),
        })
    }

    /// SHA-256 of the directive's JSON: what a node remembers it by.
    pub fn digest(&self) -> [u8; 32] {
        sha256(self.directive.as_bytes())
    }

    /// Check the signature against `keyring`, then parse the bytes it covers
    /// and check that it may be applied at `now`.
    ///
    /// # Errors
    ///
    /// If the signer is not trusted or revoked, the signature does not
    /// verify, it does not parse, or it is not yet or no longer good.
    pub fn verify(&self, keyring: &Keyring, now: u64) -> Result<Directive> {
        keyring.check(
            &self.signer,
            self.directive.as_bytes(),
            DIRECTIVE_CONTEXT,
            &self.signature,
        )?;
        let directive: Directive = serde_json::from_str(&self.directive)
            .map_err(|e| ShieldError::Invalid(format!("directive: {e}")))?;
        check_digest(&directive.shield)?;
        if directive.issued_at > now.saturating_add(DIRECTIVE_SKEW_SECS) {
            return Err(ShieldError::Invalid(
                "the directive was issued in the future".into(),
            ));
        }
        if now > directive.issued_at.saturating_add(DIRECTIVE_WINDOW_SECS) {
            return Err(ShieldError::Invalid(format!(
                "the directive was issued more than {DIRECTIVE_WINDOW_SECS} seconds ago"
            )));
        }
        Ok(directive)
    }
}

fn check_digest(digest: &str) -> Result<()> {
    if digest.len() == 64
        && digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(())
    } else {
        Err(ShieldError::Invalid(
            "a directive names its shield by all 64 lowercase hex digits of its SHA-256".into(),
        ))
    }
}

/// The signers a node trusts, and what is revoked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keyring {
    keys: BTreeMap<String, PublicKey>,
    revoked_keys: BTreeSet<String>,
    revoked_shields: BTreeSet<String>,
}

/// A keyring as a file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct KeyringFile {
    #[serde(default)]
    pub keys: Vec<KeyringEntry>,
    /// Signer IDs no longer trusted.
    #[serde(default)]
    pub revoked_keys: Vec<String>,
    /// Shield digests (SHA-256 of their JSON, hex) no longer loaded.
    #[serde(default)]
    pub revoked_shields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct KeyringEntry {
    /// The public key in hex.
    pub public_key: String,
    /// Who holds it, for people reading the file.
    #[serde(default)]
    pub label: String,
}

impl Keyring {
    /// A keyring from its file's JSON.
    ///
    /// # Errors
    ///
    /// If it does not parse or a key is not an ML-DSA-87 public key.
    pub fn parse(json: &str) -> Result<Self> {
        let file: KeyringFile =
            serde_json::from_str(json).map_err(|e| key_error(&format!("keyring: {e}")))?;
        let mut keyring = Self::default();
        for entry in &file.keys {
            keyring.trust(PublicKey::from_hex(&entry.public_key)?);
        }
        keyring.revoked_keys = file.revoked_keys.into_iter().collect();
        for digest in file.revoked_shields {
            let digest = digest.to_ascii_lowercase();
            if from_hex(&digest).is_none_or(|d| d.len() != 32) {
                return Err(key_error("a revoked shield is named by its SHA-256 in hex"));
            }
            keyring.revoked_shields.insert(digest);
        }
        Ok(keyring)
    }

    /// Whether `signature` is `signer`'s over `message`, and `signer` is
    /// trusted and not revoked.
    fn check(&self, signer: &str, message: &[u8], context: &[u8], signature: &str) -> Result<()> {
        let sig_error = |why: String| ShieldError::Signature(why);
        let Some(key) = self.keys.get(signer) else {
            return Err(sig_error(format!("{signer} is not a trusted signer")));
        };
        if self.revoked_keys.contains(signer) {
            return Err(sig_error(format!("signer {signer} is revoked")));
        }
        let signature = from_hex(signature).ok_or_else(|| sig_error("not hex".into()))?;
        if !key.verify(message, context, &signature) {
            return Err(sig_error("the signature does not verify".into()));
        }
        Ok(())
    }

    pub fn trust(&mut self, key: PublicKey) {
        self.keys.insert(key.id(), key);
    }

    pub fn revoke_shield(&mut self, digest: &[u8; 32]) {
        self.revoked_shields.insert(to_hex(digest));
    }

    pub fn revoke_key(&mut self, id: &str) {
        self.revoked_keys.insert(id.to_string());
    }

    /// Whether a loaded shield, signed by `signer`, is now revoked.
    pub fn revokes(&self, signer: &str, digest: &[u8; 32]) -> bool {
        self.revoked_keys.contains(signer)
            || !self.keys.contains_key(signer)
            || self.revoked_shields.contains(&to_hex(digest))
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// The IDs of the trusted signers.
    pub fn signers(&self) -> impl Iterator<Item = &str> {
        self.keys.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: &str = r#"{"name":"cve-x","syscalls":[{"syscall":"ptrace","action":{"kind":"errno","errno":1}}],"expiresAt":4102444800}"#;

    #[test]
    fn a_signed_shield_verifies_only_as_it_was_signed() {
        let key = SigningKey::generate().unwrap();
        let mut keyring = Keyring::default();
        keyring.trust(key.public_key().clone());
        let signed = SignedShield::sign(SPEC, &key).unwrap();
        assert_eq!(signed.verify(&keyring).unwrap().name, "cve-x");

        // One byte of the shield changed.
        let mut changed = signed.clone();
        changed.spec = changed.spec.replace("\"errno\":1", "\"errno\":2");
        assert!(changed.verify(&keyring).is_err());

        // The signature cut, or not hex.
        let mut cut = signed.clone();
        cut.signature.truncate(cut.signature.len() - 2);
        assert!(cut.verify(&keyring).is_err());
        let mut odd = signed.clone();
        odd.signature.push('z');
        assert!(odd.verify(&keyring).is_err());

        // A signature for something else, by the same key, is not a shield's.
        let other = key.sign(SPEC.as_bytes(), b"something else").unwrap();
        let mut misused = signed.clone();
        misused.signature = to_hex(&other);
        assert!(misused.verify(&keyring).is_err());

        // An untrusted signer, and revocations.
        assert!(signed.verify(&Keyring::default()).is_err());
        let mut revoked = keyring.clone();
        revoked.revoke_shield(&signed.digest());
        assert!(signed.verify(&revoked).is_err());
        assert!(revoked.revokes(&signed.signer, &signed.digest()));
        let mut key_revoked = keyring.clone();
        key_revoked.revoke_key(&signed.signer);
        assert!(signed.verify(&key_revoked).is_err());
        assert!(!keyring.revokes(&signed.signer, &signed.digest()));
    }

    #[test]
    fn nothing_invalid_is_signed() {
        let key = SigningKey::generate().unwrap();
        assert!(SignedShield::sign(r#"{"name":"a","expiresAt":1}"#, &key).is_err());
    }

    #[test]
    fn keys_round_trip_through_their_text() {
        let key = SigningKey::generate().unwrap();
        let again = SigningKey::from_seed_hex(&key.seed_hex()).unwrap();
        assert_eq!(again.public_key(), key.public_key());
        let public = PublicKey::from_hex(&key.public_key().to_hex()).unwrap();
        assert_eq!(public.id(), key.public_key().id());
        assert!(PublicKey::from_hex("00").is_err());
        assert!(SigningKey::from_seed_hex("00").is_err());

        let file = format!(
            r#"{{"keys":[{{"publicKey":"{}","label":"ops"}}],"revokedShields":["{}"]}}"#,
            key.public_key().to_hex(),
            "ab".repeat(32)
        );
        let keyring = Keyring::parse(&file).unwrap();
        assert_eq!(
            keyring.signers().collect::<Vec<_>>(),
            [key.public_key().id()]
        );
        assert!(Keyring::parse(r#"{"revokedShields":["ab"]}"#).is_err());
        assert!(Keyring::parse(r#"{"keys":[],"other":1}"#).is_err());
    }
}
