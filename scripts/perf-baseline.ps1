#requires -Version 7
<#
.SYNOPSIS
    M0 性能基线：冷启动耗时 + 常驻内存（含 WebView2 子进程）。

.DESCRIPTION
    口径说明（报数字时必须一起报）：
      - 冷启动：进程启动 → 托盘就绪，数据来自应用自己写的
        `startup tray-ready Nms` 日志行，不包含安装器/系统预热差异。
      - 内存：应用主进程 + 其 WebView2 子进程的工作集之和。
        这个口径**不含**后续里程碑按需加载的模型权重。
      - 「热键 → HUD 首帧」无法在本脚本里模拟（需要真实按键），
        由界面右上角 / 设置页展示，数据来自 report_first_paint。

.EXAMPLE
    pwsh -File scripts/perf-baseline.ps1
    pwsh -File scripts/perf-baseline.ps1 -Exe src-tauri/target/debug/trans.exe
#>
[CmdletBinding()]
param(
    [string] $Exe,
    [int]    $SettleSeconds = 6,
    [int]    $TimeoutSeconds = 30
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$identifier = 'dev.lunar.trans'

function Resolve-Exe {
    param([string] $candidate)

    if ($candidate) {
        if (-not (Test-Path $candidate)) { throw "找不到可执行文件：$candidate" }
        return (Resolve-Path $candidate).Path
    }

    foreach ($relative in @('src-tauri/target/release/trans.exe', 'src-tauri/target/debug/trans.exe')) {
        $path = Join-Path $repoRoot $relative
        if (Test-Path $path) { return (Resolve-Path $path).Path }
    }
    throw "没有找到 trans.exe，请先执行：cd src-tauri; cargo build --release"
}

function Resolve-LogPath {
    $candidates = @(
        (Join-Path $env:APPDATA "$identifier/logs/trans.log"),
        (Join-Path $env:LOCALAPPDATA "$identifier/logs/trans.log")
    )
    foreach ($path in $candidates) {
        if (Test-Path $path) { return $path }
    }
    return $candidates[0]
}

function Get-ProcessTreeWorkingSetMb {
    param([int] $RootPid)

    $all = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, WorkingSetSize
    $byParent = $all | Group-Object ParentProcessId -AsHashTable -AsString

    $total = 0
    $seen = [System.Collections.Generic.HashSet[int]]::new()
    $queue = [System.Collections.Generic.Queue[int]]::new()
    $queue.Enqueue($RootPid)

    while ($queue.Count -gt 0) {
        $current = $queue.Dequeue()
        if (-not $seen.Add($current)) { continue }

        $self = $all | Where-Object { $_.ProcessId -eq $current }
        if ($self) { $total += [int64] $self.WorkingSetSize }

        $children = $byParent["$current"]
        if ($children) {
            foreach ($child in $children) { $queue.Enqueue([int] $child.ProcessId) }
        }
    }

    [math]::Round($total / 1MB, 1)
}

$exePath = Resolve-Exe $Exe
$logPath = Resolve-LogPath

Write-Host "可执行文件 : $exePath"
Write-Host "日志文件   : $logPath"

# 掐掉上一轮实例，保证是真正的冷启动
Get-Process -Name trans -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 400

New-Item -ItemType Directory -Force (Split-Path -Parent $logPath) | Out-Null
if (Test-Path $logPath) { Remove-Item $logPath -Force }

$startedAt = Get-Date
$process = Start-Process -FilePath $exePath -PassThru
Write-Host "PID        : $($process.Id)"

$coldStartMs = $null
$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
while ((Get-Date) -lt $deadline) {
    if (Test-Path $logPath) {
        $match = Select-String -Path $logPath -Pattern 'startup tray-ready ([0-9.]+)ms' |
                 Select-Object -Last 1
        if ($match) {
            $coldStartMs = [double] $match.Matches[0].Groups[1].Value
            break
        }
    }
    Start-Sleep -Milliseconds 100
}

if ($null -eq $coldStartMs) {
    Write-Warning "在 $TimeoutSeconds 秒内没等到 'startup tray-ready' 日志，托盘可能初始化失败。"
}

Start-Sleep -Seconds $SettleSeconds

$process.Refresh()
$mainRss = [math]::Round($process.WorkingSet64 / 1MB, 1)
$treeRss = Get-ProcessTreeWorkingSetMb -RootPid $process.Id
$wallClockMs = [math]::Round(((Get-Date) - $startedAt).TotalMilliseconds, 0)

$result = [ordered]@{
    exe             = $exePath
    measured_at     = (Get-Date).ToString('o')
    cold_start_ms   = $coldStartMs
    wall_clock_ms   = $wallClockMs
    rss_main_mb     = $mainRss
    rss_tree_mb     = $treeRss
    settle_seconds  = $SettleSeconds
    note            = 'rss 不含按需加载的模型权重；含 WebView2 子进程'
}

Write-Host ''
$result.GetEnumerator() | ForEach-Object { '{0,-16} {1}' -f $_.Key, $_.Value }

$metricsDir = Join-Path $repoRoot '.metrics'
New-Item -ItemType Directory -Force $metricsDir | Out-Null
$outFile = Join-Path $metricsDir ('baseline-{0}.json' -f (Get-Date -Format 'yyyyMMdd-HHmmss'))
$result | ConvertTo-Json -Depth 4 | Set-Content -Path $outFile -Encoding utf8
Write-Host ''
Write-Host "已写入 $outFile" -ForegroundColor Green

Get-Process -Name trans -ErrorAction SilentlyContinue | Stop-Process -Force
