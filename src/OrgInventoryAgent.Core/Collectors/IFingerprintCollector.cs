using OrgInventoryAgent.Core.Collectors.Schema;

namespace OrgInventoryAgent.Core.Collectors;

/// <summary>
/// OS-specific fingerprint collector (đa nguồn: SMBIOS UUID / IOPlatformUUID,
/// machine-id, board/hardware serial). Windows có FingerprintCollector riêng;
/// Linux/macOS implement interface này để EnrollCoordinator dùng chung.
/// </summary>
public interface IFingerprintCollector
{
    FingerprintPayload Collect();
}
