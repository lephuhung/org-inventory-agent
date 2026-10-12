using Microsoft.Extensions.Logging;
using OrgInventoryAgent.Core.Collectors;
using OrgInventoryAgent.Core.Collectors.Schema;

namespace OrgInventoryAgent.MacOS.Collectors;

/// <summary>macOS-side façade for SecurityPostureV4. Aggregates all macOS security collectors.</summary>
public static class MacosSecurityCollector
{
    public static SecurityPostureV4 Collect(ILogger logger) => new()
    {
        Update = MacosUpdateCollector.Collect(logger),
        DiskEncryption = MacosDiskEncryptionCollector.Collect(logger),
        RemoteAccess = MacosRemoteAccessCollector.Collect(logger),
        EndpointProtection = MacosEndpointProtectionCollector.Collect(logger),
        PrivilegeControl = MacosPrivilegeControlCollector.Collect(logger),
        ListeningPorts = PosixPortCollector.Collect(logger),
        FirewallEnabled = MacosFirewallCollector.Collect(logger),
        LocalAccounts = MacosAccountCollector.Collect(logger),
        StartupPrograms = MacosStartupCollector.Collect(logger),
    };
}

/// <summary>Bản cập nhật macOS — `softwareupdate -l` (có thể mất tới ~60s).</summary>
public static class MacosUpdateCollector
{
    public static UpdateStatus Collect(ILogger logger)
    {
        var output = MacosCmd.Run("/usr/sbin/softwareupdate", "-l", timeoutMs: 90_000);
        if (output is null) return new UpdateStatus { Status = "unknown" };
        return Parse(output);
    }

    /// <summary>Parse `softwareupdate -l` output (testable).</summary>
    public static UpdateStatus Parse(string output)
    {
        if (output.Contains("No new software available", StringComparison.OrdinalIgnoreCase))
            return new UpdateStatus { Status = "up-to-date", PendingCount = 0, SecurityPendingCount = 0 };

        var pending = output.Split('\n')
            .Count(l => l.TrimStart().StartsWith("* Label:", StringComparison.Ordinal));
        var sec = output.Split('\n')
            .Count(l => l.Contains("recommended=\"YES\"", StringComparison.OrdinalIgnoreCase));
        return new UpdateStatus
        {
            Status = pending > 0 ? "updates-available" : "up-to-date",
            PendingCount = pending,
            SecurityPendingCount = sec > 0 ? sec : null,
            RebootRequired = output.Contains("[restart]", StringComparison.OrdinalIgnoreCase)
                          || output.Contains("requires.*restart", StringComparison.OrdinalIgnoreCase) ? true : null,
        };
    }
}

/// <summary>FileVault — `fdesetup status`.</summary>
public static class MacosDiskEncryptionCollector
{
    public static DiskEncryptionStatus Collect(ILogger logger)
    {
        var output = MacosCmd.Run("/usr/bin/fdesetup", "status");
        if (output is null) return new DiskEncryptionStatus { Enabled = null };
        var on = output.Contains("FileVault is On", StringComparison.OrdinalIgnoreCase);
        if (output.Contains("FileVault is Off", StringComparison.OrdinalIgnoreCase) || on)
        {
            return new DiskEncryptionStatus
            {
                Enabled = on,
                Technology = "filevault",
                EncryptedVolumes = on ? new List<string> { "/" } : null,
            };
        }
        return new DiskEncryptionStatus { Enabled = null };
    }
}

/// <summary>Remote Login (SSH) + Screen Sharing — systemsetup / launchctl.</summary>
public static class MacosRemoteAccessCollector
{
    public static RemoteAccessStatus Collect(ILogger logger)
    {
        var status = new RemoteAccessStatus();
        var services = new List<string>();

        // systemsetup -getremotelogin cần root; không root → thử launchctl list.
        var remoteLogin = MacosCmd.Run("/usr/sbin/systemsetup", "-getremotelogin");
        if (remoteLogin is not null)
        {
            var on = remoteLogin.Contains("Remote Login: On", StringComparison.OrdinalIgnoreCase);
            status.SshEnabled = on;
            if (on) services.Add("ssh (Remote Login)");
        }
        else
        {
            var ssh = MacosCmd.Run("/bin/launchctl", "list com.openssh.sshd");
            if (ssh is not null) { status.SshEnabled = true; services.Add("sshd"); }
        }

        var screensharing = MacosCmd.Run("/bin/launchctl", "print system/com.apple.screensharing");
        if (screensharing is not null && screensharing.Contains("state = running", StringComparison.OrdinalIgnoreCase))
        {
            status.RemoteDesktopEnabled = true;
            services.Add("screensharing");
        }

        status.Services = services.Count > 0 ? services : null;
        return status;
    }
}

/// <summary>Endpoint protection: XProtect (built-in) + các sản phẩm bên thứ 3 phổ biến.</summary>
public static class MacosEndpointProtectionCollector
{
    private static readonly (string Product, string[] ProcessNames)[] KnownProducts =
    {
        ("CrowdStrike Falcon", new[] { "falcond", "com.crowdstrike.falcon.Agent" }),
        ("SentinelOne", new[] { "sentineld", "sentinel-agent" }),
        ("Jamf Protect", new[] { "JamfProtect", "jamfprotect" }),
        ("Microsoft Defender", new[] { "wdavdaemon", "mdatp" }),
        ("osquery", new[] { "osqueryd" }),
    };

    public static List<AntivirusInfo> Collect(ILogger logger)
    {
        var found = new List<AntivirusInfo>
        {
            // XProtect là anti-malware tích hợp sẵn của macOS — luôn active.
            new()
            {
                DisplayName = "XProtect (built-in)",
                Name = "XProtect",
                Enabled = true,
                Status = "enabled",
                UpToDate = true,
            },
        };

        foreach (var (product, procs) in KnownProducts)
        {
            var output = MacosCmd.Run("/usr/bin/pgrep", "-fl " + string.Join("|", procs), timeoutMs: 3_000);
            if (!string.IsNullOrWhiteSpace(output))
            {
                found.Add(new AntivirusInfo
                {
                    DisplayName = product,
                    Name = product,
                    Enabled = true,
                    Status = "enabled",
                });
            }
        }
        return found;
    }
}

/// <summary>sudo + trạng thái root account (dscl AuthenticationAuthority).</summary>
public static class MacosPrivilegeControlCollector
{
    public static PrivilegeControlStatus Collect(ILogger logger)
    {
        var status = new PrivilegeControlStatus
        {
            SudoInstalled = File.Exists("/usr/bin/sudo"),
        };

        var auth = MacosCmd.Run("/usr/bin/dscl", ". -read /Users/root AuthenticationAuthority");
        if (auth is not null)
        {
            // Root macOS mặc định disabled (không password hash). Marker chính:
            // ;DisabledUser; trong AuthenticationAuthority. Có ShadowHash/SaltedSHA
            // → root đã được enable bằng dsenableroot.
            if (auth.Contains("DisabledUser", StringComparison.OrdinalIgnoreCase))
                status.RootAccountLocked = true;
            else if (auth.Contains("ShadowHash", StringComparison.OrdinalIgnoreCase) ||
                     auth.Contains("SaltedSHA", StringComparison.OrdinalIgnoreCase))
                status.RootAccountLocked = false;
        }
        return status;
    }
}

/// <summary>Application Firewall — socketfilterfw --getglobalstate.</summary>
public static class MacosFirewallCollector
{
    public static bool? Collect(ILogger logger)
    {
        var output = MacosCmd.Run("/usr/libexec/ApplicationFirewall/socketfilterfw", "--getglobalstate");
        if (output is null) return null;
        if (output.Contains("enabled", StringComparison.OrdinalIgnoreCase)) return true;
        if (output.Contains("disabled", StringComparison.OrdinalIgnoreCase)) return false;
        return null;
    }
}

/// <summary>Tài khoản local qua dscl — lọc uid>=500, bỏ account hệ thống (_*).</summary>
public static class MacosAccountCollector
{
    public static List<LocalAccountInfo>? Collect(ILogger logger)
    {
        var users = MacosCmd.Run("/usr/bin/dscl", ". -list /Users UniqueID");
        if (users is null) return null;

        var adminsRaw = MacosCmd.Run("/usr/bin/dscl", ". -read /Groups/admin GroupMembership") ?? "";
        var admins = ParseGroupMembership(adminsRaw);

        var list = new List<LocalAccountInfo>();
        foreach (var (name, uid) in ParseUsersUniqueId(users))
        {
            if (uid < 500 || name.StartsWith('_')) continue;
            var fullName = MacosCmd.Run("/usr/bin/dscl", $". -read /Users/{name} RealName")
                ?.Split(':', 2) is { Length: 2 } p ? p[1].Trim() : null;
            list.Add(new LocalAccountInfo
            {
                Username = name,
                Name = name,
                FullName = string.IsNullOrWhiteSpace(fullName) ? null : fullName,
                Disabled = IsAccountDisabled(name),
                IsAdmin = admins.Contains(name),
            });
            if (list.Count >= 200) break;
        }
        return list.Count > 0 ? list : null;
    }

    /// <summary>Parse `dscl . -list /Users UniqueID` → (name, uid) (testable).</summary>
    public static List<(string Name, int Uid)> ParseUsersUniqueId(string output)
    {
        var list = new List<(string, int)>();
        foreach (var line in output.Split('\n'))
        {
            var parts = line.Split(' ', StringSplitOptions.RemoveEmptyEntries);
            if (parts.Length != 2) continue;
            if (int.TryParse(parts[1], out var uid)) list.Add((parts[0], uid));
        }
        return list;
    }

    /// <summary>Parse `dscl . -read /Groups/admin GroupMembership` → member names (testable).</summary>
    public static HashSet<string> ParseGroupMembership(string output)
    {
        var set = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        // Output: "dsAttrTypeStandard:GroupMembership: admin root operator" — lấy sau ':' cuối.
        var idx = output.LastIndexOf(':');
        if (idx < 0) return set;
        foreach (var name in output[(idx + 1)..].Split(' ', StringSplitOptions.RemoveEmptyEntries))
            set.Add(name.Trim());
        return set;
    }

    private static bool IsAccountDisabled(string user)
    {
        var auth = MacosCmd.Run("/usr/bin/dscl", $". -read /Users/{user} AuthenticationAuthority", timeoutMs: 3_000);
        return auth?.Contains("DisabledUser", StringComparison.OrdinalIgnoreCase) == true;
    }
}

/// <summary>Startup programs = LaunchDaemons + LaunchAgents (3rd-party, bỏ /System).</summary>
public static class MacosStartupCollector
{
    public static List<StartupProgramInfo>? Collect(ILogger logger)
    {
        var list = new List<StartupProgramInfo>();
        try
        {
            foreach (var (dir, location) in new[]
            {
                ("/Library/LaunchDaemons", "LaunchDaemon"),
                ("/Library/LaunchAgents", "LaunchAgent"),
            })
            {
                if (!Directory.Exists(dir)) continue;
                foreach (var plist in Directory.GetFiles(dir, "*.plist"))
                {
                    var name = Path.GetFileNameWithoutExtension(plist);
                    list.Add(new StartupProgramInfo { Name = name, Location = location, Command = plist });
                    if (list.Count >= 200) return list;
                }
            }
        }
        catch { }
        return list.Count > 0 ? list : null;
    }
}
