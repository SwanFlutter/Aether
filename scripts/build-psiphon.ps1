<#
.SYNOPSIS
  ساخت استیج ۲ زنجیره: psiphon-tunnel-core.exe (ConsoleClient).
.DESCRIPTION
  معادل ویندوزیِ psiphontunnel-<ver>.aar در مخزن اندروید.

  چرا از سورس ساخته می‌شود و باینری آماده دانلود نمی‌شود:
    * نسخه به نسخهٔ موبایل پین می‌شود (PSIPHON_VERSION) تا هر دو سکو یک
      کتابخانه را اجرا کنند و یک باگ در یکی، در دیگری غیرقابل‌بازتولید نباشد.
    * زنجیرهٔ تأمین: چیزی که منتشر می‌کنیم از سورسِ تگ‌خوردهٔ بالادست آمده،
      نه از یک آرتیفکت شخص سوم.

  خروجی: dist-engine/psiphon-tunnel-core.exe  و  dist-engine/server_entries.txt
#>
param(
  [Parameter(Mandatory = $true)][string]$Arch,
  [string]$Version = '2.0.39'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$out  = Join-Path $root 'dist-engine'
New-Item -ItemType Directory -Force -Path $out | Out-Null

# GOARCH از معماری بیلد. ویندوز ۳۲بیتی همان 386 است.
$goarch = if ($Arch -eq 'x86') { '386' } else { 'amd64' }

$src = Join-Path $env:RUNNER_TEMP 'psiphon-tunnel-core'
if (-not (Test-Path $src)) {
  Write-Host "==> Cloning psiphon-tunnel-core v$Version"
  git clone --depth 1 --branch "v$Version" https://github.com/Psiphon-Labs/psiphon-tunnel-core.git $src
  if ($LASTEXITCODE -ne 0) { throw "Could not fetch psiphon-tunnel-core v$Version" }
}

Push-Location $src
try {
  $env:GOOS = 'windows'
  $env:GOARCH = $goarch
  # CGO لازم نیست و روی ویندوز فقط بیلد را شکننده می‌کند.
  $env:CGO_ENABLED = '0'
  $target = Join-Path $out 'psiphon-tunnel-core.exe'
  Write-Host "==> Building ConsoleClient (GOOS=windows GOARCH=$goarch)"
  # -s -w: بدون سمبل و DWARF — همان روحیهٔ strip در پروفایل release هستهٔ Rust.
  go build -trimpath -ldflags "-s -w" -o $target ./ConsoleClient
  if ($LASTEXITCODE -ne 0) { throw 'psiphon-tunnel-core build failed' }
  Write-Host "==> stage 2 -> dist-engine/psiphon-tunnel-core.exe"
}
finally { Pop-Location }

# فهرست سرور تعبیه‌شده — همان فایلی که نسخهٔ موبایل در assets دارد.
$entries = Join-Path $root 'assets/psiphon/server_entries.txt'
if (-not (Test-Path $entries)) { throw "Embedded server list is missing: $entries" }
Copy-Item $entries (Join-Path $out 'server_entries.txt') -Force

"$Version" | Out-File -FilePath (Join-Path $out 'PSIPHON_VERSION') -Encoding ascii -NoNewline
Write-Host "==> psiphon stage ready (v$Version, $goarch)"
