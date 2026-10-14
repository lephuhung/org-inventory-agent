//! Windows-only build script — nhúng VERSIONINFO resource vào exe.
//!
//! Bắt buộc cho nâng cấp MSI: cột `File.Version` trong MSI được WiX harvest từ
//! PE version resource (VS_FIXEDFILEINFO). Exe KHÔNG có VERSIONINFO → File.Version
//! trống → khi major-upgrade từ bản có version, Windows Installer disallow
//! component ("same component with higher versioned keyfile exists") NHƯNG vẫn
//! gỡ bản cũ → mất hẳn exe + service (incident upgrade 1.3.1 → 1.4.0).
//!
//! Version lấy từ CARGO_PKG_VERSION (CI pin qua workspace Cargo.toml mỗi release),
//! suffix prerelease bị cắt giống MSI_V trong rust-beta.yml.
//!
//! `winresource` chỉ là build-dep của target windows → toàn bộ code phải nằm sau
//! `#[cfg(windows)]` (host-gate), nếu không build.rs fail compile trên host
//! Linux/macOS (crate không nằm trong dep set của các target đó).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    embed_windows_resources();
}

#[cfg(windows)]
fn embed_windows_resources() {
    // build.rs compile cho HOST — cross-build windows từ host khác vẫn cần check.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let mut parts = version
        .split(['-', '+'])
        .next()
        .unwrap_or("0.0.0")
        .split('.')
        .map(|p| p.parse::<u64>().unwrap_or(0))
        .collect::<Vec<_>>();
    parts.resize(4, 0);
    let numeric =
        (parts[0] << 48) | (parts[1] << 32) | (parts[2] << 16) | parts[3];
    let ver4 = format!("{}.{}.{}.{}", parts[0], parts[1], parts[2], parts[3]);

    let mut res = winresource::WindowsResource::new();
    res.set_version_info(winresource::VersionInfo::FILEVERSION, numeric)
        .set_version_info(winresource::VersionInfo::PRODUCTVERSION, numeric)
        .set("FileVersion", &ver4)
        .set("ProductVersion", &ver4)
        .set("ProductName", "OrgInventory Agent")
        .set("FileDescription", "OrgInventory Agent - He thong Quan ly Tai san CNTT & ATTT")
        .set("CompanyName", "Phong ANM & PCTP su dung CNC, Cong an tinh Ha Tinh")
        .set("LegalCopyright", "(c) 2025-2026 Phong ANM & PCTP su dung CNC, Cong an tinh Ha Tinh")
        .set("OriginalFilename", "OrgInventoryAgent.exe")
        .set("InternalName", "OrgInventoryAgent");

    // Icon exe (cùng icon ARP trong MSI) — optional: build vẫn pass nếu thiếu file.
    let icon = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Assets/agent.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    if icon.exists() {
        res.set_icon(icon.to_str().unwrap());
    }

    if let Err(e) = res.compile() {
        // Release build THIẾU VERSIONINFO sẽ sinh MSI nâng cấp bị disallow component
        // (mất service trên toàn bộ máy) → fail cứng để CI bắt lỗi ngay.
        // Debug build chỉ warn để dev không cần Windows SDK.
        let msg = format!("Không nhúng được VERSIONINFO resource: {e}");
        if std::env::var("PROFILE").as_deref() == Ok("release") {
            panic!("{msg}");
        }
        println!("cargo:warning={msg}");
    }
}
