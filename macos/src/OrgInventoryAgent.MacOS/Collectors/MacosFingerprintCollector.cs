using System.Security.Cryptography;
using System.Text;
using Microsoft.Extensions.Logging;
using OrgInventoryAgent.Core.Collectors;
using OrgInventoryAgent.Core.Collectors.Schema;

namespace OrgInventoryAgent.MacOS.Collectors;

/// <summary>
/// Fingerprint đa nguồn trên macOS:
/// - SmbiosUuid ← IOPlatformUUID (ioreg IOPlatformExpertDevice — tương đương SMBIOS UUID)
/// - MachineGuid ← SHA-256(kern.hostuuid)
/// - MainboardSerial ← SHA-256(serial number phần cứng, SPHardwareDataType)
/// </summary>
public sealed class MacosFingerprintCollector : IFingerprintCollector
{
    private readonly ILogger<MacosFingerprintCollector> _logger;

    public MacosFingerprintCollector(ILogger<MacosFingerprintCollector> logger) => _logger = logger;

    public FingerprintPayload Collect()
    {
        var rawUuid = GetIoPlatformUuid() ?? MacosOsCollector.GetHardwareField("platform_UUID");
        // kern.hostuuid thiếu trên Apple Virtualization VM → fallback hash
        // platform_UUID (vẫn machine-unique, khác slot smbios_uuid dùng raw).
        var rawHostUuid = MacosCmd.Sysctl("kern.hostuuid")
                          ?? MacosOsCollector.GetHardwareField("platform_UUID");
        var rawSerial = MacosOsCollector.GetHardwareField("serial_number");

        _logger.LogDebug("macOS fingerprint: ioplatform_uuid={A}, hostuuid={B}, serial={C}",
            rawUuid is null ? "(null)" : "set",
            rawHostUuid is null ? "(null)" : "set",
            rawSerial is null ? "(null)" : "set");

        return new FingerprintPayload
        {
            SmbiosUuid = Sanitize(rawUuid),
            MachineGuid = HashOrNull(rawHostUuid),
            MainboardSerial = HashOrNull(rawSerial),
        };
    }

    /// <summary>ioreg -rd1 -c IOPlatformExpertDevice → parse "IOPlatformUUID" = "...".</summary>
    public static string? GetIoPlatformUuid()
    {
        var output = MacosCmd.Run("/usr/sbin/ioreg", "-rd1 -c IOPlatformExpertDevice");
        return ParseIoPlatformUuid(output);
    }

    /// <summary>Parse ioreg text output (testable).</summary>
    public static string? ParseIoPlatformUuid(string? ioregOutput)
    {
        if (ioregOutput is null) return null;
        const string key = "\"IOPlatformUUID\"";
        var idx = ioregOutput.IndexOf(key, StringComparison.Ordinal);
        if (idx < 0) return null;
        var eq = ioregOutput.IndexOf('=', idx);
        if (eq < 0) return null;
        var rest = ioregOutput[(eq + 1)..].Trim();
        // Dạng: "XXXXXXXX-XXXX-..." hoặc <"XXXX-..."> — lấy cặp ngoặc kép đầu tiên.
        var first = rest.IndexOf('"');
        if (first < 0) return null;
        var close = rest.IndexOf('"', first + 1);
        if (close < 0) return null;
        return rest[(first + 1)..close];
    }

    private static string? Sanitize(string? v)
    {
        if (string.IsNullOrWhiteSpace(v)) return null;
        var low = v.Trim().ToLowerInvariant();
        if (low is "none" or "default string" or "to be filled by o.e.m." or "not applicable"
            or "system serial number" or "unknown" or "n/a" or "00000000-0000-0000-0000-000000000000"
            or "00000000-0000-0000-0000-000000000001")
            return null;
        return v.Trim();
    }

    private static string? HashOrNull(string? raw)
    {
        var clean = Sanitize(raw);
        if (clean is null) return null;
        var bytes = SHA256.HashData(Encoding.UTF8.GetBytes(clean));
        return Convert.ToHexString(bytes).ToLowerInvariant();
    }
}
