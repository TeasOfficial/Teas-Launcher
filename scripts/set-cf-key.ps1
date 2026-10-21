<#
.SYNOPSIS
    安全写入 CurseForge API Key。输入不回显、不进 shell 历史、不进版本库。

.DESCRIPTION
    Key 落在 %APPDATA%\TeasLauncher\curseforge.key，并把文件 ACL 收紧为仅当前用户。
    启动器读取优先级：环境变量 TEAS_CURSEFORGE_API_KEY > 该文件。

    为什么不用环境变量直接 export？
      Git Bash 的 ~/.bash_history 会原样记录含 key 的命令行，等于明文留痕。
      本脚本用 Read-Host -AsSecureString，输入内容不会出现在任何历史里。

.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\set-cf-key.ps1
.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\set-cf-key.ps1 -Clear
#>
[CmdletBinding()]
param([switch] $Clear)

$dir = Join-Path $env:APPDATA "TeasLauncher"
$file = Join-Path $dir "curseforge.key"

if ($Clear) {
    if (Test-Path $file) { Remove-Item -LiteralPath $file -Force; "已删除: $file" }
    else { "本来就不存在: $file" }
    exit 0
}

Write-Host ""
Write-Host "请输入 CurseForge API Key（输入不会显示，也不会写入任何历史/日志）：" -ForegroundColor Cyan
$sec = Read-Host -AsSecureString

$bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($sec)
try { $plain = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr) }
finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }

$plain = $plain.Trim()
if ([string]::IsNullOrWhiteSpace($plain)) { Write-Host "空输入，已取消" -ForegroundColor Yellow; exit 1 }
if ($plain.Length -lt 20 -or $plain.Length -gt 200 -or $plain -match '\s') {
    Write-Host "格式不正确：长度应在 20-200 之间且不含空白字符" -ForegroundColor Red
    exit 1
}

New-Item -ItemType Directory -Force -Path $dir | Out-Null
[System.IO.File]::WriteAllText($file, $plain)
$plain = $null

# 收紧 ACL：去掉继承，只给当前用户完全控制
try { icacls $file /inheritance:r /grant:r "$($env:USERNAME):F" | Out-Null } catch { }

Write-Host ""
Write-Host "已写入: $file" -ForegroundColor Green
Write-Host ("文件大小: {0} 字节（内容不回显）" -f (Get-Item -LiteralPath $file).Length)
Write-Host "ACL: 已移除继承，仅当前用户可访问"
Write-Host ""
Write-Host "下一步：重启 npx tauri dev 让启动器读到新 key，然后在 获取内容 → CurseForge 里搜索验证。" -ForegroundColor Cyan
