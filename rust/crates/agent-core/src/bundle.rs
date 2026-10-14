//! Port of OfflineBundleExporter.cs — gói dữ liệu tài sản 1-Click cho máy cách ly.
//! Payload JSON canonical → ký ECDSA P-256 (SHA-256, RFC 3279 DER) → mã hóa lai
//! AES-256-GCM + RSA-2048 OAEP(SHA-256) → ZIP:
//! `manifest.json`, `encrypted_payload.bin`, `encrypted_key.bin`, `iv.bin`, `tag.bin`,
//! `signature.sig`, `public_key.pem`. ZIP không password (bảo mật nằm ở lớp mã hóa trong).

use crate::canonical;
use crate::config::AgentConfig;
use crate::keystore::KeyStore;
use crate::schema::{FingerprintPayload, InventorySnapshot};
use std::path::{Path, PathBuf};

pub struct BundleInput<'a> {
    pub output_zip: &'a Path,
    pub server_key_path: Option<&'a Path>,
    pub org_id: Option<&'a str>,
    pub fingerprint: FingerprintPayload,
    /// `spec` — snapshot collector trả về (serialize sang JSON object).
    pub spec: InventorySnapshot,
    pub hostname: String,
}

pub fn export_bundle(input: &BundleInput, config: &mut AgentConfig, keystore: &KeyStore) -> bool {
    match export_inner(input, config, keystore) {
        Ok(()) => {
            log::info!(target: "ExportBundle", "✔ Đã xuất gói ZIP mã hóa thành công: {}", input.output_zip.display());
            true
        }
        Err(e) => {
            log::error!(target: "ExportBundle", "Lỗi khi đóng gói xuất dữ liệu offline: {e}");
            false
        }
    }
}

fn export_inner(
    input: &BundleInput,
    config: &mut AgentConfig,
    keystore: &KeyStore,
) -> anyhow::Result<()> {
    log::info!(target: "ExportBundle", "Bắt đầu tiến trình thu thập tài sản offline...");

    let machine_uuid = config
        .machine_id
        .clone()
        .or_else(|| input.fingerprint.smbios_uuid.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let exported_at = chrono::Utc::now().to_rfc3339();

    let mut payload = serde_json::json!({
        "machine_uuid": machine_uuid,
        "hostname": input.hostname,
        "fingerprint": input.fingerprint,
        "spec": input.spec,
        "exported_at": exported_at,
    });
    if let Some(org) = input.org_id.filter(|s| !s.trim().is_empty()) {
        payload["org_id"] = serde_json::json!(org);
    }

    // Cặp khóa ký: private key trong PEM của máy; chưa có thì sinh + cài cert tự ký
    // (giống .NET: CreateSelfSigned CN=machine-<uuid> rồi InstallCertificate).
    let mut key_pem = keystore.find(config).map(|c| c.key_pem);
    if key_pem.is_none() {
        log::info!(target: "ExportBundle", "Chưa có client cert — sinh cặp khóa ECDSA P-256 mới cho máy...");
        match self_signed_pair(&machine_uuid) {
            Ok((cert_pem, kp_pem)) => {
                let _ = keystore.install(&cert_pem, &kp_pem, config, false).map_err(
                    |e| log::warn!(target: "ExportBundle", "Không thể lưu cert tự ký: {e}"),
                );
                let _ = config.save(&crate::paths::config_file());
                key_pem = Some(kp_pem);
            }
            Err(e) => anyhow::bail!("Không tạo được keypair ECDSA: {e}"),
        }
    }
    use p256::pkcs8::DecodePrivateKey;
    let signing = p256::SecretKey::from_pkcs8_pem(key_pem.as_deref().unwrap_or_default())
        .map_err(|e| anyhow::anyhow!("Private key PEM không đọc được: {e}"))?;
    let signing_key = p256::ecdsa::SigningKey::from(&signing);

    // Ký ECDSA trên canonical JSON.
    log::info!(target: "ExportBundle", "Ký số ECDSA trên payload cấu hình...");
    let canonical_bytes = canonical::to_canonical_string(&payload).into_bytes();
    let sig: p256::ecdsa::Signature =
        p256::ecdsa::signature::Signer::sign(&signing_key, &canonical_bytes);
    let signature_b64 = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        sig.to_der().as_bytes(),
    );
    use p256::pkcs8::EncodePublicKey;
    let agent_pub_pem = signing_key
        .verifying_key()
        .to_public_key_pem(p256::pkcs8::LineEnding::LF)
        .map_err(|e| anyhow::anyhow!("Export public key PEM lỗi: {e}"))?;

    // Server RSA public key.
    let Some(server_pem) = resolve_server_public_key(input.server_key_path, input.output_zip)
    else {
        log::error!(target: "ExportBundle", "Không tìm thấy server_public_key.pem! Copy file này từ Portal vào cùng thư mục USB.");
        anyhow::bail!("thiếu server_public_key.pem");
    };

    // AES-256-GCM + RSA-OAEP-SHA256.
    log::info!(target: "ExportBundle", "Mã hóa gói dữ liệu bằng Server Public Key (AES-256-GCM + RSA-OAEP)...");
    use aes_gcm::aead::{Aead, KeyInit, OsRng};
    use aes_gcm::{Aes256Gcm, Key};
    let session_key = Aes256Gcm::generate_key(OsRng);
    let iv_bytes = rand_nonce();
    let iv = aes_gcm::Nonce::from_slice(&iv_bytes);
    let aes = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&session_key));
    let ciphertext = aes
        .encrypt(iv, canonical_bytes.as_ref())
        .map_err(|e| anyhow::anyhow!("AES-GCM encrypt lỗi: {e}"))?;
    // aes-gcm trả ciphertext||tag trong 1 buffer — .NET tách tag 16B cuối ra file riêng.
    let (ct, tag) = ciphertext.split_at(ciphertext.len() - 16);

    use rsa::pkcs8::DecodePublicKey;
    let rsa_pub = rsa::RsaPublicKey::from_public_key_pem(server_pem.trim())
        .map_err(|e| anyhow::anyhow!("server_public_key.pem không hợp lệ: {e}"))?;
    let encrypted_key = rsa_pub
        .encrypt(
            &mut rsa::rand_core::OsRng,
            rsa::Oaep::new::<sha2::Sha256>(),
            &session_key,
        )
        .map_err(|e| anyhow::anyhow!("RSA-OAEP encrypt lỗi: {e}"))?;

    // Đóng gói ZIP.
    log::info!(target: "ExportBundle", "Nén file ZIP kết quả tại: {}", input.output_zip.display());
    let manifest = serde_json::json!({
        "machine_uuid": machine_uuid,
        "hostname": input.hostname,
        "fingerprint": input.fingerprint,
        "exported_at": exported_at,
        "org_id": input.org_id,
    });
    if let Some(dir) = input
        .output_zip
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
    {
        std::fs::create_dir_all(dir)?;
    }
    let _ = std::fs::remove_file(input.output_zip);
    let f = std::fs::File::create(input.output_zip)?;
    let mut zip = zip::ZipWriter::new(f);
    let opt = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut add = |name: &str, data: &[u8]| -> anyhow::Result<()> {
        zip.start_file(name, opt)?;
        std::io::Write::write_all(&mut zip, data)?;
        Ok(())
    };
    add(
        "manifest.json",
        serde_json::to_string_pretty(&manifest)?.as_bytes(),
    )?;
    add("encrypted_payload.bin", ct)?;
    add("encrypted_key.bin", &encrypted_key)?;
    add("iv.bin", iv.as_slice())?;
    add("tag.bin", tag)?;
    add("signature.sig", signature_b64.as_bytes())?;
    add("public_key.pem", agent_pub_pem.as_bytes())?;
    zip.finish()?;
    Ok(())
}

/// Cert tự ký CN=machine-<uuid> 10 năm (port CertificateRequest.CreateSelfSigned).
fn self_signed_pair(machine_uuid: &str) -> anyhow::Result<(String, String)> {
    let mut params = rcgen::CertificateParams::new(vec![format!("machine-{machine_uuid}")])?;
    params.distinguished_name = rcgen::DistinguishedName::new();
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, format!("machine-{machine_uuid}"));
    params.not_before = rcgen::date_time_ymd(2020, 1, 1);
    params.not_after = rcgen::date_time_ymd(2036, 1, 1);
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let cert = params.self_signed(&key)?;
    Ok((cert.pem(), key.serialize_pem()))
}

fn rand_nonce() -> [u8; 12] {
    use rand::RngCore;
    let mut b = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut b);
    b
}

fn resolve_server_public_key(explicit: Option<&Path>, output_zip: &Path) -> Option<String> {
    if let Some(p) = explicit {
        if let Ok(s) = std::fs::read_to_string(p) {
            return Some(s);
        }
    }
    let abs_zip: PathBuf =
        std::path::absolute(output_zip).unwrap_or_else(|_| output_zip.to_path_buf());
    if let Some(dir) = abs_zip.parent() {
        if let Ok(s) = std::fs::read_to_string(dir.join("server_public_key.pem")) {
            return Some(s);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if let Ok(s) = std::fs::read_to_string(dir.join("server_public_key.pem")) {
                return Some(s);
            }
        }
    }
    std::fs::read_to_string(crate::paths::data_dir().join("server_public_key.pem")).ok()
}
