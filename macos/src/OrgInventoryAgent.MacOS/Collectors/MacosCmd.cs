using System.Diagnostics;
using System.Text.Json;

namespace OrgInventoryAgent.MacOS.Collectors;

/// <summary>
/// Helper chạy lệnh hệ thống macOS (sysctl, system_profiler, ioreg, dscl...).
/// Mọi call đều có timeout + catch — collector KHÔNG BAO GIỜ throw, thiếu dữ
/// liệu thì trả null (contract giống Linux collectors).
/// </summary>
public static class MacosCmd
{
    /// <summary>Chạy lệnh, trả stdout (null khi lỗi/timeout). stderr nuốt.</summary>
    public static string? Run(string fileName, string arguments, int timeoutMs = 10_000)
    {
        try
        {
            var psi = new ProcessStartInfo
            {
                FileName = fileName,
                Arguments = arguments,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
            };
            using var p = Process.Start(psi);
            if (p is null) return null;
            if (!p.WaitForExit(timeoutMs)) { try { p.Kill(); } catch { } return null; }
            var output = p.StandardOutput.ReadToEnd();
            return string.IsNullOrWhiteSpace(output) ? null : output;
        }
        catch { return null; }
    }

    /// <summary>sysctl -n &lt;name&gt; → giá trị đã trim (null nếu không có).</summary>
    public static string? Sysctl(string name, int timeoutMs = 5_000) =>
        Run("/usr/sbin/sysctl", $"-n {name}", timeoutMs)?.Trim();

    /// <summary>system_profiler &lt;dataType&gt; -json → JsonDocument (caller Dispose).</summary>
    public static JsonDocument? SystemProfilerJson(string dataType, int timeoutMs = 30_000)
    {
        var json = Run("/usr/sbin/system_profiler", $"{dataType} -json", timeoutMs);
        if (json is null) return null;
        try { return JsonDocument.Parse(json); }
        catch { return null; }
    }

    /// <summary>Tìm giá trị string theo key trong JSON element đầu tiên của 1 dataType array.</summary>
    public static string? JsonString(JsonElement el, params string[] keys)
    {
        if (el.ValueKind != JsonValueKind.Object) return null;
        foreach (var key in keys)
        {
            if (el.TryGetProperty(key, out var v) && v.ValueKind == JsonValueKind.String)
            {
                var s = v.GetString();
                if (!string.IsNullOrWhiteSpace(s)) return s;
            }
        }
        return null;
    }
}
