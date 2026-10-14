//! Port of PemFileKeyStore.cs — client cert/key as PEM files in the data dir (`client-cert.pem`,
//! `client-key.pem` PKCS#8, mode 0600), staged as `.new`, validated, then renamed over the originals.

use crate::config::{write_atomic, AgentConfig};
use chrono::{DateTime, Duration, Utc};
use sha1::{Digest, Sha1};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ClientCert {
    pub cert_pem: String,
    pub key_pem: String,
    /// SHA-1 of the DER, upper-case hex (X509Certificate2.Thumbprint).
    pub thumbprint: String,
    pub not_before: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub subject: String,
}

#[derive(Debug, Clone)]
pub struct KeyStore {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

impl KeyStore {
    pub fn new(cert_path: PathBuf, key_path: PathBuf) -> Self {
        Self {
            cert_path,
            key_path,
        }
    }

    pub fn in_data_dir() -> Self {
        Self::new(crate::paths::cert_file(), crate::paths::key_file())
    }

    /// Load and pair-check the cert; fills `client_cert_thumbprint` if missing.
    pub fn find(&self, config: &mut AgentConfig) -> Option<ClientCert> {
        match load_pair(&self.cert_path, &self.key_path) {
            Ok(c) => {
                if config.client_cert_thumbprint.is_none() {
                    config.client_cert_thumbprint = Some(c.thumbprint.clone());
                }
                if let Some(id) = config.machine_id.as_deref() {
                    self.mirror_legacy(id, &c, false);
                }
                Some(c)
            }
            Err(e) => {
                if self.cert_path.exists() && self.key_path.exists() {
                    log::warn!(target: "PemFileKeyStore", "Load client cert (Linux file) lỗi: {e}");
                }
                None
            }
        }
    }

    pub fn install(
        &self,
        cert_pem: &str,
        key_pem: &str,
        config: &mut AgentConfig,
        is_replace: bool,
    ) -> anyhow::Result<()> {
        let cert_stage = staging(&self.cert_path);
        let key_stage = staging(&self.key_path);
        let result = (|| -> anyhow::Result<ClientCert> {
            let _ = std::fs::remove_file(&cert_stage);
            let _ = std::fs::remove_file(&key_stage);
            write_atomic(
                &cert_stage,
                format!("{}\n", cert_pem.trim()).as_bytes(),
                0o600,
            )?;
            write_atomic(&key_stage, key_pem.as_bytes(), 0o600)?;
            let loaded = load_pair(&cert_stage, &key_stage).map_err(|e| {
                anyhow::anyhow!("Cert mới không hợp lệ — không swap để giữ cert cũ còn usable: {e}")
            })?;
            let now = Utc::now();
            if loaded.not_after <= now {
                anyhow::bail!(
                    "Cert mới đã hết hạn (NotAfter={}).",
                    loaded.not_after.to_rfc3339()
                );
            }
            if loaded.not_before > now + Duration::minutes(5) {
                anyhow::bail!(
                    "Cert mới có NotBefore ở tương lai ({}) — token CSR có vấn đề?",
                    loaded.not_before.to_rfc3339()
                );
            }
            std::fs::rename(&cert_stage, &self.cert_path)?;
            std::fs::rename(&key_stage, &self.key_path)?;
            load_pair(&self.cert_path, &self.key_path)
        })();
        let _ = std::fs::remove_file(&cert_stage);
        let _ = std::fs::remove_file(&key_stage);
        let verified = result?;
        if let Some(id) = config.machine_id.as_deref() {
            self.mirror_legacy(id, &verified, true);
        }
        config.client_cert_thumbprint = Some(verified.thumbprint.clone());
        config.cert_store_location = Some("File".into());
        log::info!(
            target: "PemFileKeyStore",
            "{} client cert (Linux file) thumbprint {}",
            if is_replace { "Đã thay" } else { "Đã cài" },
            verified.thumbprint
        );
        Ok(())
    }
}

impl KeyStore {
    fn legacy_dir(&self) -> PathBuf {
        std::env::var_os("ORGINV_CERT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                self.cert_path
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join("certs")
            })
    }

    /// Copy of the active pair at `certs/machine-{id}.crt.pem|.key.pem` (LinuxKeyStore layout),
    /// which the backend `install.sh` identity check reads on reinstall.
    pub fn mirror_legacy(&self, machine_id: &str, cert: &ClientCert, force: bool) {
        let dir = self.legacy_dir();
        let crt = dir.join(format!("machine-{machine_id}.crt.pem"));
        let key = dir.join(format!("machine-{machine_id}.key.pem"));
        if !force
            && key.exists()
            && std::fs::read_to_string(&crt).is_ok_and(|t| t.trim() == cert.cert_pem.trim())
        {
            return;
        }
        let res = (|| -> anyhow::Result<()> {
            std::fs::create_dir_all(&dir)?;
            write_atomic(&key, format!("{}\n", cert.key_pem.trim()).as_bytes(), 0o600)?;
            write_atomic(
                &crt,
                format!("{}\n", cert.cert_pem.trim()).as_bytes(),
                0o644,
            )?;
            Ok(())
        })();
        if let Err(e) = res {
            log::warn!(target: "PemFileKeyStore", "Không ghi được bản sao cert tại {}: {e}", dir.display());
        }
    }
}

fn staging(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".new");
    PathBuf::from(s)
}

pub fn parse_cert_pem(cert_pem: &str) -> anyhow::Result<(Vec<u8>, ClientCertMeta)> {
    let der = rustls_pemfile::certs(&mut cert_pem.as_bytes())
        .next()
        .ok_or_else(|| anyhow::anyhow!("không có CERTIFICATE trong PEM"))??
        .to_vec();
    let meta = cert_meta(&der)?;
    Ok((der, meta))
}

#[derive(Debug, Clone)]
pub struct ClientCertMeta {
    pub thumbprint: String,
    pub not_before: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub subject: String,
    pub spki: Vec<u8>,
}

fn cert_meta(der: &[u8]) -> anyhow::Result<ClientCertMeta> {
    let (_, cert) =
        x509_parser::parse_x509_certificate(der).map_err(|e| anyhow::anyhow!("parse cert: {e}"))?;
    let ts = |t: x509_parser::time::ASN1Time| {
        DateTime::<Utc>::from_timestamp(t.timestamp(), 0).unwrap_or_default()
    };
    Ok(ClientCertMeta {
        thumbprint: hex::encode_upper(Sha1::digest(der)),
        not_before: ts(cert.validity().not_before),
        not_after: ts(cert.validity().not_after),
        subject: cert.subject().to_string(),
        spki: cert.public_key().raw.to_vec(),
    })
}

fn load_pair(cert_path: &Path, key_path: &Path) -> anyhow::Result<ClientCert> {
    let cert_pem = std::fs::read_to_string(cert_path)?;
    let key_pem = std::fs::read_to_string(key_path)?;
    let (_, meta) = parse_cert_pem(&cert_pem)?;
    let key = rcgen::KeyPair::from_pem(&key_pem)
        .map_err(|e| anyhow::anyhow!("parse private key: {e}"))?;
    if key.public_key_der() != meta.spki {
        anyhow::bail!("private key không khớp certificate");
    }
    Ok(ClientCert {
        cert_pem,
        key_pem,
        thumbprint: meta.thumbprint,
        not_before: meta.not_before,
        not_after: meta.not_after,
        subject: meta.subject,
    })
}
