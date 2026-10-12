using OrgInventoryAgent.Core.Collectors.Schema;

namespace OrgInventoryAgent.Core.Collectors;

/// <summary>
/// POSIX inventory provider (Linux/macOS): ngoài <see cref="IInventoryProvider.Collect"/>
/// (envelope v4) còn trả flat <see cref="InventorySnapshot"/> (CPU/RAM/disk/network/software)
/// cho InventoryPayloadBuilder ghép payload.
/// </summary>
public interface IPosixInventoryProvider : IInventoryProvider
{
    InventorySnapshot CollectSnapshot();
}
