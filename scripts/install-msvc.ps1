<#
.SYNOPSIS
    安装 MSVC 生成工具（Visual Studio Build Tools）到非系统盘，供 Rust / Tauri 链接使用。

.DESCRIPTION
    背景：这台机器没装过 VS / MSVC / Windows SDK；rustc 的 host 是 x86_64-pc-windows-gnu，
    但项目 .cargo/config.toml 固定 target = x86_64-pc-windows-msvc，
    所以 cargo test / cargo build 会报 "linker link.exe not found"。

    目录约定（VS 安装器要求 --installPath 必须是空目录或不存在的目录）：
      InstallPath  工具链本体（安装器创建；脚本只在安装前确认它是空的）
      WorkDir      引导器 / 临时文件 / 安装日志（默认与 InstallPath 同级）

    必须在管理员 PowerShell 里运行。

.PARAMETER InstallPath
    工具链安装目录，默认 D:\VSBuildTools。换盘只改这个参数。

.PARAMETER WorkDir
    下载与日志目录，默认 <InstallPath 的父目录>\VSBuildTools-setup。

.PARAMETER SkipInstall
    只做自检，不安装（用于安装后复核）。

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\scripts\install-msvc.ps1
.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\scripts\install-msvc.ps1 -InstallPath E:\VSBuildTools
#>
[CmdletBinding()]
param(
    [string] $InstallPath = "D:\VSBuildTools",
    [string] $WorkDir = "",
    [switch] $SkipInstall
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($WorkDir)) {
    $WorkDir = Join-Path (Split-Path $InstallPath -Parent) "VSBuildTools-setup"
}
$LogFile = Join-Path $WorkDir "install.log"
$BootDir = Join-Path $WorkDir "bootstrapper"
$TmpDir  = Join-Path $WorkDir "tmp"

function Write-Log {
    param([string] $Message)
    $line = "{0}  {1}" -f (Get-Date -Format "HH:mm:ss"), $Message
    Write-Host $line
    try { Add-Content -LiteralPath $LogFile -Value $line -Encoding UTF8 } catch { }
}

# ── 0. 前置检查 ────────────────────────────────────────────────
$principal = [Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host "需要管理员权限。请用管理员 PowerShell 执行：" -ForegroundColor Yellow
    Write-Host ("  powershell -ExecutionPolicy Bypass -File " + $PSCommandPath + " -InstallPath " + $InstallPath) -ForegroundColor Yellow
    exit 1
}

New-Item -ItemType Directory -Force -Path $WorkDir, $BootDir, $TmpDir | Out-Null
$env:TEMP = $TmpDir
$env:TMP  = $TmpDir
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

Write-Log "=== Teas Launcher: MSVC 安装开始 ==="
Write-Log "工具链目录: $InstallPath"
Write-Log "工作目录  : $WorkDir"

function Find-LinkExe {
    param([string] $Root)
    if (-not (Test-Path $Root)) { return $null }
    Get-ChildItem -Path $Root -Filter "link.exe" -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match "Hostx64.x64" } |
        Select-Object -First 1 -ExpandProperty FullName
}

# ── 1. 已装则跳过 ──────────────────────────────────────────────
$existing = Find-LinkExe (Join-Path $InstallPath "VC")
if ($existing) {
    Write-Log "已存在 link.exe: $existing（跳过安装）"
    $SkipInstall = $true
}

# 该路径下已有 VS 实例时走 modify（补装组件），目录必然非空
$hasInstance = Test-Path (Join-Path $InstallPath "Common7\Tools\LaunchDevCmd.bat")
if ($hasInstance) { Write-Log "检测到已有实例，将以 modify 模式补装缺少的组件" }

if (-not $SkipInstall) {
    # ── 2. 全新安装时目录必须为空或不存在 ─────────────────────
    if ((-not $hasInstance) -and (Test-Path $InstallPath)) {
        $items = Get-ChildItem -LiteralPath $InstallPath -Force -ErrorAction SilentlyContinue
        if ($items) {
            $names = @($items | ForEach-Object { $_.Name })
            $known = @("bootstrapper", ".tmp", "install.log")
            $foreign = @($names | Where-Object { $known -notcontains $_ })
            if ($foreign.Count -gt 0) {
                Write-Log ("目标目录非空，且含有非本脚本产生的文件: " + ($foreign -join ", "))
                Write-Log "请先清空 $InstallPath，或用 -InstallPath 换一个目录"
                exit 4
            }
            Write-Log "清理上次失败尝试留下的目录（只含 bootstrapper/.tmp/install.log）"
            Remove-Item -LiteralPath $InstallPath -Recurse -Force
        }
    }

    # ── 3. 下载引导器 ──────────────────────────────────────────
    $bootstrapper = Join-Path $BootDir "vs_BuildTools.exe"
    if (Test-Path $bootstrapper) {
        Write-Log "复用已下载的引导器: $bootstrapper"
    } else {
        $url = "https://aka.ms/vs/17/release/vs_BuildTools.exe"
        Write-Log "下载引导器: $url"
        $ok = $false
        try {
            Invoke-WebRequest -Uri $url -OutFile $bootstrapper -UseBasicParsing -TimeoutSec 180
            $ok = $true
        } catch {
            Write-Log "Invoke-WebRequest 失败: $($_.Exception.Message)，改用 curl.exe"
            & curl.exe -L --fail --retry 3 --connect-timeout 30 -o $bootstrapper $url
            $ok = ($LASTEXITCODE -eq 0)
        }
        if (-not $ok -or -not (Test-Path $bootstrapper)) {
            Write-Log "下载失败。可手动从 https://aka.ms/vs/17/release/vs_BuildTools.exe 下载后放到 $BootDir"
            exit 2
        }
        Write-Log ("引导器大小: {0:N1} MB" -f ((Get-Item $bootstrapper).Length / 1MB))
    }

    # ── 4. 静默安装 ────────────────────────────────────────────
    # --nocache : 不保留下载缓存（否则 C:\ProgramData 还会留 1GB+）
    # --wait    : 等安装结束再返回，便于拿退出码
    # 注意：VCTools 工作负载的**默认**组件不含 MSVC 编译器（VC.Tools.x86.x64 属于
    # includeRecommended）。只加工作负载会装出一个没有 cl.exe/link.exe 的空壳。
    $installerArgs = @(
        "--quiet", "--wait", "--norestart", "--nocache",
        "--installPath", $InstallPath,
        "--add", "Microsoft.VisualStudio.Workload.VCTools",
        "--add", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
        "--includeRecommended"
    )
    Write-Log ("执行: " + $bootstrapper + " " + ($installerArgs -join " "))
    $proc = Start-Process -FilePath $bootstrapper -ArgumentList $installerArgs -Wait -PassThru
    Write-Log "安装器退出码: $($proc.ExitCode)"

    switch ($proc.ExitCode) {
        0    { Write-Log "安装成功" }
        3010 { Write-Log "安装成功（需要重启才能完全生效）" }
        1602 { Write-Log "安装被取消"; exit 3 }
        5007 { Write-Log "安装被阻止（常见原因：有 VS 安装器正在运行）"; exit 3 }
        default {
            Write-Log "安装失败，退出码 $($proc.ExitCode)"
            Write-Log ("详细日志在 " + $TmpDir + "（dd_installer_*.log / dd_setup_*.log）")
            exit 3
        }
    }
}

# ── 5. 自检 ───────────────────────────────────────────────────
Write-Log "=== 自检 ==="
$link = Find-LinkExe (Join-Path $InstallPath "VC")
if ($link) { Write-Log "link.exe : $link" } else { Write-Log "未找到 link.exe" }

$vswhere = "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"
if (Test-Path $vswhere) {
    $inst = & $vswhere -products * -property installationPath 2>$null
    Write-Log "vswhere  : $inst"
}

$sdkCandidates = @(
    (Join-Path $InstallPath "Windows Kits"),
    (Join-Path $InstallPath "SDK"),
    "C:\Program Files (x86)\Windows Kits"
)
foreach ($p in $sdkCandidates) {
    if (Test-Path $p) { Write-Log "Windows SDK: $p" }
}

Write-Log "=== 各目录占用 ==="
$sizePaths = @(
    $InstallPath,
    "C:\Program Files (x86)\Microsoft Visual Studio\Installer",
    "C:\Program Files (x86)\Windows Kits",
    "C:\ProgramData\Microsoft\VisualStudio\Packages"
)
foreach ($p in $sizePaths) {
    if (Test-Path $p) {
        $sum = (Get-ChildItem -LiteralPath $p -Recurse -Force -ErrorAction SilentlyContinue |
                Measure-Object -Property Length -Sum).Sum
        Write-Log ("{0,-64} {1,8:N0} MB" -f $p, ($sum / 1MB))
    } else {
        Write-Log ("{0,-64} {1,8}" -f $p, "(不存在)")
    }
}

Write-Log "=== 完成 ==="
Write-Log "下一步在项目目录执行: cargo test --lib"
Write-Log "日志: $LogFile"
