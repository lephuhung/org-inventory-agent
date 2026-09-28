using System.Security.Cryptography;
using System.Text;
using YamlDotNet.Core;
using YamlDotNet.Serialization;
using YamlDotNet.Serialization.NamingConventions;

namespace OrgInventoryAgent.Core;

/// <summary>
/// agent.config.yaml — client config do backend sinh (<c>GET /download/agent.config.yaml</c>),
/// tương tự <c>client.config.yaml</c> của Velociraptor: binary/MSI tải từ GitHub Releases,
/// cấu hình tải từ backend và installer đặt cạnh agent.
///
/// Áp dụng khi nội dung file khác lần áp dụng trước (so <see cref="AgentConfig.ClientConfigHash"/>)
/// để cấu hình server đẩy về qua mTLS sau đó không bị file cũ ghi đè mỗi lần restart.
/// </summary>
public sealed class ClientConfig
{
    public const string FileName = "agent.config.yaml";
    public const int SupportedVersion = 1;

    public int Version { get; set; }
    public List<string>? ServerUrls { get; set; }
    public string? PortalUrl { get; set; }
    public int? HeartbeatIntervalSeconds { get; set; }
    public int? HeartbeatJitterSeconds { get; set; }
    public int? InventoryIntervalHours { get; set; }
    public int? RenewBeforePercent { get; set; }
    public string? HttpProxy { get; set; }
    public string? AgentConfigHash { get; set; }

    private static readonly IDeserializer Deserializer = new DeserializerBuilder()
        .WithNamingConvention(UnderscoredNamingConvention.Instance)
        .IgnoreUnmatchedProperties()
        .Build();

    /// <summary>Parse + validate YAML. Ném <see cref="FormatException"/> khi không hợp lệ.</summary>
    public static ClientConfig Parse(string yaml)
    {
        ClientConfig? doc;
        try { doc = Deserializer.Deserialize<ClientConfig?>(yaml); }
        catch (YamlException ex) { throw new FormatException($"YAML không hợp lệ: {ex.Message}", ex); }

        if (doc is null) throw new FormatException("File rỗng.");
        if (doc.Version != SupportedVersion)
            throw new FormatException($"version={doc.Version} không hỗ trợ (cần {SupportedVersion}).");

        var urls = (doc.ServerUrls ?? new List<string>())
            .Where(u => !string.IsNullOrWhiteSpace(u))
            .Select(u => u.Trim().TrimEnd('/'))
            .ToList();
        if (urls.Count == 0) throw new FormatException("Thiếu server_urls.");
        foreach (var u in urls)
        {
            if (!Uri.TryCreate(u, UriKind.Absolute, out var uri) || (uri.Scheme != Uri.UriSchemeHttps && uri.Scheme != Uri.UriSchemeHttp))
                throw new FormatException($"server_urls chứa URL không hợp lệ: {u}");
        }
        doc.ServerUrls = urls;
        return doc;
    }

    public static string ComputeHash(byte[] content) =>
        Convert.ToHexString(SHA256.HashData(content)).ToLowerInvariant();

    /// <summary>Ghi đè các trường cấu hình của <paramref name="cfg"/> bằng giá trị trong file.</summary>
    public void ApplyTo(AgentConfig cfg, string hash)
    {
        cfg.Endpoints = ServerUrls!.ToArray();
        if (HeartbeatIntervalSeconds is > 0) cfg.HeartbeatIntervalSeconds = HeartbeatIntervalSeconds.Value;
        if (HeartbeatJitterSeconds is >= 0) cfg.HeartbeatJitterSeconds = HeartbeatJitterSeconds.Value;
        if (InventoryIntervalHours is > 0) cfg.InventoryIntervalHours = InventoryIntervalHours.Value;
        if (RenewBeforePercent is > 0 and < 100) cfg.RenewBeforePercent = RenewBeforePercent.Value;
        if (!string.IsNullOrWhiteSpace(HttpProxy)) cfg.HttpProxy = HttpProxy.Trim();
        cfg.ClientConfigHash = hash;
        cfg.Normalize();
    }

    /// <summary>
    /// Đọc <paramref name="path"/> và áp dụng vào <paramref name="cfg"/> nếu file mới/đã đổi.
    /// Trả <c>true</c> khi cfg thay đổi (caller phải Save). File thiếu → false; file lỗi →
    /// false + <paramref name="error"/> (giữ nguyên cấu hình hiện tại).
    /// </summary>
    public static bool TryApplyFile(AgentConfig cfg, string path, out string? error)
    {
        error = null;
        if (!File.Exists(path)) return false;
        try
        {
            var bytes = File.ReadAllBytes(path);
            var hash = ComputeHash(bytes);
            if (string.Equals(hash, cfg.ClientConfigHash, StringComparison.OrdinalIgnoreCase)) return false;
            Parse(Encoding.UTF8.GetString(bytes).TrimStart('\uFEFF')).ApplyTo(cfg, hash);
            return true;
        }
        catch (Exception ex) when (ex is FormatException or IOException or UnauthorizedAccessException)
        {
            error = $"Bỏ qua {path}: {ex.Message}";
            return false;
        }
    }
}
