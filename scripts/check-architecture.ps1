#requires -Version 7
<#
.SYNOPSIS
    守住 core / platform 的分层纪律。

.DESCRIPTION
    1. src-tauri/src/core/** 里不允许出现 cfg(target_os) 与原生 crate；
    2. cfg(target_os) 只允许出现在 platform/mod.rs 与 platform/<backend>/** 里；
    3. core/ 不允许直接 use windows / use objc / use gtk 等平台 crate。

    任何一条被破坏就退出码 1，让 CI 红掉。
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$srcRoot = Join-Path $repoRoot 'src-tauri/src'

if (-not (Test-Path $srcRoot)) {
    Write-Error "找不到 Rust 源码目录：$srcRoot"
    exit 1
}

$failures = [System.Collections.Generic.List[string]]::new()

function Get-RelativePath([string] $full) {
    $full.Replace($repoRoot + [System.IO.Path]::DirectorySeparatorChar, '').Replace('\', '/')
}

$rustFiles = Get-ChildItem -Path $srcRoot -Recurse -File -Filter *.rs

# ---- 1. core/ 不得出现 cfg(target_os) ----
$coreFiles = $rustFiles | Where-Object { $_.FullName -match [regex]::Escape("src\core\") }
foreach ($file in $coreFiles) {
    $hits = Select-String -Path $file.FullName -Pattern 'cfg\(\s*(target_os|windows|unix)' -AllMatches
    foreach ($hit in $hits) {
        $failures.Add("$(Get-RelativePath $file.FullName):$($hit.LineNumber) core/ 里出现了平台条件编译：" + $hit.Line.Trim())
    }
}

# ---- 2. core/ 不得直接使用平台 crate ----
$forbidden = '^\s*use\s+(windows|winapi|objc2?|cocoa|core_graphics|gtk|gdk|atspi|wayland_client|libc)\b'
foreach ($file in $coreFiles) {
    $hits = Select-String -Path $file.FullName -Pattern $forbidden
    foreach ($hit in $hits) {
        $failures.Add("$(Get-RelativePath $file.FullName):$($hit.LineNumber) core/ 直接依赖了平台 crate：" + $hit.Line.Trim())
    }
}

# ---- 3. cfg(target_os) 只允许在 platform/mod.rs 与后端内部 ----
$allowedPattern = '^src-tauri/src/platform/'
foreach ($file in $rustFiles) {
    $relative = Get-RelativePath $file.FullName
    if ($relative -match $allowedPattern) { continue }

    $hits = Select-String -Path $file.FullName -Pattern 'cfg\(\s*target_os' -AllMatches
    foreach ($hit in $hits) {
        $failures.Add("${relative}:$($hit.LineNumber) cfg(target_os) 只能出现在 platform/ 下：" + $hit.Line.Trim())
    }
}

# ---- 4. 三个窗口的路由与声明必须成对存在 ----
$confPath = Join-Path $repoRoot 'src-tauri/tauri.conf.json'
if (Test-Path $confPath) {
    $conf = Get-Content $confPath -Raw | ConvertFrom-Json
    foreach ($label in @('shell', 'settings', 'ocr-region')) {
        $declared = $conf.app.windows | Where-Object { $_.label -eq $label }
        if (-not $declared) {
            $failures.Add("src-tauri/tauri.conf.json 缺少窗口声明：$label（见 AGENTS.md 不变式 1）")
        }
    }
}

if ($failures.Count -gt 0) {
    Write-Host ''
    Write-Host "分层纪律检查失败（$($failures.Count) 项）：" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host "  - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "分层纪律检查通过（$($rustFiles.Count) 个 Rust 文件）。" -ForegroundColor Green
exit 0
