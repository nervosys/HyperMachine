# Cryptography

HyperMachine implements FIPS 140-3 compliant cryptographic modules.

## Supported Algorithms

| Type             | Algorithms                                             |
| ---------------- | ------------------------------------------------------ |
| **Symmetric**    | AES-128/256-GCM, AES-128/256-CBC                       |
| **Hash**         | SHA-256, SHA-384, SHA-512                              |
| **MAC**          | HMAC-SHA-256, HMAC-SHA-512                             |
| **KDF**          | HKDF-SHA-256, PBKDF2-SHA-256                           |
| **Asymmetric**   | RSA-2048/3072/4096, ECDSA P-256/P-384/P-521            |
| **Post-Quantum** | ML-KEM (Kyber), ML-DSA (Dilithium), SLH-DSA (SPHINCS+) |

## Usage

### Symmetric Encryption

```rust
use hv2_core::crypto::{Aes256Gcm, Cipher};

let key = Aes256Gcm::generate_key();
let cipher = Aes256Gcm::new(&key);

let plaintext = b"sensitive data";
let ciphertext = cipher.encrypt(plaintext)?;
let decrypted = cipher.decrypt(&ciphertext)?;
```

### Hashing

```rust
use hv2_core::crypto::Sha256;

let hash = Sha256::digest(b"data to hash");
println!("SHA-256: {}", hex::encode(hash));
```

### Post-Quantum

```rust
use hv2_core::crypto::MlKem768;

// Key encapsulation
let (public_key, secret_key) = MlKem768::keypair();
let (ciphertext, shared_secret) = public_key.encapsulate();
let decapsulated = secret_key.decapsulate(&ciphertext);
```

## FIPS Mode

Enable FIPS 140-3 mode:

```toml
[crypto]
fips_mode = true
```

In FIPS mode, only approved algorithms are available.

## Performance

| Operation           | Throughput |
| ------------------- | ---------- |
| AES-256-GCM encrypt | ~600 MiB/s |
| AES-256-GCM decrypt | ~700 MiB/s |
| SHA-256             | ~3.7 GiB/s |
| SHA-512             | ~3.5 GiB/s |
