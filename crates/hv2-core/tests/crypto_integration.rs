//! Integration tests for crypto modules
//!
//! Tests FIPS symmetric crypto operations

use hv2_core::crypto::fips::{AesKeySize, FipsCrypto, FipsMode};

mod fips_integration {
    use super::*;

    #[test]
    fn test_fips_crypto_full_workflow() {
        let crypto = FipsCrypto::new(FipsMode::Enabled).expect("Failed to create FIPS crypto");

        // Generate key
        let key = crypto
            .generate_aes_key(AesKeySize::Aes256)
            .expect("Failed to generate key");
        assert_eq!(key.len(), 32);

        // Encrypt/decrypt cycle
        let plaintext = b"Integration test message for FIPS crypto";
        let aad = b"additional authenticated data";

        let ciphertext = crypto
            .aes_gcm_encrypt(key.as_bytes(), plaintext, aad)
            .expect("Encryption failed");

        let decrypted = crypto
            .aes_gcm_decrypt(key.as_bytes(), &ciphertext, aad)
            .expect("Decryption failed");

        assert_eq!(plaintext.as_slice(), decrypted.as_slice());
    }

    #[test]
    fn test_fips_hash_consistency() {
        let crypto = FipsCrypto::new(FipsMode::Enabled).unwrap();
        let data = b"Test data for hash consistency check";

        // Same input should produce same hash
        let hash1 = crypto.sha256(data).unwrap();
        let hash2 = crypto.sha256(data).unwrap();
        assert_eq!(hash1, hash2);

        // Different input should produce different hash
        let hash3 = crypto.sha256(b"Different data").unwrap();
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_fips_hmac_verification() {
        let crypto = FipsCrypto::new(FipsMode::Enabled).unwrap();
        let key = vec![0xab; 32];
        let message = b"Message to authenticate";

        let mac1 = crypto.hmac_sha256(&key, message).unwrap();
        let mac2 = crypto.hmac_sha256(&key, message).unwrap();

        // Same key and message should produce same MAC
        assert_eq!(mac1, mac2);

        // Different message should produce different MAC
        let mac3 = crypto.hmac_sha256(&key, b"Different message").unwrap();
        assert_ne!(mac1, mac3);
    }

    #[test]
    fn test_fips_hkdf_key_derivation() {
        let crypto = FipsCrypto::new(FipsMode::Enabled).unwrap();

        let ikm = vec![0x01; 32];
        let salt = vec![0x02; 32];
        let info = b"key derivation context";

        let derived_32 = crypto.hkdf_sha256(&salt, &ikm, info, 32).unwrap();
        let derived_64 = crypto.hkdf_sha256(&salt, &ikm, info, 64).unwrap();

        assert_eq!(derived_32.len(), 32);
        assert_eq!(derived_64.len(), 64);

        // First 32 bytes of 64-byte output should match 32-byte output
        assert_eq!(&derived_32[..], &derived_64[..32]);
    }

    #[test]
    fn test_fips_random_uniqueness() {
        let crypto = FipsCrypto::new(FipsMode::Enabled).unwrap();

        let mut buf1 = vec![0u8; 32];
        let mut buf2 = vec![0u8; 32];

        crypto.random_bytes(&mut buf1).unwrap();
        crypto.random_bytes(&mut buf2).unwrap();

        // Random outputs should be unique (with overwhelming probability)
        assert_ne!(buf1, buf2);
    }

    #[test]
    fn test_fips_self_tests() {
        let mut crypto = FipsCrypto::new(FipsMode::Enabled).unwrap();
        assert!(crypto.run_self_tests().is_ok());
    }

    #[test]
    fn test_fips_multiple_key_sizes() {
        let crypto = FipsCrypto::new(FipsMode::Enabled).unwrap();
        let plaintext = b"Test plaintext for multiple key sizes";
        let aad = b"aad";

        for key_size in [AesKeySize::Aes128, AesKeySize::Aes256] {
            let key = crypto.generate_aes_key(key_size).unwrap();
            let ciphertext = crypto.aes_gcm_encrypt(key.as_bytes(), plaintext, aad).unwrap();
            let decrypted = crypto.aes_gcm_decrypt(key.as_bytes(), &ciphertext, aad).unwrap();
            assert_eq!(plaintext.as_slice(), decrypted.as_slice());
        }
    }

    #[test]
    fn test_repeated_operations() {
        let fips = FipsCrypto::new(FipsMode::Enabled).unwrap();
        let key = fips.generate_aes_key(AesKeySize::Aes256).unwrap();

        for i in 0..100 {
            let plaintext = format!("Message iteration {}", i);
            let ct = fips.aes_gcm_encrypt(key.as_bytes(), plaintext.as_bytes(), b"").unwrap();
            let pt = fips.aes_gcm_decrypt(key.as_bytes(), &ct, b"").unwrap();
            assert_eq!(plaintext.as_bytes(), pt.as_slice());
        }
    }

    #[test]
    fn test_large_data_handling() {
        let fips = FipsCrypto::new(FipsMode::Enabled).unwrap();
        let key = fips.generate_aes_key(AesKeySize::Aes256).unwrap();

        // 1MB of data
        let large_data = vec![0xab_u8; 1024 * 1024];

        let ct = fips.aes_gcm_encrypt(key.as_bytes(), &large_data, b"large").unwrap();
        let pt = fips.aes_gcm_decrypt(key.as_bytes(), &ct, b"large").unwrap();

        assert_eq!(large_data, pt);
    }

    #[test]
    fn test_concurrent_crypto_operations() {
        use std::thread;

        let handles: Vec<_> = (0..4)
            .map(|i| {
                thread::spawn(move || {
                    let fips = FipsCrypto::new(FipsMode::Enabled).unwrap();
                    let key = fips.generate_aes_key(AesKeySize::Aes256).unwrap();

                    for j in 0..25 {
                        let data = format!("Thread {} iteration {}", i, j);
                        let ct = fips.aes_gcm_encrypt(key.as_bytes(), data.as_bytes(), b"").unwrap();
                        let pt = fips.aes_gcm_decrypt(key.as_bytes(), &ct, b"").unwrap();
                        assert_eq!(data.as_bytes(), pt.as_slice());
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().expect("Thread panicked");
        }
    }
}
