using System.Diagnostics;
using System.Net.Http.Headers;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Nodes;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;

namespace OrgInventoryAgent.Core.Services;

/// <summary>
/// Auto-update: định kỳ tải manifest phiên bản (agent-version.json trên GitHub
/// Releases /latest, hoặc UpdateManifestUrl custom), so sánh semver với bản đang
/// chạy, nếu mới hơn thì tải asset + verify SHA-256 rồi áp dụng.
///
/// Áp dụng theo OS:
/// - Windows: stage MSI rồi spawn `msiexec /i /qn` tách biệt — MSI (MajorUpgrade
///   + ServiceControl Stop=both/Start=install) tự stop/đổi file/start lại service.
/// - Linux: agent chạy user `orginventory` không ghi được /opt → stage binary vào
///   {DataDir}/update/ + ghi marker `update.pending`; systemd path unit
///   `orginventory-agent-update.path` (do installer cài) kích hoạt
///   `apply-update.sh` bằng root để thay binary + restart. Thiếu unit (bản cài
///   cũ) → chỉ stage + cảnh báo; fallback chạy trực tiếp nếu đang là root.
/// </summary>
public sealed class UpdateService : BackgroundService
{
    /// <summary>Manifest mặc định — release "latest" của repo agent.</summary>
    public const string DefaultManifestUrl =
        "https://github.com/lephuhung/org-inventory-agent/releases/latest/download/agent-version.json";

    /// <summary>Marker mà orginventory-agent-update.path watch (systemd PathExists).</summary>
    public const string PendingMarkerName = "update.pending";

    private static readonly TimeSpan InitialDelay = TimeSpan.FromSeconds(90);
    private static readonly TimeSpan HttpTimeout = TimeSpan.FromSeconds(30);
    private static readonly TimeSpan DownloadTimeout = TimeSpan.FromMinutes(10);

    /// <summary>Version đang chạy — lấy từ entry assembly (exe), KHÔNG phải Core.dll
    /// (release pipeline chỉ pin version vào csproj exe; Core luôn mang version gốc).
    /// So sánh sai ở đây sẽ khiến agent tải cập nhật lặp vô hạn.</summary>
    public static string CurrentVersion =>
        System.Reflection.Assembly.GetEntryAssembly()?.GetName().Version?.ToString(3)
        ?? AppInfo.Version;

    private readonly AgentConfig _config;
    private readonly AgentState _state;
    private readonly ILogger<UpdateService> _logger;
    private readonly HttpClient _http;

    public UpdateService(AgentConfig config, AgentState state, ILogger<UpdateService> logger)
    {
        _config = config;
        _state = state;
        _logger = logger;
        _http = new HttpClient();
        _http.DefaultRequestHeaders.UserAgent.Add(
            new ProductInfoHeaderValue("OrgInventoryAgent", CurrentVersion));
    }

    public override void Dispose()
    {
        _http.Dispose();
        base.Dispose();
    }

    protected override async Task ExecuteAsync(CancellationToken ct)
    {
        if (!_config.AutoUpdateEnabled)
        {
            _logger.LogInformation("Auto-update tắt (autoUpdateEnabled=false).");
            return;
        }

        try { await Task.Delay(InitialDelay, ct); }
        catch (OperationCanceledException) { return; }

        while (!ct.IsCancellationRequested)
        {
            try
            {
                await CheckAndApplyAsync(ct);
            }
            catch (OperationCanceledException) when (ct.IsCancellationRequested)
            {
                break;
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Kiểm tra cập nhật lỗi.");
            }

            // Jitter ±25% chống đồng loạt tải (nhiều máy cùng giờ).
            var hours = Math.Clamp(_config.UpdateCheckIntervalHours, 1, 24 * 7);
            var jitter = 1.0 + (Random.Shared.NextDouble() - 0.5) * 0.5;
            var delay = TimeSpan.FromHours(hours * jitter);
            try { await Task.Delay(delay, ct); }
            catch (OperationCanceledException) { break; }
        }
    }

    // ── Luồng chính: check → download → verify → apply ────────────────

    /// <summary>Kết quả của 1 lần check (dùng cho service loop và CLI --check-update).</summary>
    public sealed record UpdateCheckResult(
        string CurrentVersion,
        string? LatestVersion,
        bool UpdateAvailable,
        string? PendingVersion,
        string? Error);

    /// <summary>Chỉ kiểm tra (không tải/không áp dụng) — cho --check-update.</summary>
    public async Task<UpdateCheckResult> CheckAsync(CancellationToken ct)
    {
        try
        {
            var manifestJson = await FetchManifestAsync(ct);
            if (manifestJson is null)
                return new(CurrentVersion, null, false, ReadPendingVersion(), "Không tải được manifest.");

            var latest = UpdateManifest.SelectVersion(manifestJson, CurrentRid());
            var newer = latest is not null && UpdateManifest.IsNewer(latest, CurrentVersion);
            return new(CurrentVersion, latest, newer, ReadPendingVersion(), null);
        }
        catch (Exception ex)
        {
            return new(CurrentVersion, null, false, ReadPendingVersion(), ex.Message);
        }
    }

    /// <summary>Check + tải + áp dụng nếu có bản mới. Trả về true khi update đã được stage/apply.</summary>
    public async Task<bool> CheckAndApplyAsync(CancellationToken ct)
    {
        var manifestUrl = ManifestUrl();
        var manifestJson = await FetchManifestAsync(ct);
        _state.LastUpdateCheckAt = DateTimeOffset.UtcNow.ToString("o");
        _state.Save();
        if (manifestJson is null) return false;

        var latest = UpdateManifest.SelectVersion(manifestJson, CurrentRid());
        if (latest is null)
        {
            _logger.LogDebug("Manifest không có entry cho RID {Rid}.", CurrentRid());
            return false;
        }
        if (!UpdateManifest.IsNewer(latest, CurrentVersion))
        {
            _logger.LogDebug("Đã ở bản mới nhất ({Current} >= {Latest}).", CurrentVersion, latest);
            CleanupStagingIfIdle();
            return false;
        }

        var pending = ReadPendingVersion();
        if (pending is not null && !UpdateManifest.IsNewer(latest, pending))
        {
            // Đã stage bản này rồi, đang chờ updater áp dụng (hoặc updater chưa cài).
            _logger.LogInformation("Bản {Ver} đã stage, đang chờ apply.", pending);
            EnsurePendingMarker(pending);
            return true;
        }

        var assetName = UpdateManifest.AssetName(CurrentRid());
        var stagedName = UpdateManifest.StagedName(CurrentRid());
        var baseUrl = manifestUrl[..manifestUrl.LastIndexOf('/')];
        var assetUrl = $"{baseUrl}/{assetName}";
        var shaUrl = assetUrl + ".sha256";

        _logger.LogInformation("Có phiên bản mới {Latest} (đang chạy {Current}) — tải {Asset}...",
            latest, CurrentVersion, assetUrl);

        var stageDir = AppPaths.UpdateDir;
        Directory.CreateDirectory(stageDir);
        CleanupStagingIfIdle();

        var staged = Path.Combine(stageDir, stagedName);
        var tmp = staged + ".part";
        try
        {
            await DownloadToFileAsync(assetUrl, tmp, DownloadTimeout, ct);

            // SHA-256 bắt buộc — auto-update không người giám sát phải fail-closed.
            var expectedSha = await FetchTextAsync(shaUrl, HttpTimeout, ct);
            if (string.IsNullOrWhiteSpace(expectedSha))
            {
                _logger.LogWarning("Thiếu sidecar SHA-256 ({Url}) — hủy cập nhật.", shaUrl);
                TryDelete(tmp);
                return false;
            }
            var expected = expectedSha.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries)[0].Trim();
            var actual = Convert.ToHexString(SHA256.HashData(await File.ReadAllBytesAsync(tmp, ct))).ToLowerInvariant();
            if (!string.Equals(expected, actual, StringComparison.OrdinalIgnoreCase))
            {
                _logger.LogWarning("SHA-256 không khớp (server={Exp}, file={Act}) — hủy cập nhật.", expected, actual);
                TryDelete(tmp);
                return false;
            }

            File.Move(tmp, staged, true);
            if (!OperatingSystem.IsWindows())
            {
                try { File.SetUnixFileMode(staged, UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute | UnixFileMode.GroupRead | UnixFileMode.GroupExecute | UnixFileMode.OtherRead | UnixFileMode.OtherExecute); }
                catch { }
            }
            await File.WriteAllTextAsync(staged + ".version", latest + "\n", ct);
            // Sidecar sha256 staged lại để apply-update.sh re-verify trước khi install.
            await File.WriteAllTextAsync(staged + ".sha256", $"{actual}  {stagedName}\n", ct);
        }
        catch (OperationCanceledException) when (ct.IsCancellationRequested) { TryDelete(tmp); throw; }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Tải bản mới thất bại.");
            TryDelete(tmp);
            return false;
        }

        _logger.LogInformation("Đã tải + verify {Asset} v{Latest} ({Size:N0} bytes).",
            stagedName, latest, new FileInfo(staged).Length);

        if (OperatingSystem.IsWindows())
            StartWindowsUpgrade(staged, latest);
        else
            StageLinuxUpdate(latest);
        return true;
    }

    // ── Windows: MSI self-upgrade ─────────────────────────────────────

    private void StartWindowsUpgrade(string msiPath, string version)
    {
        try
        {
            var log = Path.Combine(AppPaths.UpdateDir, "msi-install.log");
            // msiexec chạy độc lập; Windows Installer service (msiserver) thực hiện
            // upgrade — service này sẽ bị stop (ServiceControl Stop=both), file được
            // thay, service start lại (Start=install). Không cần helper script.
            var psi = new ProcessStartInfo
            {
                FileName = "msiexec.exe",
                Arguments = $"/i \"{msiPath}\" /qn /norestart /l*v \"{log}\"",
                UseShellExecute = false,
                CreateNoWindow = true,
            };
            Process.Start(psi);
            _logger.LogInformation("Đã khởi chạy nâng cấp MSI v{Ver} — service sẽ restart trong giây lát.", version);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Không chạy được msiexec — bản {Ver} giữ ở staging.", version);
        }
    }

    // ── Linux: staging + marker cho updater path unit ─────────────────

    private void StageLinuxUpdate(string version)
    {
        var pending = Path.Combine(AppPaths.UpdateDir, PendingMarkerName);
        try
        {
            File.WriteAllText(pending, version + "\n");
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Không ghi được pending marker.");
            return;
        }

        if (FindUpdaterUnit() is not null)
        {
            _logger.LogInformation("Đã stage v{Ver} — orginventory-agent-update.path sẽ áp dụng.", version);
            return;
        }

        // Fallback: chạy trực tiếp khi đang root (dev/manual) và có apply script.
        const string applyScript = "/opt/orginventory/apply-update.sh";
        if (Environment.UserName == "root" && File.Exists(applyScript))
        {
            try
            {
                Process.Start(new ProcessStartInfo
                {
                    FileName = "/bin/bash",
                    Arguments = $"\"{applyScript}\"",
                    UseShellExecute = false,
                    CreateNoWindow = true,
                });
                _logger.LogInformation("Chạy apply-update.sh trực tiếp (root).");
                return;
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Không chạy được apply-update.sh.");
            }
        }

        _logger.LogWarning(
            "Update v{Ver} đã stage tại {Dir} nhưng thiếu systemd unit " +
            "orginventory-agent-update.path — cài lại bằng install script bản mới để bật auto-apply.",
            version, AppPaths.UpdateDir);
    }

    private void EnsurePendingMarker(string version)
    {
        try
        {
            var pending = Path.Combine(AppPaths.UpdateDir, PendingMarkerName);
            if (!File.Exists(pending))
                File.WriteAllText(pending, version + "\n");
        }
        catch { }
    }

    private string? ReadPendingVersion()
    {
        try
        {
            var pending = Path.Combine(AppPaths.UpdateDir, PendingMarkerName);
            if (File.Exists(pending))
            {
                var v = File.ReadAllText(pending).Trim();
                return string.IsNullOrWhiteSpace(v) ? null : v;
            }
        }
        catch { }
        return null;
    }

    /// <summary>Xóa file staged khi không còn update nào pending (tránh rác tích lũy).</summary>
    private void CleanupStagingIfIdle()
    {
        try
        {
            var dir = AppPaths.UpdateDir;
            if (File.Exists(Path.Combine(dir, PendingMarkerName))) return;
            foreach (var f in Directory.GetFiles(dir))
            {
                if (f.EndsWith(PendingMarkerName, StringComparison.Ordinal)) continue;
                TryDelete(f);
            }
        }
        catch { }
    }

    private static string? FindUpdaterUnit()
    {
        foreach (var dir in new[] { "/etc/systemd/system", "/usr/lib/systemd/system", "/lib/systemd/system" })
        {
            var p = Path.Combine(dir, "orginventory-agent-update.path");
            if (File.Exists(p)) return p;
        }
        return null;
    }

    // ── HTTP helpers ──────────────────────────────────────────────────

    private string ManifestUrl() =>
        string.IsNullOrWhiteSpace(_config.UpdateManifestUrl)
            ? DefaultManifestUrl
            : _config.UpdateManifestUrl!.Trim();

    private async Task<string?> FetchManifestAsync(CancellationToken ct)
    {
        var json = await FetchTextAsync(ManifestUrl(), HttpTimeout, ct);
        return json;
    }

    private async Task<string?> FetchTextAsync(string url, TimeSpan timeout, CancellationToken ct)
    {
        try
        {
            using var cts = CancellationTokenSource.CreateLinkedTokenSource(ct);
            cts.CancelAfter(timeout);
            using var resp = await _http.GetAsync(url, HttpCompletionOption.ResponseHeadersRead, cts.Token);
            if (!resp.IsSuccessStatusCode)
            {
                _logger.LogWarning("GET {Url} → HTTP {Status}", url, (int)resp.StatusCode);
                return null;
            }
            return await resp.Content.ReadAsStringAsync(cts.Token);
        }
        catch (OperationCanceledException) when (!ct.IsCancellationRequested)
        {
            _logger.LogWarning("GET {Url} timeout.", url);
            return null;
        }
        catch (Exception ex)
        {
            _logger.LogWarning("GET {Url} lỗi: {Msg}", url, ex.Message);
            return null;
        }
    }

    private async Task DownloadToFileAsync(string url, string path, TimeSpan timeout, CancellationToken ct)
    {
        using var cts = CancellationTokenSource.CreateLinkedTokenSource(ct);
        cts.CancelAfter(timeout);
        using var resp = await _http.GetAsync(url, HttpCompletionOption.ResponseHeadersRead, cts.Token);
        resp.EnsureSuccessStatusCode();
        await using var fs = new FileStream(path, FileMode.Create, FileAccess.Write, FileShare.None, 1 << 16, true);
        await resp.Content.CopyToAsync(fs, cts.Token);
    }

    private static void TryDelete(string path)
    {
        try { File.Delete(path); } catch { }
    }

    // ── Platform target ───────────────────────────────────────────────

    /// <summary>RID dùng để tra manifest (win-x64 / linux-x64 / linux-arm64).</summary>
    public static string CurrentRid()
    {
        if (OperatingSystem.IsWindows())
            return RuntimeInformation.OSArchitecture == Architecture.X86 ? "win-x86" : "win-x64";
        return RuntimeInformation.OSArchitecture == Architecture.Arm64 ? "linux-arm64" : "linux-x64";
    }
}

/// <summary>
/// Parse/chọn phiên bản từ manifest agent-version.json:
/// {"msi_version": "1.2.2", "linux": {"linux-x64": "1.2.2", "linux-arm64": "1.2.2"}}
/// </summary>
public static class UpdateManifest
{
    /// <summary>Version dành cho RID; null khi manifest hỏng/thiếu entry.</summary>
    public static string? SelectVersion(string manifestJson, string rid)
    {
        try
        {
            var node = JsonNode.Parse(manifestJson);
            if (node is not JsonObject obj) return null;

            if (rid.StartsWith("win", StringComparison.Ordinal))
                return obj["msi_version"]?.GetValue<string>();

            var linux = obj["linux"] as JsonObject;
            return linux?[rid]?.GetValue<string>();
        }
        catch (Exception)
        {
            return null;
        }
    }

    /// <summary>Tên asset trên GitHub Releases cho RID.</summary>
    public static string AssetName(string rid) =>
        rid.StartsWith("win", StringComparison.Ordinal)
            ? "OrgInventoryAgent.msi"
            : $"OrgInventoryAgent-{rid}";

    /// <summary>Tên file trong staging dir (canonical — apply-update.sh / msiexec dùng).
    /// Linux luôn stage thành 'OrgInventoryAgent' bất kể RID suffix của asset.</summary>
    public static string StagedName(string rid) =>
        rid.StartsWith("win", StringComparison.Ordinal)
            ? "OrgInventoryAgent.msi"
            : "OrgInventoryAgent";

    /// <summary>true nếu candidate mới hơn current (semver; chấp nhận prefix 'v').</summary>
    public static bool IsNewer(string candidate, string current)
    {
        if (!TryParseVersion(candidate, out var c) || !TryParseVersion(current, out var cur))
            return false;
        return c > cur;
    }

    public static bool TryParseVersion(string? text, out Version version)
    {
        version = new Version(0, 0);
        if (string.IsNullOrWhiteSpace(text)) return false;
        var t = text.Trim().TrimStart('v', 'V');
        // Cắt hậu tố (-beta, +build) — release tag của repo là semver 3 số.
        var dash = t.IndexOfAny(new[] { '-', '+' });
        if (dash >= 0) t = t[..dash];
        return Version.TryParse(t, out version!);
    }
}
