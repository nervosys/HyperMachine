//! Integration tests for crypto modules
//!
//! Tests FIPS symmetric crypto operations.
//! Most tests require the `ring` feature for real crypto.
//! Without `ring`, operations return `CryptoError::NotImplemented`.

use hv2_core::crypto::fips::{AesKeySize, FipsCrypto, FipsMode};

mod fips_integration {
    use super::*;

    fn get_crypto() -> FipsCrypto {
        // Use Disabled mode to skip self-tests (which require `ring` feature)
        FipsCrypto::new(FipsMode::Disabled).unwrap()
    }

    #[test]
    fn test_fips_crypto_init() {
        let crypto = get_crypto();
        // Disabled mode does not enable FIPS
        assert!(!crypto.is_fips_enabled());
    }

    #[test]
    fn test_fips_crypto_encrypt_requires_ring() {
        let crypto = get_crypto();

        let key = crypto
            .generate_aes_key(AesKeySize::Aes256)
            .expect("Failed to generate key");
        assert_eq!(key.len(), 32);

        let plaintext = b"Integration test message for FIPS crypto";
        let aad = b"additional authenticated data";

        let result = crypto.aes_gcm_encrypt(key.as_bytes(), plaintext, aad);
        // Without the `ring` feature, AES-GCM returns NotImplemented
        #[cfg(not(feature = "ring"))]
        assert!(result.is_err(), "AES-GCM encrypt requires `ring` feature");
        #[cfg(feature = "ring")]
        {
            let ciphertext = result.unwrap();
            let decrypted = crypto
                .aes_gcm_decrypt(key.as_bytes(), &ciphertext, aad)
                .expect("Decryption failed");
            assert_eq!(plaintext.as_slice(), decrypted.as_slice());
        }
    }

    #[test]
    fn test_fips_hash_requires_ring() {
        let crypto = get_crypto();
        let data = b"Test data for hash consistency check";

        let result = crypto.sha256(data);
        #[cfg(not(feature = "ring"))]
        assert!(result.is_err(), "SHA-256 requires `ring` feature");
        #[cfg(feature = "ring")]
        {
            let hash1 = result.unwrap();
            let hash2 = crypto.sha256(data).unwrap();
            assert_eq!(hash1, hash2);
            let hash3 = crypto.sha256(b"Different data").unwrap();
            assert_ne!(hash1, hash3);
        }
    }

    #[test]
    fn test_fips_hmac_requires_ring() {
        let crypto = get_crypto();
        let key = vec![0xab; 32];
        let message = b"Message to authenticate";

        let result = crypto.hmac_sha256(&key, message);
        #[cfg(not(feature = "ring"))]
        assert!(result.is_err(), "HMAC-SHA256 requires `ring` feature");
        #[cfg(feature = "ring")]
        {
            let mac1 = result.unwrap();
            let mac2 = crypto.hmac_sha256(&key, message).unwrap();
            assert_eq!(mac1, mac2);
            let mac3 = crypto.hmac_sha256(&key, b"Different message").unwrap();
            assert_ne!(mac1, mac3);
        }
    }

    #[test]
    fn test_fips_hkdf_requires_ring() {
        let crypto = get_crypto();
        let ikm = vec![0x01; 32];
        let salt = vec![0x02; 32];
        let info = b"key derivation context";

        let result = crypto.hkdf_sha256(&salt, &ikm, info, 32);
        #[cfg(not(feature = "ring"))]
        assert!(result.is_err(), "HKDF requires `ring` feature");
        #[cfg(feature = "ring")]
        {
            let derived_32 = result.unwrap();
            let derived_64 = crypto.hkdf_sha256(&salt, &ikm, info, 64).unwrap();
            assert_eq!(derived_32.len(), 32);
            assert_eq!(derived_64.len(), 64);
            assert_eq!(&derived_32[..], &derived_64[..32]);
        }
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
        let result = crypto.run_self_tests();
        // Without the `ring` feature, self-tests fail (crypto ops return NotImplemented)
        #[cfg(not(feature = "ring"))]
        assert!(result.is_err(), "Self-tests require `ring` feature");
        #[cfg(feature = "ring")]
        assert!(result.is_ok());
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
    use hv2_core::crypto::asymmetric::{EcCurve, RsaKeySize};
    #[cfg(feature = "ring")]
    use hv2_core::crypto::asymmetric::SignatureAlgorithm;
    use hv2_core::crypto::fips::{FipsCrypto, FipsMode};

    fn get_crypto() -> FipsCrypto {
        FipsCrypto::new(FipsMode::Disabled).unwrap()
    }

    #[test]
    fn test_rsa_keygen_requires_ring() {
        let crypto = get_crypto();
        let result = crypto.generate_rsa_keypair(RsaKeySize::Rsa2048);
        #[cfg(not(feature = "ring"))]
        assert!(result.is_err(), "RSA keygen requires `ring` feature");
        #[cfg(feature = "ring")]
        {
            let key = result.unwrap();
            assert_eq!(key.public.size, RsaKeySize::Rsa2048);
            let message = b"Test message for RSA signature";
            let signature = crypto
                .rsa_sign(&key, message, SignatureAlgorithm::RsaPkcs1Sha256)
                .expect("RSA signing failed");
            assert_eq!(signature.algorithm, SignatureAlgorithm::RsaPkcs1Sha256);
            let valid = crypto
                .rsa_verify(&key.public, message, &signature)
                .expect("Verify failed");
            assert!(valid);
        }
    }

    #[test]
    fn test_ecdsa_keygen_requires_ring() {
        let crypto = get_crypto();
        let result = crypto.generate_ecdsa_keypair(EcCurve::P256);
        #[cfg(not(feature = "ring"))]
        assert!(result.is_err(), "ECDSA keygen requires `ring` feature");
        #[cfg(feature = "ring")]
        {
            let key = result.unwrap();
            assert_eq!(key.public.curve, EcCurve::P256);
            let message = b"Test message for ECDSA signature";
            let signature = crypto.ecdsa_sign(&key, message).expect("ECDSA signing failed");
            let valid = crypto
                .ecdsa_verify(&key.public, message, &signature)
                .expect("Verify failed");
            assert!(valid);
        }
    }
}

mod pqc_integration {
    use hv2_core::crypto::fips::{FipsCrypto, FipsMode};
    use hv2_core::crypto::pqc::{MlDsaParameterSet, MlKemParameterSet, SlhDsaParameterSet};

    fn get_crypto() -> FipsCrypto {
        FipsCrypto::new(FipsMode::Disabled).unwrap()
    }

    #[test]
    fn test_ml_kem_requires_ring() {
        let crypto = get_crypto();
        // ML-KEM keygen only uses RNG, so it works without `ring`
        let sk = crypto.ml_kem_keygen(MlKemParameterSet::MlKem768).unwrap();
        assert_eq!(sk.public.parameter_set, MlKemParameterSet::MlKem768);

        // But encaps uses SHA-256 which requires `ring`
        let result = crypto.ml_kem_encaps(&sk.public);
        #[cfg(not(feature = "ring"))]
        assert!(result.is_err(), "ML-KEM encaps requires `ring` feature");
        #[cfg(feature = "ring")]
        {
            let (ciphertext, shared_secret1) = result.unwrap();
            let shared_secret2 = crypto
                .ml_kem_decaps(&sk, &ciphertext)
                .expect("Decaps failed");
            assert_eq!(shared_secret1.len(), 32);
            assert_eq!(shared_secret2.len(), 32);
        }
    }

    #[test]
    fn test_ml_dsa_requires_ring() {
        let crypto = get_crypto();
        let result = crypto.ml_dsa_keygen(MlDsaParameterSet::MlDsa65);
        #[cfg(not(feature = "ring"))]
        assert!(result.is_err(), "ML-DSA requires `ring` feature");
        #[cfg(feature = "ring")]
        {
            let secret_key = result.unwrap();
            let message = b"Post-quantum secure message";
            let signature = crypto
                .ml_dsa_sign(&secret_key, message)
                .expect("ML-DSA signing failed");
            let valid = crypto
                .ml_dsa_verify(&secret_key.public, message, &signature)
                .expect("Verify failed");
            assert!(valid);
        }
    }

    #[test]
    fn test_slh_dsa_requires_ring() {
        let crypto = get_crypto();
        let result = crypto.slh_dsa_keygen(SlhDsaParameterSet::Sha2_128f);
        #[cfg(not(feature = "ring"))]
        assert!(result.is_err(), "SLH-DSA requires `ring` feature");
        #[cfg(feature = "ring")]
        {
            let secret_key = result.unwrap();
            let message = b"Hash-based signature test";
            let signature = crypto
                .slh_dsa_sign(&secret_key, message)
                .expect("SLH-DSA signing failed");
            let valid = crypto
                .slh_dsa_verify(&secret_key.public, message, &signature)
                .expect("Verify failed");
            assert!(valid);
        }
    }
}
