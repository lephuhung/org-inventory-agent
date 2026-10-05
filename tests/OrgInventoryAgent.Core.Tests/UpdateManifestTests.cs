using OrgInventoryAgent.Core.Services;
using Xunit;

namespace OrgInventoryAgent.Core.Tests;

/// <summary>
/// Test parse manifest agent-version.json + chọn asset + so sánh semver
/// của UpdateService (auto-update từ GitHub Releases).
/// </summary>
public class UpdateManifestTests
{
    private const string Manifest =
        """{"msi_version": "1.2.2", "linux": {"linux-x64": "1.2.2", "linux-arm64": "1.2.1"}, "velociraptor_msi_version": null}""";

    [Theory]
    [InlineData("win-x64", "1.2.2")]
    [InlineData("win-x86", "1.2.2")]
    [InlineData("linux-x64", "1.2.2")]
    [InlineData("linux-arm64", "1.2.1")]
    public void SelectVersion_PicksPerRid(string rid, string expected)
    {
        Assert.Equal(expected, UpdateManifest.SelectVersion(Manifest, rid));
    }

    [Fact]
    public void SelectVersion_MissingLinuxEntry_ReturnsNull()
    {
        var m = """{"msi_version": "1.2.2", "linux": {"linux-arm64": "1.2.2"}}""";
        Assert.Null(UpdateManifest.SelectVersion(m, "linux-x64"));
    }

    [Fact]
    public void SelectVersion_MissingMsi_ReturnsNull()
    {
        var m = """{"linux": {"linux-x64": "1.2.2"}}""";
        Assert.Null(UpdateManifest.SelectVersion(m, "win-x64"));
    }

    [Theory]
    [InlineData("")]
    [InlineData("not json")]
    [InlineData("{}")]
    [InlineData("[]")]
    [InlineData("null")]
    public void SelectVersion_Malformed_ReturnsNull(string manifest)
    {
        Assert.Null(UpdateManifest.SelectVersion(manifest, "linux-x64"));
    }

    [Theory]
    [InlineData("win-x64", "OrgInventoryAgent.msi")]
    [InlineData("linux-x64", "OrgInventoryAgent-linux-x64")]
    [InlineData("linux-arm64", "OrgInventoryAgent-linux-arm64")]
    public void AssetName_PerRid(string rid, string expected)
    {
        Assert.Equal(expected, UpdateManifest.AssetName(rid));
    }

    [Theory]
    [InlineData("win-x64", "OrgInventoryAgent.msi")]
    [InlineData("linux-x64", "OrgInventoryAgent")]
    [InlineData("linux-arm64", "OrgInventoryAgent")]
    public void StagedName_Canonical(string rid, string expected)
    {
        Assert.Equal(expected, UpdateManifest.StagedName(rid));
    }

    [Theory]
    [InlineData("1.2.2", "1.1.0", true)]
    [InlineData("1.2.2", "1.2.2", false)]
    [InlineData("1.1.0", "1.2.2", false)]
    [InlineData("2.0.0", "1.9.9", true)]
    [InlineData("v1.3.0", "1.2.9", true)]
    [InlineData("1.10.0", "1.9.0", true)] // semver, không phải string sort
    [InlineData("abc", "1.0.0", false)]
    [InlineData("1.2.2", "abc", false)]
    [InlineData("", "1.0.0", false)]
    public void IsNewer_ComparesSemver(string candidate, string current, bool expected)
    {
        Assert.Equal(expected, UpdateManifest.IsNewer(candidate, current));
    }
}
