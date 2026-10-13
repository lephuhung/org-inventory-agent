//! Port of CsrGenerator.cs — PKCS#10, ECDSA P-256 / SHA-256, CA=false, KU digitalSignature|keyEncipherment.

use rcgen::{
    CertificateParams, CustomExtension, DistinguishedName, DnType, KeyPair, PKCS_ECDSA_P256_SHA256,
};

pub fn new_key_pair() -> anyhow::Result<KeyPair> {
    Ok(KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?)
}

pub fn create_csr_pem(key: &KeyPair, common_name: &str) -> anyhow::Result<String> {
    let mut params = CertificateParams::new(Vec::<String>::new())?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, common_name);
    params.distinguished_name = dn;
    // rcgen rejects is_ca/key_usages in CSRs, so encode the extensionRequest entries directly.
    let mut basic_constraints =
        CustomExtension::from_oid_content(&[2, 5, 29, 19], vec![0x30, 0x00]);
    basic_constraints.set_criticality(true);
    // KeyUsage BIT STRING: digitalSignature (bit 0) | keyEncipherment (bit 2).
    let mut key_usage =
        CustomExtension::from_oid_content(&[2, 5, 29, 15], vec![0x03, 0x02, 0x05, 0xa0]);
    key_usage.set_criticality(true);
    params.custom_extensions = vec![basic_constraints, key_usage];
    Ok(params.serialize_request(key)?.pem()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csr_has_pem_armor() {
        let k = new_key_pair().unwrap();
        let pem = create_csr_pem(&k, "machine-123").unwrap();
        assert!(pem.starts_with("-----BEGIN CERTIFICATE REQUEST-----"));
        assert!(k.serialize_pem().contains("BEGIN PRIVATE KEY"));
    }
}
