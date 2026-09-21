//! Integration tests for crypto modules
//!
//! Tests FIPS symmetric crypto operations.
//!
//! These used to be written in halves: exercise the operation when the `ring`
//! feature was on, assert it returned `CryptoError::NotImplemented` when it was
//! off. The primitives come from IronCrypto now and are unconditional, so there
//! is no configuration in which they are absent and every test asserts the
//! operation succeeds.

use hv2_core::crypto::fips::{AesKeySize, FipsCrypto, FipsMode};

mod fips_integration {
    use super::*;

    fn get_crypto() -> FipsCrypto {
        // Disabled mode skips the power-on self-tests, which the tests below
        // exercise on their own.
        FipsCrypto::new(FipsMode::Disabled).unwrap()
    }

    #[test]
    fn test_fips_crypto_init() {
        let crypto = get_crypto();
        // Disabled mode does not enable FIPS
        assert!(!crypto.is_fips_enabled());
    }

    #[test]
    fn test_fips_crypto_encrypt_roundtrip() {
        let crypto = get_crypto();

        let key = crypto
            .generate_aes_key(AesKeySize::Aes256)
            .expect("Failed to generate key");
        assert_eq!(key.len(), 32);

        let plaintext = b"Integration test message for FIPS crypto";
        let aad = b"additional authenticated data";

        let ciphertext = crypto
            .aes_gcm_encrypt(key.as_bytes(), plaintext, aad)
            .expect("AES-GCM encrypt");
        let decrypted = crypto
            .aes_gcm_decrypt(key.as_bytes(), &ciphertext, aad)
            .expect("Decryption failed");
        assert_eq!(plaintext.as_slice(), decrypted.as_slice());
    }

    #[test]
    fn test_fips_hash_is_deterministic() {
        let crypto = get_crypto();
        let data = b"Test data for hash consistency check";

        let hash1 = crypto.sha256(data).expect("sha256");
        let hash2 = crypto.sha256(data).unwrap();
        assert_eq!(hash1, hash2);
        let hash3 = crypto.sha256(b"Different data").unwrap();
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_fips_hmac_is_deterministic() {
        let crypto = get_crypto();
        let key = vec![0xab; 32];
        let message = b"Message to authenticate";

        let mac1 = crypto.hmac_sha256(&key, message).expect("hmac");
        let mac2 = crypto.hmac_sha256(&key, message).unwrap();
        assert_eq!(mac1, mac2);
        let mac3 = crypto.hmac_sha256(&key, b"Different message").unwrap();
        assert_ne!(mac1, mac3);
    }

    #[test]
    fn test_fips_hkdf_derives() {
        let crypto = get_crypto();
        let ikm = vec![0x01; 32];
        let salt = vec![0x02; 32];
        let info = b"key derivation context";

        let derived_32 = crypto.hkdf_sha256(&salt, &ikm, info, 32).expect("hkdf");
        let derived_64 = crypto.hkdf_sha256(&salt, &ikm, info, 64).expect("hkdf");
        assert_eq!(derived_32.len(), 32);
        assert_eq!(derived_64.len(), 64);
        // HKDF-Expand is a prefix function of the output length: the first 32
        // bytes of a 64-byte derivation are the 32-byte derivation.
        assert_eq!(&derived_32[..], &derived_64[..32]);

        // And `info` has to matter, or the context separation is a fiction.
        let other = crypto
            .hkdf_sha256(&salt, &ikm, b"other context", 32)
            .expect("hkdf");
        assert_ne!(derived_32, other);
    }

    #[test]
    fn test_fips_random_uniqueness() {
        let crypto = get_crypto();

        let mut buf1 = vec![0u8; 32];
        let mut buf2 = vec![0u8; 32];

        crypto.random_bytes(&mut buf1).unwrap();
        crypto.random_bytes(&mut buf2).unwrap();

        // Random outputs should be unique (with overwhelming probability)
        assert_ne!(buf1, buf2);
    }

    #[test]
    fn test_fips_self_tests() {
        let mut crypto = get_crypto();
        // These used to fail without `ring`, correctly: every primitive they
        // exercise returned NotImplemented, so the self-tests reported the
        // crypto as broken. Nothing can be absent now.
        let result = crypto.run_self_tests();
        assert!(result.is_ok(), "self-tests: {result:?}");
    }

    #[test]
    fn test_fips_key_generation() {
        let crypto = get_crypto();

        for key_size in [AesKeySize::Aes128, AesKeySize::Aes256] {
            let key = crypto.generate_aes_key(key_size).unwrap();
            assert_eq!(key.len(), key_size.bytes());
        }
    }
}

mod asymmetric_integration {
    use hv2_core::crypto::asymmetric::{EcCurve, RsaKeySize, SignatureAlgorithm};
    use hv2_core::crypto::fips::{FipsCrypto, FipsMode};

    fn get_crypto() -> FipsCrypto {
        FipsCrypto::new(FipsMode::Disabled).unwrap()
    }

    #[test]
    fn test_rsa_keygen_sign_verify() {
        // RSA comes from IronCrypto's `ic-rsa`; signing and verification
        // round-trip via PKCS#1 v1.5.
        let crypto = get_crypto();
        let key = crypto
            .generate_rsa_keypair(RsaKeySize::Rsa2048)
            .expect("RSA keygen failed");
        assert_eq!(key.public.size, RsaKeySize::Rsa2048);

        let message = b"Test message for RSA signature";
        let signature = crypto
            .rsa_sign(&key, message, SignatureAlgorithm::RsaPkcs1Sha256)
            .expect("RSA signing failed");
        assert_eq!(signature.algorithm, SignatureAlgorithm::RsaPkcs1Sha256);

        assert!(crypto
            .rsa_verify(&key.public, message, &signature)
            .expect("RSA verify failed"));
        // A tampered message must not verify.
        assert!(!crypto
            .rsa_verify(&key.public, b"tampered message", &signature)
            .expect("RSA verify failed"));
    }

    #[test]
    fn test_ecdsa_keygen_sign_verify() {
        let crypto = get_crypto();
        let key = crypto
            .generate_ecdsa_keypair(EcCurve::P256)
            .expect("ECDSA keygen");
        assert_eq!(key.public.curve, EcCurve::P256);
        let message = b"Test message for ECDSA signature";
        let signature = crypto
            .ecdsa_sign(&key, message)
            .expect("ECDSA signing failed");
        assert!(crypto
            .ecdsa_verify(&key.public, message, &signature)
            .expect("Verify failed"));
        // A tampered message must not verify -- the same check the RSA test
        // makes, and the one that separates real verification from a stub that
        // returns `Ok(true)`.
        assert!(!crypto
            .ecdsa_verify(&key.public, b"tampered message", &signature)
            .expect("Verify failed"));
    }
}

mod pqc_integration {
    use hv2_core::crypto::fips::{FipsCrypto, FipsMode};
    use hv2_core::crypto::pqc::{MlDsaParameterSet, MlKemParameterSet, SlhDsaParameterSet};

    fn get_crypto() -> FipsCrypto {
        FipsCrypto::new(FipsMode::Disabled).unwrap()
    }

    #[cfg(feature = "pqc")]
    #[test]
    fn test_ml_kem_roundtrip() {
        let crypto = get_crypto();
        let sk = crypto.ml_kem_keygen(MlKemParameterSet::MlKem768).unwrap();
        assert_eq!(sk.public.parameter_set, MlKemParameterSet::MlKem768);

        let (ciphertext, shared_secret1) = crypto.ml_kem_encaps(&sk.public).expect("Encaps failed");
        let shared_secret2 = crypto
            .ml_kem_decaps(&sk, &ciphertext)
            .expect("Decaps failed");
        assert_eq!(shared_secret1.len(), 32);
        // Encapsulated and decapsulated shared secrets must agree.
        assert_eq!(shared_secret1, shared_secret2);
    }

    #[cfg(feature = "pqc")]
    #[test]
    fn test_ml_dsa_roundtrip() {
        let crypto = get_crypto();
        let secret_key = crypto.ml_dsa_keygen(MlDsaParameterSet::MlDsa65).unwrap();
        let message = b"Post-quantum secure message";
        let signature = crypto
            .ml_dsa_sign(&secret_key, message)
            .expect("ML-DSA signing failed");
        assert!(crypto
            .ml_dsa_verify(&secret_key.public, message, &signature)
            .expect("Verify failed"));
        assert!(!crypto
            .ml_dsa_verify(&secret_key.public, b"tampered", &signature)
            .expect("Verify failed"));
    }

    #[cfg(feature = "pqc")]
    #[test]
    fn test_slh_dsa_roundtrip() {
        let crypto = get_crypto();
        let secret_key = crypto
            .slh_dsa_keygen(SlhDsaParameterSet::Sha2_128f)
            .unwrap();
        let message = b"Hash-based signature test";
        let signature = crypto
            .slh_dsa_sign(&secret_key, message)
            .expect("SLH-DSA signing failed");
        assert!(crypto
            .slh_dsa_verify(&secret_key.public, message, &signature)
            .expect("Verify failed"));
        assert!(!crypto
            .slh_dsa_verify(&secret_key.public, b"tampered", &signature)
            .expect("Verify failed"));
    }
}
