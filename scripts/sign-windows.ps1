# ===========================================================================
# CPing Windows 制品签名（M17）
#
# 用法：
#   pwsh -File scripts/sign-windows.ps1 -Path <文件或目录> [-Recurse]
#
# 行为：
#   · 提供了证书（CPING_SIGN_CERT_PFX_BASE64 + CPING_SIGN_CERT_PASSWORD）时，
#     用 signtool 做 Authenticode 签名，并用 /pa 走默认策略、
#     时间戳服务器保证签名在证书过期后仍然可验证。
#   · 没提供证书时**不做任何伪装**：只打印每个文件的 SHA-256，并给出
#     取得证书后的确切命令。**绝不用自签证书冒充正式签名**——
#     自签证书在用户机器上照样会弹「未知发布者」，冒充只会让人误判。
#
# 所需密钥（GitHub Secrets）：
#   CPING_SIGN_CERT_PFX_BASE64   PFX 证书的 base64（不要提交证书本身）
#   CPING_SIGN_CERT_PASSWORD     PFX 密码
#   CPING_SIGN_TIMESTAMP_URL     可选，默认 http://timestamp.digicert.com
# ===========================================================================
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Path,
    [switch]$Recurse
)

$ErrorActionPreference = 'Stop'

function Get-TargetFiles {
    param([string]$Root, [bool]$Deep)
    if (Test-Path -LiteralPath $Root -PathType Leaf) {
        return @(Get-Item -LiteralPath $Root)
    }
    $items = if ($Deep) {
        Get-ChildItem -LiteralPath $Root -Recurse -File
    } else {
        Get-ChildItem -LiteralPath $Root -File
    }
    # 只对可执行/包类文件签名；给 .txt 做 Authenticode 没有意义
    return @($items | Where-Object {
            $_.Extension -in @('.exe', '.dll', '.msi', '.ps1', '.cmd', '.bat')
        })
}

$files = Get-TargetFiles -Root $Path -Deep:$Recurse.IsPresent
if ($files.Count -eq 0) {
    Write-Host "没有需要签名的文件（$Path）"
    exit 0
}

$haveCert = -not [string]::IsNullOrEmpty($env:CPING_SIGN_CERT_PFX_BASE64)
$havePass = -not [string]::IsNullOrEmpty($env:CPING_SIGN_CERT_PASSWORD)
$timestampUrl = if ([string]::IsNullOrEmpty($env:CPING_SIGN_TIMESTAMP_URL)) {
    'http://timestamp.digicert.com'
} else {
    $env:CPING_SIGN_TIMESTAMP_URL
}

if ($haveCert -and $havePass) {
    $signtool = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if (-not $signtool) {
        Write-Error '找不到 signtool.exe（需要安装 Windows SDK 的 Signing Tools）'
        exit 1
    }

    # PFX 落到临时目录并尽快删掉；密钥不进仓库、不进构建产物
    $pfx = Join-Path $env:RUNNER_TEMP 'cping-sign.pfx'
    if (-not $env:RUNNER_TEMP) { $pfx = Join-Path ([System.IO.Path]::GetTempPath()) 'cping-sign.pfx' }
    [System.IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:CPING_SIGN_CERT_PFX_BASE64))

    try {
        foreach ($f in $files) {
            Write-Host "签名：$($f.FullName)"
            & $signtool.Source sign /fd SHA256 /f $pfx /p $env:CPING_SIGN_CERT_PASSWORD `
                /tr $timestampUrl /td SHA256 /v $f.FullName
            if ($LASTEXITCODE -ne 0) {
                Write-Error "signtool 失败（退出码 $LASTEXITCODE）：$($f.FullName)"
                exit $LASTEXITCODE
            }
        }
        foreach ($f in $files) {
            $sig = Get-AuthenticodeSignature -LiteralPath $f.FullName
            Write-Host ("{0}  →  {1}" -f $f.Name, $sig.Status)
            if ($sig.Status -ne 'Valid') {
                Write-Error "签名校验未通过：$($f.FullName)（$($sig.Status)）"
                exit 1
            }
        }
        Write-Host '全部文件已签名并通过校验。'
    } finally {
        if (Test-Path -LiteralPath $pfx) { Remove-Item -LiteralPath $pfx -Force }
    }
    exit 0
}

# ---- 没有证书 -----------------------------------------------------------
Write-Host '未提供签名证书（CPING_SIGN_CERT_PFX_BASE64 / CPING_SIGN_CERT_PASSWORD），'
Write-Host '本次产物**未签名**。以下是各文件的 SHA-256，供发布时随附：'
Write-Host ''
foreach ($f in $files) {
    $hash = (Get-FileHash -LiteralPath $f.FullName -Algorithm SHA256).Hash.ToLower()
    Write-Host ("{0}  {1}" -f $hash, $f.Name)
}
Write-Host ''
Write-Host '⚠️ 不要用自签证书冒充正式签名：用户在 Windows 上照样会看到'
Write-Host '   「未知发布者」，只会让人误判发布状态。取得代码签名证书后，'
Write-Host '   把 PFX 的 base64 与密码放进 Secrets，重跑本脚本即可。'
exit 0
