using Microsoft.Extensions.Logging;
using OrgInventoryAgent.Core.Collectors.Schema;

namespace OrgInventoryAgent.Core.Collectors;

/// <summary>Đọc TCP listeners qua IPGlobalProperties — cross-platform .NET API (Linux/macOS).</summary>
public static class PosixPortCollector
{
    public static List<ListeningPortInfo>? Collect(ILogger logger)
    {
        try
        {
            var props = System.Net.NetworkInformation.IPGlobalProperties.GetIPGlobalProperties();
            var endpoints = props.GetActiveTcpListeners();
            var list = endpoints
                .Select(ep => new ListeningPortInfo
                {
                    Port = ep.Port,
                    Protocol = "TCP",
                    Address = ep.Address.ToString(),
                })
                .Take(200) // cap
                .ToList();
            return list.Count > 0 ? list : null;
        }
        catch (Exception ex)
        {
            logger.LogDebug("GetActiveTcpListeners lỗi: {Msg}", ex.Message);
            return null;
        }
    }
}
