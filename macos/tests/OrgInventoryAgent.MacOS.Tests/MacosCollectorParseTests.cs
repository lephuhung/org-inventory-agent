using System.Text.Json;
using OrgInventoryAgent.Core.Collectors.Schema;
using OrgInventoryAgent.MacOS.Collectors;
using Xunit;

namespace OrgInventoryAgent.MacOS.Tests;

/// <summary>
/// Unit test cho các hàm PARSE của macOS collectors (chạy được trên mọi OS —
/// không gọi sysctl/system_profiler thật, feed canned output).
/// </summary>
public class MacosCollectorParseTests
{
    [Fact]
    public void ParseSwVers_ExtractsNameVersionBuild()
    {
        var output = "ProductName:\tmacOS\nProductVersion:\t14.5\nBuildVersion:\t23F79\n";
        var (name, version, build) = MacosOsCollector.ParseSwVers(output);
        Assert.Equal("macOS", name);
        Assert.Equal("14.5", version);
        Assert.Equal("23F79", build);
    }

    [Fact]
    public void ParseSwVers_NullInput_ReturnsAllNull()
    {
        var (name, version, build) = MacosOsCollector.ParseSwVers(null);
        Assert.Null(name);
        Assert.Null(version);
        Assert.Null(build);
    }

    [Fact]
    public void ParseIoPlatformUuid_ExtractsQuotedUuid()
    {
        var ioreg = """
            +-o IOPlatformExpertDevice  <class IOPlatformExpertDevice>
                {
                  "IOPlatformUUID" = "A1B2C3D4-1234-5678-9ABC-DEF012345678"
                  "serial-number" = <"C02ABCDEFGH">
                }
            """;
        Assert.Equal("A1B2C3D4-1234-5678-9ABC-DEF012345678",
            MacosFingerprintCollector.ParseIoPlatformUuid(ioreg));
    }

    [Fact]
    public void ParseIoPlatformUuid_MissingKey_ReturnsNull()
    {
        Assert.Null(MacosFingerprintCollector.ParseIoPlatformUuid("+-o Root"));
        Assert.Null(MacosFingerprintCollector.ParseIoPlatformUuid(null));
    }

    [Fact]
    public void ParseStorageDataType_GroupsByPhysicalDrive()
    {
        var json = """
            {"SPStorageDataType":[
              {"_name":"Macintosh HD","bsd_name":"disk3s1s1","file_system":"apfs",
               "size_in_bytes":994662584320,
               "physical_drive":{"device_name":"APPLE SSD AP1024Z","is_internal_disk":"yes","medium_type":"ssd"}},
              {"_name":"Data","bsd_name":"disk3s5","file_system":"apfs",
               "size_in_bytes":994662584320,
               "physical_drive":{"device_name":"APPLE SSD AP1024Z","is_internal_disk":"yes","medium_type":"ssd"}},
              {"_name":"NoDriveVol","bsd_name":"disk9s1"}
            ]}
            """;
        var disks = MacosOsCollector.ParseStorageDataType(JsonDocument.Parse(json).RootElement);
        Assert.NotNull(disks);
        var disk = Assert.Single(disks!);
        Assert.Equal("APPLE SSD AP1024Z", disk.Model);
        Assert.Equal("SSD", disk.Type);
        Assert.Equal(994662584320, disk.SizeBytes);
    }

    [Fact]
    public void ParseStorageDataType_NoPhysicalDrives_ReturnsNull()
    {
        var json = """{"SPStorageDataType":[{"_name":"vol"}]}""";
        Assert.Null(MacosOsCollector.ParseStorageDataType(JsonDocument.Parse(json).RootElement));
    }

    [Fact]
    public void ParseApplicationsDataType_MapsFields()
    {
        var json = """
            {"SPApplicationsDataType":[
              {"_name":"Safari","path":"/Applications/Safari.app","version":"17.5","obtained_from":"apple"},
              {"_name":"CustomApp","path":"/Users/me/Applications/CustomApp.app","version":"1.2","obtained_from":"identified_developer"}
            ]}
            """;
        var apps = MacosSoftwareCollector.ParseApplicationsDataType(JsonDocument.Parse(json).RootElement);
        Assert.Equal(2, apps.Count);
        Assert.Equal("Safari", apps[0].DisplayName);
        Assert.Equal("17.5", apps[0].Version);
        Assert.Equal("Apple", apps[0].Publisher);
        Assert.False(apps[0].IsPerUser);
        Assert.True(apps[1].IsPerUser);
        Assert.Equal("Identified Developer", apps[1].Publisher);
    }

    [Fact]
    public void ParseSoftwareUpdate_UpToDate()
    {
        var output = "Software Update Tool\n\nFinding available software\nNo new software available.\n";
        var st = MacosUpdateCollector.Parse(output);
        Assert.Equal("up-to-date", st.Status);
        Assert.Equal(0, st.PendingCount);
    }

    [Fact]
    public void ParseSoftwareUpdate_CountsLabels()
    {
        var output = """
            Software Update Tool

            Finding available software
            Software Update found the following new or updated software:
            * Label: macOS Sonoma 14.6-23G80
            * Label: Safari17.6
            """;
        var st = MacosUpdateCollector.Parse(output);
        Assert.Equal("updates-available", st.Status);
        Assert.Equal(2, st.PendingCount);
    }

    [Fact]
    public void ParseSoftwareUpdate_RealFormat_RecommendedAndRestart()
    {
        // Format thật của `softwareupdate -l` (macOS 15/26): "Recommended: YES"
        // không quote, update cần reboot có "Action: restart".
        var output = """
            Software Update Tool

            Finding available software
            Software Update found the following new or updated software:
            * Label: macOS 26.6.3-25G212
                    Title: macOS 26.6.3, Version: 26.6.3, Size: 3145728K, Recommended: YES, Action: restart,
            * Label: Safari26.0.1
                    Title: Safari, Version: 26.0.1, Size: 102400K, Recommended: YES,
            """;
        var st = MacosUpdateCollector.Parse(output);
        Assert.Equal("updates-available", st.Status);
        Assert.Equal(2, st.PendingCount);
        Assert.Equal(2, st.SecurityPendingCount);
        Assert.True(st.RebootRequired);
    }

    [Fact]
    public void ParseUsersUniqueId_ParsesNameUid()
    {
        var output = "daemon        1\nadmin         501\n_guest        201\n";
        var list = MacosAccountCollector.ParseUsersUniqueId(output);
        Assert.Equal(3, list.Count);
        Assert.Contains(("admin", 501), list);
    }

    [Fact]
    public void ParseGroupMembership_ExtractsMembers()
    {
        var output = "dsAttrTypeStandard:GroupMembership: admin root operator\n";
        var admins = MacosAccountCollector.ParseGroupMembership(output);
        Assert.Contains("admin", admins);
        Assert.Contains("root", admins);
        Assert.Equal(3, admins.Count);
    }
}
