using System.Runtime.InteropServices;
using Microsoft.Extensions.Logging;
using OrgInventoryAgent.Core;
using OrgInventoryAgent.Core.Collectors;
using OrgInventoryAgent.Core.Collectors.Schema;

namespace OrgInventoryAgent.MacOS.Collectors;

/// <summary>
/// OS-specific collector for macOS. Builds a v4 InventoryEnvelope populated
/// with agent metadata, OS metadata, and SecurityPostureV4 from macOS collectors.
/// Hardware/software fields live in <see cref="InventorySnapshot"/> (flat schema).
/// </summary>
public sealed class MacosInventoryProvider : IPosixInventoryProvider
{
    private readonly ILogger<MacosInventoryProvider> _logger;

    public MacosInventoryProvider(ILogger<MacosInventoryProvider> logger) => _logger = logger;

    public InventoryEnvelope Collect()
    {
        var arch = RuntimeInformation.OSArchitecture.ToString().ToLowerInvariant(); // x64 | arm64
        return new InventoryEnvelope
        {
            Agent = new AgentMetadata
            {
                Name = AppInfo.Name,
                Version = Core.AppInfo.EffectiveVersion,
                Runtime = ".NET 8.0",
                Platform = "macos",
                Architecture = arch,
                PackageType = DetectPackageType(),
            },
            Os = ReadOsMetadata(arch),
            Security = MacosSecurityCollector.Collect(_logger),
        };
    }

    /// <summary>Collect flat InventorySnapshot (CPU, RAM, disks, network, software, etc.).</summary>
    public InventorySnapshot CollectSnapshot()
    {
        var arch = RuntimeInformation.OSArchitecture.ToString().ToLowerInvariant();
        return new InventorySnapshot
        {
            OsName = MacosOsCollector.GetOsPrettyName(),
            OsVersion = ReadOsMetadata(arch).DistributionVersion,
            OsBuild = ReadOsMetadata(arch).KernelVersion,
            OsArch = arch,
            Cpu = MacosOsCollector.GetCpu(),
            RamGb = MacosOsCollector.GetRamGb(),
            Disks = MacosOsCollector.GetDisks(),
            Gpu = MacosOsCollector.GetGpuModel() is { } gpu ? new GpuInfo { Model = gpu } : null,
            Mainboard = MacosOsCollector.GetMainboard(),
            Bios = MacosOsCollector.GetBios(),
            Network = PosixNetworkCollector.Collect(),
            LoggedUser = MacosOsCollector.GetLoggedUser(),
            InstalledSoftware = MacosSoftwareCollector.Collect(_logger),
            Security = MacosSecurityCollector.Collect(_logger),
            IsVm = MacosOsCollector.GetIsVm(),
        };
    }

    /// <summary>pkg khi cài qua installer package; launchd/script khi cài bằng install.sh.</summary>
    private static string? DetectPackageType()
    {
        try
        {
            var receipt = MacosCmd.Run("/usr/sbin/pkgutil", "--pkg-info com.orginventory.agent");
            if (receipt is not null) return "pkg";
        }
        catch { }
        return "bin";
    }

    private static OsMetadata ReadOsMetadata(string arch)
    {
        var meta = new OsMetadata { Platform = "macos", Architecture = arch };
        try
        {
            var (name, version, build) = MacosOsCollector.ParseSwVers(MacosCmd.Run("/usr/bin/sw_vers", ""));
            meta.Distribution = name ?? "macOS";
            meta.DistributionVersion = version;
            // Kernel = Darwin version (sw_vers BuildVersion là OS build, kernel là uname -r)
            meta.KernelVersion = MacosCmd.Sysctl("kern.osrelease") ?? build;
        }
        catch { }
        return meta;
    }
}
