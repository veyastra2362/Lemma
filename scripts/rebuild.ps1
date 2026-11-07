# 重建 lemma.exe 并保证能写文件。
#
# 为什么需要：浏览器扩展连着宿主时，Chrome 会持有 lemma.exe 句柄，
# `pnpm tauri build` 会报 "failed to remove file ...lemma.exe"（拒绝访问）。
# 这里先把宿主清单临时改名切断重连，重建完再放回去。
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$manifest = Join-Path $env:APPDATA "dev.lunar.lemma\native-host\dev.lunar.lemma.json"

if (Test-Path $manifest) {
  Move-Item $manifest "$manifest.off" -Force
  Write-Host "已临时移开宿主清单（切断浏览器重连）"
}
taskkill /F /IM lemma.exe 2>$null | Out-Null
Start-Sleep -Milliseconds 800

Push-Location $root
try {
  & pnpm tauri build @args
} finally {
  Pop-Location
  if (Test-Path "$manifest.off") {
    Move-Item "$manifest.off" $manifest -Force
    Write-Host "宿主清单已恢复"
  }
}
