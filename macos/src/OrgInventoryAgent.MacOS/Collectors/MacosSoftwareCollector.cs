using System.Text.Json;
using Microsoft.Extensions.Logging;
using OrgInventoryAgent.Core.Collectors.Schema;

namespace OrgInventoryAgent.MacOS.Collectors;

/// <summary>
/// Danh mục phần mềm macOS: system_profiler SPApplicationsDataType -json
/// (quét /Applications, /System/Applications, ~/Applications, /Applications/Utilities).
/// Fallback: duyệt thư mục *.app trực tiếp khi profiler lỗi/quá chậm.
/// Cap 500 như bản Linux.
/// </summary>
public static class MacosSoftwareCollector
{
    private const int MaxApps = 500;

    public static List<SoftwareInfo> Collect(ILogger logger)
    {
        using var doc = MacosCmd.SystemProfilerJson("SPApplicationsDataType", timeoutMs: 60_000);
        if (doc is not null)
        {
            var list = ParseApplicationsDataType(doc.RootElement);
            if (list.Count > 0) return list;
        }

        // Fallback: scan *.app (chỉ có tên, không version) — vẫn tốt hơn trả rỗng.
        logger.LogDebug("SPApplicationsDataType lỗi/rỗng — fallback scan /Applications.");
        return ScanApplicationsDirs();
    }

    /// <summary>Parse SPApplicationsDataType -json (testable với canned JSON).</summary>
    public static List<SoftwareInfo> ParseApplicationsDataType(JsonElement root)
    {
        var list = new List<SoftwareInfo>();
        try
        {
            if (!root.TryGetProperty("SPApplicationsDataType", out var items) || items.ValueKind != JsonValueKind.Array)
                return list;
            foreach (var app in items.EnumerateArray())
            {
                var name = MacosCmd.JsonString(app, "_name");
                if (name is null) continue;
                var path = MacosCmd.JsonString(app, "path");
                list.Add(new SoftwareInfo
                {
                    DisplayName = name,
                    Name = name,
                    Version = MacosCmd.JsonString(app, "version"),
                    Publisher = MapObtainedFrom(MacosCmd.JsonString(app, "obtained_from")),
                    IsPerUser = path?.StartsWith("/Users/", StringComparison.Ordinal) == true,
                });
                if (list.Count >= MaxApps) break;
            }
        }
        catch { }
        return list;
    }

    /// <summary>obtained_from: apple | mac_app_store | identified_developer | unknown.</summary>
    private static string? MapObtainedFrom(string? obtained) => obtained switch
    {
        "apple" => "Apple",
        "mac_app_store" => "Mac App Store",
        "identified_developer" => "Identified Developer",
        "unknown" => null,
        _ => obtained,
    };

    private static List<SoftwareInfo> ScanApplicationsDirs()
    {
        var list = new List<SoftwareInfo>();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var dir in new[] { "/Applications", "/System/Applications", "/System/Applications/Utilities" })
        {
            try
            {
                if (!Directory.Exists(dir)) continue;
                foreach (var app in Directory.GetDirectories(dir, "*.app"))
                {
                    var name = Path.GetFileNameWithoutExtension(app);
                    if (seen.Add(name))
                    {
                        list.Add(new SoftwareInfo
                        {
                            DisplayName = name,
                            Name = name,
                            IsPerUser = false,
                        });
                        if (list.Count >= MaxApps) return list;
                    }
                }
            }
            catch { }
        }
        return list;
    }
}
