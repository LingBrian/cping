# ===========================================================================
# CPing Windows 打包脚本（M11）
#
# 用法：
#   powershell -ExecutionPolicy Bypass -File scripts\package.ps1 -Version 2.0.0 -Tag windows-x64
#   powershell -ExecutionPolicy Bypass -File scripts\package.ps1 -Version 2.0.0 -Tag windows-x86 -Bin cping-tui,cping-cli
#
# 产出（默认写入 release\<version>\）：
#   <bin>-<version>-<tag>.zip      随包附 README.txt
#   SHA256SUMS.txt                 覆盖本次产出的全部压缩包
#
# 设计约束：
#   - 只打包实际存在的二进制；缺哪个就报缺哪个，并以非 0 退出。
#   - 校验和格式与 Linux 侧一致（`*文件名`），可被 `certutil` / `sha256sum` 校验。
# ===========================================================================
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$Tag,
    [string[]]$Bin = @('cping-tui', 'cping-cli'),
    [string]$TargetDir,
    [string]$OutDir
)

$ErrorActionPreference = 'Stop'

$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (-not $TargetDir) { $TargetDir = Join-Path $repo 'target\release' }
if (-not $OutDir) { $OutDir = Join-Path $repo "release\$Version" }

if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }

$stageRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("cping-pack-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stageRoot -Force | Out-Null

Write-Host '==== CPing 打包 ===='
Write-Host "  版本       : $Version"
Write-Host "  平台标签   : $Tag"
Write-Host "  二进制目录 : $TargetDir"
Write-Host "  输出目录   : $OutDir"
Write-Host ''

$missing = @()
$resolved = @()
foreach ($name in $Bin) {
    $exe = Join-Path $TargetDir "$name.exe"
    if (Test-Path $exe) { $resolved += $name } else { $missing += $exe }
}

if ($missing.Count -gt 0) {
    foreach ($m in $missing) { Write-Host "[缺失] $m" -ForegroundColor Red }
    Write-Host ''
    Write-Host '先构建：cargo build --release -p cping-tui -p cping-cli' -ForegroundColor Yellow
    Remove-Item -Recurse -Force $stageRoot
    exit 1
}

# 打包时把版本、平台与用法写进包内 README.txt，离线拿到压缩包也能直接上手。
function Write-PackageReadme {
    param([string]$BinName, [string]$Dest)
    $text = @"
CPing $Version（$Tag）
========================================

本包内含二进制：$BinName.exe

运行
----
  $BinName.exe --help

退出码（cping-cli 无头模式）
----------------------------
  0  全部目标在线
  1  存在离线/超时目标
  2  参数错误
  3  权限不足或后端不可用

权限说明
--------
ICMP 探测需要原始套接字权限。Windows 上普通用户可发 ICMP Echo（系统允许无特权
ICMPv4），但读取本机邻居表（ARP）用于 MAC/厂商补全需要管理员权限；拿不到权限时
程序会如实提示，不会伪造数据。

无权限或目标不响应 ICMP 时，可改用 TCP 方式：
  $BinName.exe --mode tcp --port 80 --target 192.168.1

校验
----
  Get-FileHash .\$BinName-$Version-$Tag.zip -Algorithm SHA256
  或与 SHA256SUMS.txt 中对应行比对
"@
    Set-Content -Path (Join-Path $Dest 'README.txt') -Value $text -Encoding UTF8
}

$produced = @()
foreach ($name in $resolved) {
    $stage = Join-Path $stageRoot "$name-$Version-$Tag"
    New-Item -ItemType Directory -Path $stage -Force | Out-Null
    Copy-Item (Join-Path $TargetDir "$name.exe") (Join-Path $stage "$name.exe")
    Write-PackageReadme -BinName $name -Dest $stage

    $zip = Join-Path $OutDir "$name-$Version-$Tag.zip"
    # 覆盖旧包用 .NET 的 File.Delete，而不是 Remove-Item 或 Compress-Archive -Force：
    # 后两者在受限环境里都会走回收站，被策略拦下时整轮打包会直接中断。
    if ([System.IO.File]::Exists($zip)) { [System.IO.File]::Delete($zip) }
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -CompressionLevel Optimal
    $sizeMb = [Math]::Round((Get-Item $zip).Length / 1MB, 2)
    Write-Host "[产出] $zip  ($sizeMb MB)"
    $produced += (Split-Path $zip -Leaf)
}

# 临时目录清理失败不该让已经产出的包作废，因此不抛错。
Remove-Item -Recurse -Force $stageRoot -ErrorAction SilentlyContinue

# 校验和清单：在输出目录内计算，写相对文件名，便于整目录拷走后原地校验。
$sumsPath = Join-Path $OutDir 'SHA256SUMS.txt'
$lines = @()
foreach ($file in $produced) {
    $hash = (Get-FileHash -Algorithm SHA256 (Join-Path $OutDir $file)).Hash.ToLower()
    $lines += "$hash *$file"
}
Set-Content -Path $sumsPath -Value ($lines -join "`r`n") -Encoding ASCII

Write-Host ''
Write-Host "==== 校验和 ($sumsPath) ===="
$lines | ForEach-Object { Write-Host $_ }
Write-Host ''
Write-Host "自检：cd `"$OutDir`" && Get-Content SHA256SUMS.txt"
