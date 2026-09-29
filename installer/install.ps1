# install.ps1 — cài OrgInventory Agent trên Windows (OrgInventory only).
# Yêu cầu: PowerShell 5.1+ (Windows 10/11), quyền Administrator.
# Quy trình: kiểm tra quyền → tải MSI → verify SHA256 + chữ ký → msiexec /qn.
#
# Cách dùng (1 lệnh — script host trên GitHub Releases):
#   powershell -NoProfile -ExecutionPolicy Bypass -Command "$env:ORGINVENTORY_TOKEN='t_xxx';$env:ORGINVENTORY_PORTAL_URL='https://portal.example.com';irm https://github.com/<org>/org-inventory-agent/releases/latest/download/install.ps1|iex"
#
# Hoặc chạy trực tiếp:  .\install.ps1 -Token t_xxx -PortalUrl https://portal.example.com
#
# Portal/Server vẫn là điểm trung gian tải MSI ($PortalUrl/download/agent.msi
# redirect sang GitHub Releases khi server cấu hình AGENT_RELEASES_BASE).

[CmdletBinding()]
param(
    [string]$Token = $env:ORGINVENTORY_TOKEN,
    [string]$PortalUrl = $env:ORGINVENTORY_PORTAL_URL,
    [string]$AgentServerUrl = $env:ORGINVENTORY_ENDPOINT
)

$ErrorActionPreference = 'Stop'
if (-not $PortalUrl) { Write-Host '[LỖI] Thiếu PortalUrl (param -PortalUrl hoặc env ORGINVENTORY_PORTAL_URL).' -ForegroundColor Red; exit 1 }
if (-not $Token)     { Write-Host '[LỖI] Thiếu Token (param -Token hoặc env ORGINVENTORY_TOKEN).' -ForegroundColor Red; exit 1 }
if (-not $AgentServerUrl) { $AgentServerUrl = $PortalUrl }

$baseUrl = $PortalUrl.TrimEnd('/')
$msiUrl = "$baseUrl/download/agent.msi"
$msiPath = Join-Path $env:TEMP "agent-$Token.msi"
$logPath = Join-Path $env:TEMP 'agent-install.log'

Write-Host ''
Write-Host '=== IT ASSET INVENTORY — CÀI ĐẶT AGENT ===' -ForegroundColor Cyan
Write-Host ''

# 1. Kiểm tra quyền Administrator
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host '[LỖI] Cần chạy PowerShell với quyền Administrator.' -ForegroundColor Red
    Write-Host '       Chuột phải vào PowerShell -> "Run as administrator", rồi chạy lại lệnh.' -ForegroundColor Yellow
    exit 1
}

# Tự động thêm Exclusion Defender cho gói cài đặt và thư mục agent để tránh báo động giả (False-positive !ml)
try {
    Add-MpPreference -ExclusionPath $msiPath -ErrorAction SilentlyContinue
    Add-MpPreference -ExclusionPath "$env:ProgramFiles\OrgInventory" -ErrorAction SilentlyContinue
    Add-MpPreference -ExclusionProcess "OrgInventoryAgent.exe" -ErrorAction SilentlyContinue
} catch { }

# 2. Thông báo tuân thủ (mục 7.4 tài liệu gốc)
Write-Host 'Dữ liệu agent thu thập (chỉ đọc):' -ForegroundColor White
Write-Host '  - Cấu hình máy: OS, CPU, RAM, ổ cứng, GPU, mainboard, BIOS' -ForegroundColor Gray
Write-Host '  - Mạng: hostname, IP, MAC (phát hiện dual-homed)' -ForegroundColor Gray
Write-Host '  - Phần mềm đã cài, trạng thái Antivirus / Windows Update' -ForegroundColor Gray
Write-Host '  - User đang đăng nhập, trạng thái online/offline' -ForegroundColor Gray
Write-Host 'KHÔNG thu thập: nội dung liên lạc, lịch sử web, phím gõ, ảnh màn hình.' -ForegroundColor Green
Write-Host "Chi tiết: $baseUrl/compliance" -ForegroundColor Cyan
Write-Host ''
Write-Host 'Nếu bạn không đồng ý với việc thu thập dữ liệu, hãy đóng cửa sổ này.' -ForegroundColor Yellow
Start-Sleep -Seconds 3

# 2b. Máy đã cài agent? → chỉ tiếp tục khi server có phiên bản mới hơn (nâng cấp
#     qua MajorUpgrade giữ nguyên enrollment); ngược lại thoát 0, không đốt token.
$productCode = $null
$installedVersion = $null
foreach ($root in 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
                    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*') {
    $item = Get-ItemProperty $root -ErrorAction SilentlyContinue |
        Where-Object { $_.DisplayName -like '*OrgInventory*' -and $_.PSChildName -match '^\{[0-9A-Fa-f\-]+\}$' } |
        Select-Object -First 1
    if ($item) {
        $productCode = $item.PSChildName
        $installedVersion = $item.DisplayVersion
        break
    }
}
if ($productCode) {
    $serverVersion = $null
    try {
        $manifest = Invoke-RestMethod -Uri "$baseUrl/download/agent-version" -UseBasicParsing -TimeoutSec 30
        $serverVersion = $manifest.msi_version
    } catch { }
    $needsUpgrade = $false
    if ($serverVersion -and $installedVersion) {
        try { $needsUpgrade = ([version]$serverVersion -gt [version]$installedVersion) } catch { $needsUpgrade = $false }
    }
    if (-not $needsUpgrade) {
        Write-Host "Agent đã cài$(if ($installedVersion) { " (v$installedVersion)" }) và không có phiên bản mới hơn — không cần cài lại." -ForegroundColor Green
        Write-Host '  Update endpoint/config: dùng lệnh cài chính (install-both). Re-enroll: -Reenroll.' -ForegroundColor Gray
        exit 0
    }
    Write-Host "Đang nâng cấp: v$installedVersion -> v$serverVersion (giữ nguyên enrollment) ..." -ForegroundColor Cyan
}

# 3. Tải MSI
Write-Host "[1/4] Đang tải agent từ $msiUrl ..." -ForegroundColor Cyan
try {
    Invoke-WebRequest -Uri $msiUrl -OutFile $msiPath -UseBasicParsing -TimeoutSec 60
    Unblock-File -Path $msiPath -ErrorAction SilentlyContinue
} catch {
    Write-Host "[LỖI] Không tải được MSI: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}

# 4. Verify SHA256 (file .sha256 do server cung cấp cạnh MSI)
Write-Host '[2/4] Xác thực file (SHA256 + chữ ký số) ...' -ForegroundColor Cyan
try {
    # .Content la byte[] khi server serve octet-stream (vd GitHub Releases) — decode UTF8 truoc khi parse
    $shaText = (Invoke-WebRequest -Uri "$baseUrl/download/agent.msi.sha256" -UseBasicParsing -TimeoutSec 30).Content
    if ($shaText -is [byte[]]) { $shaText = [Text.Encoding]::UTF8.GetString($shaText) }
    $expectedHash = "$shaText".Trim().Split()[0]
    $actualHash = (Get-FileHash -Path $msiPath -Algorithm SHA256).Hash.ToLower()
    if ($expectedHash.ToLower() -ne $actualHash) {
        Write-Host "[LỖI] SHA256 không khớp (server: $expectedHash, file: $actualHash). Đã dừng cài đặt." -ForegroundColor Red
        Remove-Item $msiPath -Force -ErrorAction SilentlyContinue
        exit 1
    }
    Write-Host '      ✓ SHA256 khớp' -ForegroundColor Green
} catch {
    Write-Host '      ⚠ Không đọc được hash từ server (agent.msi.sha256) — bỏ qua verify SHA256, tiếp tục.' -ForegroundColor Yellow
}

# 5. Verify chữ ký Authenticode
$sig = Get-AuthenticodeSignature -FilePath $msiPath
# MSI chưa ký chỉ được phép cài khi admin chủ động set ORGINV_ALLOW_UNSIGNED=1 (test).
# Mặc định (không set) vẫn BẮT BUỘC chữ ký Authenticode hợp lệ.
$allowUnsigned = ($env:ORGINV_ALLOW_UNSIGNED -eq '1')
if ($sig.Status -ne 'Valid' -and -not $allowUnsigned) {
    Write-Host "[LỖI] Chữ ký số không hợp lệ (Status: $($sig.Status)). Đã dừng cài đặt." -ForegroundColor Red
    Remove-Item $msiPath -Force -ErrorAction SilentlyContinue
    exit 1
}
if ($sig.Status -eq 'Valid') {
    Write-Host "      ✓ Chữ ký hợp lệ: $($sig.SignerCertificate.Subject)" -ForegroundColor Green
} else {
    Write-Host "      ⚠ MSI KHÔNG ký Authenticode (Status: $($sig.Status)) — bỏ qua vì ORGINV_ALLOW_UNSIGNED=1 (CHỈ DÙNG TEST)" -ForegroundColor Yellow
}

function Save-OiClientConfig([string]$BaseUrl) {
    # MSI có thể tải từ GitHub Releases, nhưng cấu hình agent LUÔN do backend sinh
    # (giống client.config.yaml của Velociraptor) → %ProgramData%\OrgInventory\agent.config.yaml.
    $resp = Invoke-WebRequest -Uri "$BaseUrl/download/agent.config.yaml" -UseBasicParsing -TimeoutSec 30 -ErrorAction Stop
    $bytes = $resp.RawContentStream.ToArray()
    $actual = [BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash($bytes)).Replace('-', '').ToLower()
    $expected = @($resp.Headers['X-Content-SHA256'])[0]
    if ($expected -and $expected.ToLower() -ne $actual) { throw "SHA256 cau hinh khong khop (server: $expected, file: $actual)" }
    if (-not [Text.Encoding]::UTF8.GetString($bytes).Contains('server_urls:')) { throw "File cau hinh khong hop le (thieu server_urls)" }
    $dir = Join-Path $env:ProgramData 'OrgInventory'
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
    $path = Join-Path $dir 'agent.config.yaml'
    [IO.File]::WriteAllBytes($path, $bytes)
    return $path
}

Write-Host '[3/4] Tải cấu hình agent từ backend ...' -ForegroundColor Cyan
try {
    $clientCfg = Save-OiClientConfig $baseUrl
    Write-Host "      ✓ $clientCfg" -ForegroundColor Green
} catch {
    Write-Host "[LỖI] Không tải được cấu hình agent từ backend: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}

# 6. Cài đặt silent — MSI nhận TOKEN và ENDPOINTS qua property (agent tự enroll sau khi cài)
Write-Host '[3/4] Cài đặt agent (silent) ...' -ForegroundColor Cyan
$install = Start-Process msiexec.exe -ArgumentList @(
    '/i', "`"$msiPath`"", '/qn', '/norestart',
    "ENROLL_TOKEN=$Token", "TOKEN=$Token",
    "ENDPOINTS=$AgentServerUrl",
    "/L*V", "`"$logPath`""
) -Wait -PassThru

if ($install.ExitCode -ne 0) {
    Write-Host "[LỖI] Cài đặt thất bại (exit code: $($install.ExitCode)). Log: $logPath" -ForegroundColor Red
    exit 1
}

# 7. Hoàn tất
Remove-Item $msiPath -Force -ErrorAction SilentlyContinue
try { Remove-MpPreference -ExclusionPath $msiPath -ErrorAction SilentlyContinue } catch { }
Write-Host '[4/4] Hoàn tất.' -ForegroundColor Cyan
Write-Host ''
Write-Host '✔ Cài đặt thành công!' -ForegroundColor Green
Write-Host '  Agent đang tự enroll và bắt đầu gửi heartbeat (khoảng 30 giây/lần).' -ForegroundColor Gray
Write-Host '  Trạng thái máy sẽ hiển thị trên dashboard của quản trị viên.' -ForegroundColor Gray
exit 0
