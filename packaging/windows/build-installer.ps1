#Requires -Version 5.1
<#
.SYNOPSIS
    mdview の Windows インストーラを組む（B-3）。

.DESCRIPTION
    版数は Cargo.toml から取る。**2 か所に書くと必ず食い違う。**

    先に次が要る。
      - アイコン: cargo run --example make-icons
      - 実行ファイル: cargo build --release

    Inno Setup（ISCC.exe）が無ければ、入れ方を示して止まる。

.EXAMPLE
    pwsh -File packaging\windows\build-installer.ps1
#>
[CmdletBinding()]
param(
    # 既定は手元のリリースビルド
    [string]$SourceExe,
    [string]$OutputDir
)

$ErrorActionPreference = 'Stop'

$repo = Resolve-Path (Join-Path $PSScriptRoot '..\..')
if (-not $SourceExe) { $SourceExe = Join-Path $repo 'target\release\mdview.exe' }
if (-not $OutputDir) { $OutputDir = Join-Path $repo 'dist' }

# **ISCC へは絶対パスで渡す。**
#
# `.iss` の中の相対パスは、**スクリプトの置き場**（packaging\windows）を
# 基準に解かれる。呼び出し側の作業フォルダ基準ではない。相対のまま渡すと
# `packaging\windows\target\...` を探して「ファイルが無い」で落ちる
# （組み立ての試験で踏んだ。2026-10-05）。
#
# ここで直すのは、`Test-Path` は通ってしまうからである——PowerShell は
# 作業フォルダ基準で解くので、**落ちるのは ISCC まで進んでから**になる。
function Resolve-Full([string]$path) {
    if ([System.IO.Path]::IsPathRooted($path)) {
        return [System.IO.Path]::GetFullPath($path)
    }
    # **出力先はまだ無いことがある**ので Resolve-Path は使えない
    return [System.IO.Path]::GetFullPath((Join-Path (Get-Location).Path $path))
}

$SourceExe = Resolve-Full $SourceExe
$OutputDir = Resolve-Full $OutputDir

function Fail([string]$message) {
    Write-Host "失敗: $message" -ForegroundColor Red
    exit 1
}

# --- 版数を取る（Cargo.toml が唯一の出どころ） ---
$cargo = Get-Content (Join-Path $repo 'Cargo.toml') -Encoding UTF8
$line = $cargo | Where-Object { $_ -match '^version\s*=' } | Select-Object -First 1
if (-not $line) { Fail 'Cargo.toml から version を取れない' }
$version = ($line -split '"')[1]

# `2.0.0-alpha.1` から数字だけを 4 つ取り出す（リソースに入れるため）
$numbers = [regex]::Matches($version, '\d+') | ForEach-Object { $_.Value }
while ($numbers.Count -lt 4) { $numbers += '0' }
$fileVersion = ($numbers[0..3]) -join '.'

Write-Host "版数: $version（リソース: $fileVersion）"

# --- 材料がそろっているか ---
if (-not (Test-Path $SourceExe)) {
    Fail "実行ファイルが無い: $SourceExe`n  cargo build --release を先に実行する"
}
$icon = Join-Path $repo 'assets\icons\mdview.ico'
if (-not (Test-Path $icon)) {
    Fail "アイコンが無い: $icon`n  cargo run --example make-icons を先に実行する"
}

# --- Inno Setup を探す ---
$iscc = Get-Command 'iscc.exe' -ErrorAction SilentlyContinue
if ($iscc) {
    $isccPath = $iscc.Source
} else {
    # **利用者ごとの置き場も見る。** winget は既定でそちらへ入れる
    $candidates = @(
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    )
    $isccPath = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $isccPath) {
    Fail @"
Inno Setup が見つからない。次のいずれかで入れる。
  winget install --id JRSoftware.InnoSetup --silent
  choco install innosetup -y
"@
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

$script = Join-Path $PSScriptRoot 'mdview.iss'
& $isccPath `
    "/DMyAppVersion=$version" `
    "/DMyFileVersion=$fileVersion" `
    "/DMySourceExe=$SourceExe" `
    "/DMyOutputDir=$OutputDir" `
    $script

if ($LASTEXITCODE -ne 0) { Fail "ISCC が終了コード $LASTEXITCODE で失敗した" }

$setup = Join-Path $OutputDir "mdview-$version-windows-x86_64-setup.exe"
if (-not (Test-Path $setup)) { Fail "出来たはずの $setup が無い" }

$size = [math]::Round((Get-Item $setup).Length / 1MB, 1)
Write-Host "作った: $setup（$size MB）" -ForegroundColor Green
