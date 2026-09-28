using System.IO;
using OrgInventoryAgent.Core;
using Xunit;

namespace OrgInventoryAgent.Core.Tests;

public class ClientConfigTests : IDisposable
{
    private const string BackendYaml = """
        # OrgInventory Agent client config — sinh bởi backend
        version: 1
        server_urls:
        - https://agent.example.gov.vn/
        portal_url: https://portal.example.gov.vn
        heartbeat_interval_seconds: 120
        heartbeat_jitter_seconds: 5
        inventory_interval_hours: 6
        renew_before_percent: 60
        agent_config_hash: abc
        future_field: ignored
        """;

    private readonly string _tempDir;
    private readonly string _yamlPath;

    public ClientConfigTests()
    {
        _tempDir = Path.Combine(Path.GetTempPath(), "ClientConfigTest_" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(_tempDir);
        AppPaths.Initialize(_tempDir);
        _yamlPath = AppPaths.ClientConfigFile;
    }

    public void Dispose()
    {
        try { if (Directory.Exists(_tempDir)) Directory.Delete(_tempDir, true); } catch { }
    }

    [Fact]
    public void TryApplyFile_AppliesBackendValues_AndRecordsHash()
    {
        File.WriteAllText(_yamlPath, BackendYaml);
        var cfg = new AgentConfig { Endpoints = new[] { "https://old.example.gov.vn" } };

        Assert.True(ClientConfig.TryApplyFile(cfg, _yamlPath, out var error));

        Assert.Null(error);
        Assert.Equal(new[] { "https://agent.example.gov.vn" }, cfg.Endpoints);
        Assert.Equal(120, cfg.HeartbeatIntervalSeconds);
        Assert.Equal(5, cfg.HeartbeatJitterSeconds);
        Assert.Equal(6, cfg.InventoryIntervalHours);
        Assert.Equal(60, cfg.RenewBeforePercent);
        Assert.Equal(ClientConfig.ComputeHash(File.ReadAllBytes(_yamlPath)), cfg.ClientConfigHash);
    }

    [Fact]
    public void TryApplyFile_SameFile_DoesNotOverrideServerSyncedValues()
    {
        File.WriteAllText(_yamlPath, BackendYaml);
        var cfg = new AgentConfig();
        Assert.True(ClientConfig.TryApplyFile(cfg, _yamlPath, out _));
        cfg.Save();

        cfg.ApplyServerSettings("https://new.example.gov.vn", 300, null, null, null);
        cfg.Save();

        var reloaded = AgentConfig.Load();
        Assert.False(ClientConfig.TryApplyFile(reloaded, _yamlPath, out var error));
        Assert.Null(error);
        Assert.Equal("https://new.example.gov.vn", reloaded.PrimaryEndpoint);
        Assert.Equal(300, reloaded.HeartbeatIntervalSeconds);
    }

    [Fact]
    public void TryApplyFile_ChangedFile_IsReapplied()
    {
        File.WriteAllText(_yamlPath, BackendYaml);
        var cfg = new AgentConfig();
        Assert.True(ClientConfig.TryApplyFile(cfg, _yamlPath, out _));

        File.WriteAllText(_yamlPath, BackendYaml.Replace("https://agent.example.gov.vn/", "https://agent2.example.gov.vn"));
        Assert.True(ClientConfig.TryApplyFile(cfg, _yamlPath, out _));
        Assert.Equal("https://agent2.example.gov.vn", cfg.PrimaryEndpoint);
    }

    [Fact]
    public void TryApplyFile_MissingFile_NoChange()
    {
        var cfg = new AgentConfig { Endpoints = new[] { "https://keep.example.gov.vn" } };
        Assert.False(ClientConfig.TryApplyFile(cfg, _yamlPath, out var error));
        Assert.Null(error);
        Assert.Equal("https://keep.example.gov.vn", cfg.PrimaryEndpoint);
    }

    [Theory]
    [InlineData("version: 1\nheartbeat_interval_seconds: 30\n")]
    [InlineData("version: 2\nserver_urls: [https://a.example.gov.vn]\n")]
    [InlineData("version: 1\nserver_urls: [ftp://a.example.gov.vn]\n")]
    [InlineData("version: 1\nserver_urls: [https://a\n")]
    [InlineData("")]
    public void TryApplyFile_InvalidFile_KeepsConfig_AndReportsError(string yaml)
    {
        File.WriteAllText(_yamlPath, yaml);
        var cfg = new AgentConfig { Endpoints = new[] { "https://keep.example.gov.vn" }, HeartbeatIntervalSeconds = 45 };

        Assert.False(ClientConfig.TryApplyFile(cfg, _yamlPath, out var error));

        Assert.NotNull(error);
        Assert.Equal("https://keep.example.gov.vn", cfg.PrimaryEndpoint);
        Assert.Equal(45, cfg.HeartbeatIntervalSeconds);
        Assert.Null(cfg.ClientConfigHash);
    }

    [Fact]
    public void TryApplyFile_AcceptsUtf8Bom()
    {
        File.WriteAllText(_yamlPath, BackendYaml, new System.Text.UTF8Encoding(encoderShouldEmitUTF8Identifier: true));
        var cfg = new AgentConfig();
        Assert.True(ClientConfig.TryApplyFile(cfg, _yamlPath, out var error));
        Assert.Null(error);
        Assert.Equal("https://agent.example.gov.vn", cfg.PrimaryEndpoint);
    }
}
