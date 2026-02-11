use std::collections::HashMap;
use std::path::PathBuf;

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};
use thiserror::Error;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroize;

#[derive(Debug, Error)]
pub enum EncryptionError {
    #[error("Encryption failed: {0}")]
    EncryptFailed(String),
    #[error("Decryption failed: {0}")]
    DecryptFailed(String),
    #[error("Group key not found: {0}")]
    GroupKeyNotFound(String),
    #[error("Key file error: {0}")]
    KeyFileError(String),
    #[error("Key exchange error: {0}")]
    KeyExchangeError(String),
}

type Result<T> = std::result::Result<T, EncryptionError>;

/// Manages X25519 keypair and per-group AES-256-GCM keys.
pub struct KeyManager {
    keys_dir: PathBuf,
    private_key: StaticSecret,
    public_key: PublicKey,
    group_keys: HashMap<String, [u8; 32]>,
}

impl KeyManager {
    /// Load or generate a keypair, then load any existing group keys.
    pub fn new(keys_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&keys_dir)
            .map_err(|e| EncryptionError::KeyFileError(format!("Cannot create keys dir: {e}")))?;

        let (private_key, public_key) = Self::init_keypair(&keys_dir)?;
        let mut mgr = Self {
            keys_dir,
            private_key,
            public_key,
            group_keys: HashMap::new(),
        };
        mgr.load_group_keys();
        Ok(mgr)
    }

    /// Generate or load the X25519 keypair from disk.
    fn init_keypair(keys_dir: &PathBuf) -> Result<(StaticSecret, PublicKey)> {
        let priv_path = keys_dir.join("private.key");
        let pub_path = keys_dir.join("public.key");

        if priv_path.exists() && pub_path.exists() {
            let priv_bytes = std::fs::read(&priv_path)
                .map_err(|e| EncryptionError::KeyFileError(format!("Read private key: {e}")))?;
            let pub_bytes = std::fs::read(&pub_path)
                .map_err(|e| EncryptionError::KeyFileError(format!("Read public key: {e}")))?;

            if priv_bytes.len() != 32 || pub_bytes.len() != 32 {
                return Err(EncryptionError::KeyFileError(
                    "Invalid key file length".into(),
                ));
            }

            let mut priv_arr = [0u8; 32];
            priv_arr.copy_from_slice(&priv_bytes);
            let secret = StaticSecret::from(priv_arr);
            priv_arr.zeroize();

            let mut pub_arr = [0u8; 32];
            pub_arr.copy_from_slice(&pub_bytes);
            let public = PublicKey::from(pub_arr);

            Ok((secret, public))
        } else {
            let secret = StaticSecret::random_from_rng(OsRng);
            let public = PublicKey::from(&secret);

            // Write private key with restricted permissions
            std::fs::write(&priv_path, secret.to_bytes())
                .map_err(|e| EncryptionError::KeyFileError(format!("Write private key: {e}")))?;
            set_file_permissions(&priv_path, 0o600)?;

            std::fs::write(&pub_path, public.to_bytes())
                .map_err(|e| EncryptionError::KeyFileError(format!("Write public key: {e}")))?;
            set_file_permissions(&pub_path, 0o600)?;

            Ok((secret, public))
        }
    }

    /// Return the public key as a base64-encoded string.
    pub fn public_key_b64(&self) -> String {
        B64.encode(self.public_key.as_bytes())
    }

    /// Create a new random AES-256 group key, save to disk, and store in memory.
    pub fn create_group_key(&mut self, group_name: &str) -> Result<[u8; 32]> {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);

        let groups_dir = self.keys_dir.join("groups");
        std::fs::create_dir_all(&groups_dir)
            .map_err(|e| EncryptionError::KeyFileError(format!("Create groups dir: {e}")))?;

        let key_path = groups_dir.join(format!("{group_name}.key"));
        std::fs::write(&key_path, key)
            .map_err(|e| EncryptionError::KeyFileError(format!("Write group key: {e}")))?;
        set_file_permissions(&key_path, 0o600)?;

        self.group_keys.insert(group_name.to_string(), key);
        Ok(key)
    }

    /// Load all group keys from keys_dir/groups/.
    pub fn load_group_keys(&mut self) {
        let groups_dir = self.keys_dir.join("groups");
        if !groups_dir.exists() {
            return;
        }
        let entries = match std::fs::read_dir(&groups_dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("key") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if let Ok(bytes) = std::fs::read(&path) {
                        if bytes.len() == 32 {
                            let mut key = [0u8; 32];
                            key.copy_from_slice(&bytes);
                            self.group_keys.insert(stem.to_string(), key);
                        }
                    }
                }
            }
        }
    }

    /// Get a reference to a group's AES key.
    pub fn get_group_key(&self, group_name: &str) -> Option<&[u8; 32]> {
        self.group_keys.get(group_name)
    }

    /// Check whether a group key is available.
    pub fn has_group_key(&self, group_name: &str) -> bool {
        self.group_keys.contains_key(group_name)
    }

    /// Wrap (encrypt) a group key for a specific recipient using X25519 + AES-GCM.
    /// Returns the wrapped key as a base64 string.
    pub fn wrap_group_key(&self, group_name: &str, recipient_pubkey_b64: &str) -> Result<String> {
        let group_key = self
            .group_keys
            .get(group_name)
            .ok_or_else(|| EncryptionError::GroupKeyNotFound(group_name.to_string()))?;

        let recipient_pub = decode_public_key(recipient_pubkey_b64)?;
        let shared_secret = self.private_key.diffie_hellman(&recipient_pub);
        let aes_key = derive_aes_key(shared_secret.as_bytes());

        let (ciphertext, nonce) = encrypt_field(&aes_key, group_key)?;

        // Pack nonce + ciphertext
        let mut packed = Vec::with_capacity(nonce.len() + ciphertext.len());
        packed.extend_from_slice(&nonce);
        packed.extend_from_slice(&ciphertext);

        Ok(B64.encode(&packed))
    }

    /// Unwrap (decrypt) a group key from a sender, store it in memory and on disk.
    pub fn unwrap_group_key(
        &mut self,
        group_name: &str,
        wrapped_b64: &str,
        sender_pubkey_b64: &str,
    ) -> Result<()> {
        let sender_pub = decode_public_key(sender_pubkey_b64)?;
        let shared_secret = self.private_key.diffie_hellman(&sender_pub);
        let aes_key = derive_aes_key(shared_secret.as_bytes());

        let packed = B64
            .decode(wrapped_b64)
            .map_err(|e| EncryptionError::KeyExchangeError(format!("Base64 decode: {e}")))?;

        if packed.len() < 12 {
            return Err(EncryptionError::KeyExchangeError(
                "Wrapped key too short".into(),
            ));
        }

        let (nonce_bytes, ciphertext) = packed.split_at(12);
        let plaintext = decrypt_field(&aes_key, ciphertext, nonce_bytes)?;

        if plaintext.len() != 32 {
            return Err(EncryptionError::KeyExchangeError(
                "Unwrapped key has invalid length".into(),
            ));
        }

        let mut key = [0u8; 32];
        key.copy_from_slice(&plaintext);

        // Save to disk
        let groups_dir = self.keys_dir.join("groups");
        std::fs::create_dir_all(&groups_dir)
            .map_err(|e| EncryptionError::KeyFileError(format!("Create groups dir: {e}")))?;
        let key_path = groups_dir.join(format!("{group_name}.key"));
        std::fs::write(&key_path, key)
            .map_err(|e| EncryptionError::KeyFileError(format!("Write group key: {e}")))?;
        set_file_permissions(&key_path, 0o600)?;

        self.group_keys.insert(group_name.to_string(), key);
        Ok(())
    }
}

// ===== Free functions =====

/// Encrypt plaintext with AES-256-GCM. Returns (ciphertext, nonce).
pub fn encrypt_field(key: &[u8; 32], plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| EncryptionError::EncryptFailed(e.to_string()))?;

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| EncryptionError::EncryptFailed(e.to_string()))?;

    Ok((ciphertext, nonce_bytes.to_vec()))
}

/// Decrypt ciphertext with AES-256-GCM.
pub fn decrypt_field(key: &[u8; 32], ciphertext: &[u8], nonce: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| EncryptionError::DecryptFailed(e.to_string()))?;

    let nonce = Nonce::from_slice(nonce);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| EncryptionError::DecryptFailed(e.to_string()))?;

    Ok(plaintext)
}

/// Encrypt a string, returning (base64_ciphertext, base64_nonce).
pub fn encrypt_string(key: &[u8; 32], text: &str) -> Result<(String, String)> {
    let (ct, nonce) = encrypt_field(key, text.as_bytes())?;
    Ok((B64.encode(&ct), B64.encode(&nonce)))
}

/// Decrypt a base64-encoded ciphertext and nonce back to a string.
pub fn decrypt_string(key: &[u8; 32], b64_ct: &str, b64_nonce: &str) -> Result<String> {
    let ct = B64
        .decode(b64_ct)
        .map_err(|e| EncryptionError::DecryptFailed(format!("Base64 ciphertext: {e}")))?;
    let nonce = B64
        .decode(b64_nonce)
        .map_err(|e| EncryptionError::DecryptFailed(format!("Base64 nonce: {e}")))?;

    let plaintext = decrypt_field(key, &ct, &nonce)?;
    String::from_utf8(plaintext)
        .map_err(|e| EncryptionError::DecryptFailed(format!("UTF-8 decode: {e}")))
}

// ===== Model encryption/decryption =====

use crate::models::{Alias, EncryptedAlias, EncryptedHistoryEntry, HistoryEntry};

/// Encrypt a HistoryEntry for wire transmission.
/// Encrypts: command, cwd, exit_code, duration_ms, hostname.
/// Each field gets its own random nonce stored as a JSON array in `nonces`.
pub fn encrypt_history_entry(
    key: &[u8; 32],
    entry: &HistoryEntry,
) -> Result<EncryptedHistoryEntry> {
    let (ct_command, n_command) = encrypt_string(key, &entry.command)?;
    let (ct_cwd, n_cwd) = encrypt_string(key, &entry.cwd)?;
    let (ct_exit, n_exit) = encrypt_string(key, &entry.exit_code.to_string())?;
    let (ct_dur, n_dur) = encrypt_string(key, &entry.duration_ms.to_string())?;
    let (ct_host, n_host) = encrypt_string(key, &entry.hostname)?;

    let nonces = serde_json::json!([n_command, n_cwd, n_exit, n_dur, n_host]);

    Ok(EncryptedHistoryEntry {
        id: entry.id.clone(),
        command: ct_command,
        cwd: ct_cwd,
        exit_code: ct_exit,
        duration_ms: ct_dur,
        session_id: entry.session_id.clone(),
        machine_id: entry.machine_id.clone(),
        hostname: ct_host,
        timestamp: entry.timestamp,
        shell: entry.shell.clone(),
        group_name: entry.group_name.clone(),
        nonces: nonces.to_string(),
    })
}

/// Decrypt an EncryptedHistoryEntry back to a HistoryEntry.
pub fn decrypt_history_entry(key: &[u8; 32], enc: &EncryptedHistoryEntry) -> Result<HistoryEntry> {
    let nonces: Vec<String> = serde_json::from_str(&enc.nonces)
        .map_err(|e| EncryptionError::DecryptFailed(format!("Parse nonces: {e}")))?;

    if nonces.len() < 5 {
        return Err(EncryptionError::DecryptFailed(
            "Expected 5 nonces for history entry".into(),
        ));
    }

    let command = decrypt_string(key, &enc.command, &nonces[0])?;
    let cwd = decrypt_string(key, &enc.cwd, &nonces[1])?;
    let exit_code: i32 = decrypt_string(key, &enc.exit_code, &nonces[2])?
        .parse()
        .map_err(|e| EncryptionError::DecryptFailed(format!("Parse exit_code: {e}")))?;
    let duration_ms: i64 = decrypt_string(key, &enc.duration_ms, &nonces[3])?
        .parse()
        .map_err(|e| EncryptionError::DecryptFailed(format!("Parse duration_ms: {e}")))?;
    let hostname = decrypt_string(key, &enc.hostname, &nonces[4])?;

    Ok(HistoryEntry {
        id: enc.id.clone(),
        command,
        cwd,
        exit_code,
        duration_ms,
        session_id: enc.session_id.clone(),
        machine_id: enc.machine_id.clone(),
        hostname,
        timestamp: enc.timestamp,
        shell: enc.shell.clone(),
        group_name: enc.group_name.clone(),
    })
}

/// Encrypt an Alias for wire transmission. Only the command field is encrypted.
pub fn encrypt_alias(key: &[u8; 32], alias: &Alias) -> Result<EncryptedAlias> {
    let (ct_command, nonce) = encrypt_string(key, &alias.command)?;

    Ok(EncryptedAlias {
        id: alias.id,
        name: alias.name.clone(),
        command: ct_command,
        group_name: alias.group_name.clone(),
        created_by_machine: alias.created_by_machine.clone(),
        created_at: alias.created_at,
        updated_at: alias.updated_at,
        version: alias.version,
        nonce,
    })
}

/// Decrypt an EncryptedAlias back to an Alias.
pub fn decrypt_alias(key: &[u8; 32], enc: &EncryptedAlias) -> Result<Alias> {
    let command = decrypt_string(key, &enc.command, &enc.nonce)?;

    Ok(Alias {
        id: enc.id,
        name: enc.name.clone(),
        command,
        group_name: enc.group_name.clone(),
        created_by_machine: enc.created_by_machine.clone(),
        created_at: enc.created_at,
        updated_at: enc.updated_at,
        version: enc.version,
    })
}

// ===== Internal helpers =====

/// Decode a base64-encoded X25519 public key.
fn decode_public_key(b64: &str) -> Result<PublicKey> {
    let bytes = B64
        .decode(b64)
        .map_err(|e| EncryptionError::KeyExchangeError(format!("Base64 decode pubkey: {e}")))?;
    if bytes.len() != 32 {
        return Err(EncryptionError::KeyExchangeError(
            "Public key must be 32 bytes".into(),
        ));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(PublicKey::from(arr))
}

/// Derive a 256-bit AES key from a shared secret using SHA-256.
fn derive_aes_key(shared_secret: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(shared_secret);
    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

/// Set file permissions (Unix only).
fn set_file_permissions(path: &std::path::Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(mode);
        std::fs::set_permissions(path, perms)
            .map_err(|e| EncryptionError::KeyFileError(format!("Set permissions: {e}")))?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_field_roundtrip() {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        let plaintext = b"hello world, this is a secret message";

        let (ct, nonce) = encrypt_field(&key, plaintext).unwrap();
        let decrypted = decrypt_field(&key, &ct, &nonce).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn encrypt_decrypt_string_roundtrip() {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        let text = "git status --short";

        let (b64_ct, b64_nonce) = encrypt_string(&key, text).unwrap();
        let decrypted = decrypt_string(&key, &b64_ct, &b64_nonce).unwrap();
        assert_eq!(decrypted, text);
    }

    #[test]
    fn decrypt_with_wrong_key_fails() {
        let mut key1 = [0u8; 32];
        let mut key2 = [0u8; 32];
        OsRng.fill_bytes(&mut key1);
        OsRng.fill_bytes(&mut key2);

        let (ct, nonce) = encrypt_field(&key1, b"secret").unwrap();
        let result = decrypt_field(&key2, &ct, &nonce);
        assert!(result.is_err());
    }

    #[test]
    fn keypair_generation_and_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let keys_dir = dir.path().join("keys");

        let mgr1 = KeyManager::new(keys_dir.clone()).unwrap();
        let pub1 = mgr1.public_key_b64();

        // Reloading should produce the same public key
        let mgr2 = KeyManager::new(keys_dir).unwrap();
        let pub2 = mgr2.public_key_b64();

        assert_eq!(pub1, pub2);
        assert!(!pub1.is_empty());
    }

    #[test]
    fn group_key_create_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let keys_dir = dir.path().join("keys");

        let mut mgr = KeyManager::new(keys_dir.clone()).unwrap();
        mgr.create_group_key("work").unwrap();
        assert!(mgr.has_group_key("work"));
        assert!(!mgr.has_group_key("personal"));

        // Reload and verify persistence
        let mgr2 = KeyManager::new(keys_dir).unwrap();
        assert!(mgr2.has_group_key("work"));
    }

    #[test]
    fn wrap_unwrap_group_key() {
        let dir = tempfile::tempdir().unwrap();

        // Machine A creates the group key
        let keys_a = dir.path().join("keys_a");
        let mut mgr_a = KeyManager::new(keys_a).unwrap();
        mgr_a.create_group_key("team").unwrap();

        // Machine B wants the key
        let keys_b = dir.path().join("keys_b");
        let mut mgr_b = KeyManager::new(keys_b).unwrap();
        assert!(!mgr_b.has_group_key("team"));

        // A wraps the key for B
        let wrapped = mgr_a
            .wrap_group_key("team", &mgr_b.public_key_b64())
            .unwrap();

        // B unwraps it
        mgr_b
            .unwrap_group_key("team", &wrapped, &mgr_a.public_key_b64())
            .unwrap();

        assert!(mgr_b.has_group_key("team"));

        // Verify both have the same key
        assert_eq!(
            mgr_a.get_group_key("team").unwrap(),
            mgr_b.get_group_key("team").unwrap()
        );
    }

    #[test]
    fn encrypt_decrypt_history_entry_roundtrip() {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);

        let entry = HistoryEntry {
            id: "abc-123".into(),
            command: "docker compose up -d".into(),
            cwd: "/home/user/project".into(),
            exit_code: 0,
            duration_ms: 1234,
            session_id: "sess-1".into(),
            machine_id: "machine-1".into(),
            hostname: "my-laptop".into(),
            timestamp: 1700000000,
            shell: "zsh".into(),
            group_name: "default".into(),
        };

        let encrypted = encrypt_history_entry(&key, &entry).unwrap();

        // Verify sensitive fields are encrypted (not plaintext)
        assert_ne!(encrypted.command, entry.command);
        assert_ne!(encrypted.cwd, entry.cwd);
        assert_ne!(encrypted.hostname, entry.hostname);

        // Verify routing fields stay plaintext
        assert_eq!(encrypted.id, entry.id);
        assert_eq!(encrypted.session_id, entry.session_id);
        assert_eq!(encrypted.machine_id, entry.machine_id);
        assert_eq!(encrypted.timestamp, entry.timestamp);
        assert_eq!(encrypted.shell, entry.shell);
        assert_eq!(encrypted.group_name, entry.group_name);

        let decrypted = decrypt_history_entry(&key, &encrypted).unwrap();
        assert_eq!(decrypted.command, entry.command);
        assert_eq!(decrypted.cwd, entry.cwd);
        assert_eq!(decrypted.exit_code, entry.exit_code);
        assert_eq!(decrypted.duration_ms, entry.duration_ms);
        assert_eq!(decrypted.hostname, entry.hostname);
    }

    #[test]
    fn encrypt_decrypt_alias_roundtrip() {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);

        let alias = Alias {
            id: 42,
            name: "gs".into(),
            command: "git status --short".into(),
            group_name: "work".into(),
            created_by_machine: "machine-1".into(),
            created_at: 1000,
            updated_at: 2000,
            version: 3,
        };

        let encrypted = encrypt_alias(&key, &alias).unwrap();

        // Command is encrypted
        assert_ne!(encrypted.command, alias.command);
        // Name stays plaintext
        assert_eq!(encrypted.name, alias.name);

        let decrypted = decrypt_alias(&key, &encrypted).unwrap();
        assert_eq!(decrypted.command, alias.command);
        assert_eq!(decrypted.name, alias.name);
        assert_eq!(decrypted.id, alias.id);
    }

    #[test]
    fn key_file_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let keys_dir = dir.path().join("keys");

        let _mgr = KeyManager::new(keys_dir.clone()).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let priv_meta = std::fs::metadata(keys_dir.join("private.key")).unwrap();
            assert_eq!(priv_meta.permissions().mode() & 0o777, 0o600);

            let pub_meta = std::fs::metadata(keys_dir.join("public.key")).unwrap();
            assert_eq!(pub_meta.permissions().mode() & 0o777, 0o600);
        }
    }
}
