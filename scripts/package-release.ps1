# 发布打包：NSIS 安装包 + 免安装 zip
#
# 产物都放 dist/：
#   Lemma_<ver>_x64-setup.exe   官方安装包（NSIS）
#   Lemma_<ver>_x64-portable.zip 免安装：解压即用（exe + models/）
#
# 注意：免安装版**数据仍在 %APPDATA%**（设置、词库索引、模型落地目录），
# 只是不用安装程序。想彻底便携需要另做"数据跟着程序走"，目前没做。
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dist = Join-Path $root "dist"
New-Item -ItemType Directory -Force $dist | Out-Null

$ver = (Get-Content (Join-Path $root "src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json).version
Write-Host "版本 $ver"

Push-Location $root
try {
  Write-Host "== 构建（NSIS + exe）=="
  & pwsh -NoProfile -File (Join-Path $PSScriptRoot "rebuild.ps1")
  if ($LASTEXITCODE -ne 0) { throw "构建失败" }
} finally {
  Pop-Location
}

$release = Join-Path $root "src-tauri\target\release"
$setup = Get-ChildItem (Join-Path $release "bundle\nsis") -Filter *.exe -ErrorAction SilentlyContinue |
  Select-Object -First 1
if ($setup) {
  Copy-Item $setup.FullName (Join-Path $dist "Lemma_${ver}_x64-setup.exe") -Force
  Write-Host ("安装包 : {0:N1} MB" -f ($setup.Length / 1MB))
} else {
  Write-Warning "没找到 NSIS 安装包（target/release/bundle/nsis 为空）"
}

# ---- 免安装包：exe + models/ ----
$portable = Join-Path $env:TEMP "lemma-portable"
if (Test-Path $portable) { Remove-Item $portable -Recurse -Force }
New-Item -ItemType Directory -Force $portable | Out-Null
Copy-Item (Join-Path $release "lemma.exe") $portable -Force

# 安装包里的资源（模型/词库）也要带上，否则免安装版缺数据
$models = Join-Path $release "models"
if (Test-Path $models) {
  Copy-Item $models $portable -Recurse -Force
} else {
  Write-Warning "target/release/models 不存在：免安装包将不含词库与模型"
}

@"
Lemma 免安装版 $ver

双击 lemma.exe 即可运行（托盘常驻）。
数据仍在 %APPDATA%\dev.lunar.lemma\：设置、翻译缓存、模型落地目录。
卸载 = 删掉本文件夹 + 删掉上面那个数据目录。
"@ | Set-Content (Join-Path $portable "使用说明.txt") -Encoding utf8

$zip = Join-Path $dist "Lemma_${ver}_x64-portable.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path (Join-Path $portable "*") -DestinationPath $zip -CompressionLevel Optimal
Write-Host ("免安装 : {0:N1} MB" -f ((Get-Item $zip).Length / 1MB))

Write-Host "`n产物："
Get-ChildItem $dist -File | Sort-Object Name | ForEach-Object {
  Write-Host ("  {0,-40} {1,8:N1} MB" -f $_.Name, ($_.Length / 1MB))
}
