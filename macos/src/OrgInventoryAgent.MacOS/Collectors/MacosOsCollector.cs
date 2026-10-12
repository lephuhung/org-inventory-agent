using System.Text.Json;
using OrgInventoryAgent.Core.Collectors.Schema;

namespace OrgInventoryAgent.MacOS.Collectors;

/// <summary>
/// Hardware/OS info trên macOS — qua sysctl, sw_vers, system_profiler -json
/// và ioreg. Mọi hàm fail-silent: thiếu dữ liệu → null (contract chung POSIX).
/// </summary>
public static class MacosOsCollector
{
    /// <summary>PRETTY_NAME tương đương: "macOS 14.5".</summary>
    public static string? GetOsPrettyName()
    {
        var parsed = ParseSwVers(MacosCmd.Run("/usr/bin/sw_vers", ""));
        if (parsed.Name is null) return null;
        return parsed.Version is null ? parsed.Name : $"{parsed.Name} {parsed.Version}";
    }

    /// <summary>Parse output `sw_vers` (text) → (Name, Version, Build).</summary>
    public static (string? Name, string? Version, string? Build) ParseSwVers(string? output)
    {
        string? name = null, version = null, build = null;
        if (output is not null)
        {
            foreach (var line in output.Split('\n'))
            {
                var parts = line.Split(':', 2);
                if (parts.Length != 2) continue;
                switch (parts[0].Trim())
                {
                    case "ProductName": name = parts[1].Trim(); break;
                    case "ProductVersion": version = parts[1].Trim(); break;
                    case "BuildVersion": build = parts[1].Trim(); break;
                }
            }
        }
        return (name, version, build);
    }

    public static CpuInfo? GetCpu()
    {
        // machdep.cpu.brand_string chỉ có trên Intel; Apple Silicon không có
        // → fallback chip_type trong SPHardwareDataType, rồi hw.model.
        var model = MacosCmd.Sysctl("machdep.cpu.brand_string")
                    ?? GetHardwareField("chip_type")
                    ?? MacosCmd.Sysctl("hw.model");
        return model is null ? null : new CpuInfo { Model = model, Cores = Environment.ProcessorCount };
    }

    public static double? GetRamGb()
    {
        var raw = MacosCmd.Sysctl("hw.memsize");
        return long.TryParse(raw, out var bytes) ? Math.Round(bytes / (1024.0 * 1024.0 * 1024.0), 1) : null;
    }

    /// <summary>Disks qua SPStorageDataType: gom theo physical_drive, type từ medium_type.</summary>
    public static List<DiskInfo>? GetDisks()
    {
        using var doc = MacosCmd.SystemProfilerJson("SPStorageDataType");
        if (doc is null) return null;
        return ParseStorageDataType(doc.RootElement);
    }

    /// <summary>Parse SPStorageDataType -json (testable với canned JSON).</summary>
    public static List<DiskInfo>? ParseStorageDataType(JsonElement root)
    {
        try
        {
            if (!root.TryGetProperty("SPStorageDataType", out var items) || items.ValueKind != JsonValueKind.Array)
                return null;
            var byDevice = new Dictionary<string, DiskInfo>(StringComparer.OrdinalIgnoreCase);
            foreach (var vol in items.EnumerateArray())
            {
                if (!vol.TryGetProperty("physical_drive", out var drive) || drive.ValueKind != JsonValueKind.Object)
                    continue;
                var model = MacosCmd.JsonString(drive, "device_name", "media_name");
                if (model is null) continue;
                var medium = MacosCmd.JsonString(drive, "medium_type") ?? "";
                long size = 0;
                if (vol.TryGetProperty("size_in_bytes", out var sz) && sz.ValueKind == JsonValueKind.Number)
                    size = sz.GetInt64();
                if (byDevice.TryGetValue(model, out var existing))
                {
                    if (size > (existing.SizeBytes ?? 0)) existing.SizeBytes = size;
                }
                else
                {
                    byDevice[model] = new DiskInfo
                    {
                        Model = model,
                        SizeBytes = size > 0 ? size : null,
                        SizeGb = size > 0 ? Math.Round(size / (1024.0 * 1024.0 * 1024.0), 0) : null,
                        Type = medium.Contains("ssd", StringComparison.OrdinalIgnoreCase) ? "SSD"
                             : medium.Contains("hdd", StringComparison.OrdinalIgnoreCase) ||
                               medium.Contains("rotational", StringComparison.OrdinalIgnoreCase) ? "HDD"
                             : null,
                    };
                }
            }
            return byDevice.Count > 0 ? new List<DiskInfo>(byDevice.Values) : null;
        }
        catch { return null; }
    }

    /// <summary>Mainboard-equivalent: Model Identifier + Serial (Mac không có board riêng).</summary>
    public static MainboardInfo? GetMainboard()
    {
        using var doc = MacosCmd.SystemProfilerJson("SPHardwareDataType");
        if (doc is null) return null;
        try
        {
            if (!doc.RootElement.TryGetProperty("SPHardwareDataType", out var items) ||
                items.ValueKind != JsonValueKind.Array || items.GetArrayLength() == 0)
                return null;
            var hw = items[0];
            var model = MacosCmd.JsonString(hw, "machine_name", "model_name") is { } name
                ? (MacosCmd.JsonString(hw, "machine_model", "model_identifier") is { } id ? $"{name} ({id})" : name)
                : MacosCmd.JsonString(hw, "machine_model", "model_identifier");
            var serial = MacosCmd.JsonString(hw, "serial_number");
            if (model is null && serial is null) return null;
            return new MainboardInfo { Model = model, Serial = serial };
        }
        catch { return null; }
    }

    /// <summary>Boot ROM (Intel) / iBoot (Apple Silicon) — tương đương BIOS.</summary>
    public static BiosInfo? GetBios()
    {
        var v = GetHardwareField("boot_rom_version") ?? GetHardwareField("os_loader_version");
        return v is null ? null : new BiosInfo { Version = v };
    }

    public static string? GetGpuModel()
    {
        using var doc = MacosCmd.SystemProfilerJson("SPDisplaysDataType");
        if (doc is null) return null;
        try
        {
            if (!doc.RootElement.TryGetProperty("SPDisplaysDataType", out var items) ||
                items.ValueKind != JsonValueKind.Array || items.GetArrayLength() == 0)
                return null;
            // Intel: spdisplays_ndrvs[] mỗi GPU 1 entry có _name/sppci_model.
            // ARM: entry đầu có sppci_model = tên chip (GPU tích hợp).
            var first = items[0];
            if (first.TryGetProperty("spdisplays_ndrvs", out var gpus) &&
                gpus.ValueKind == JsonValueKind.Array && gpus.GetArrayLength() > 0)
            {
                return MacosCmd.JsonString(gpus[0], "_name", "sppci_model");
            }
            return MacosCmd.JsonString(first, "_name", "sppci_model", "spdisplays_gpu");
        }
        catch { return null; }
    }

    public static bool? GetIsVm()
    {
        // kern.hv_vmm_present=1 → đang chạy trong hypervisor (VMware/Parallels/UTM).
        if (MacosCmd.Sysctl("kern.hv_vmm_present") == "1") return true;
        var model = (MacosCmd.Sysctl("hw.model") ?? "").ToLowerInvariant();
        if (model.Length == 0) return null;
        var markers = new[] { "vmware", "virtualbox", "parallels", "qemu", "virtualmac", "vm" };
        return markers.Any(m => model.Contains(m, StringComparison.OrdinalIgnoreCase));
    }

    /// <summary>User đang login console: `stat -f %Su /dev/console`.</summary>
    public static string? GetLoggedUser()
    {
        var user = MacosCmd.Run("/usr/bin/stat", "-f %Su /dev/console")?.Trim();
        if (!string.IsNullOrWhiteSpace(user) && user != "root" && !user.StartsWith('_')) return user;
        return Environment.UserName;
    }

    /// <summary>Đọc 1 field trong SPHardwareDataType entry đầu.</summary>
    public static string? GetHardwareField(params string[] keys)
    {
        using var doc = MacosCmd.SystemProfilerJson("SPHardwareDataType");
        if (doc is null) return null;
        try
        {
            if (!doc.RootElement.TryGetProperty("SPHardwareDataType", out var items) ||
                items.ValueKind != JsonValueKind.Array || items.GetArrayLength() == 0)
                return null;
            return MacosCmd.JsonString(items[0], keys);
        }
        catch { return null; }
    }
}
