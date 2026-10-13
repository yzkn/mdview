#Requires -Version 5.1
<#
.SYNOPSIS
    この clone の中身を検査してコミットし、公開リポジトリの main へ push する。

.DESCRIPTION
    **公開用 PC で、公開リポジトリの clone に置いたこのスクリプトを実行する**（2026-10-13）。

      cd <公開リポジトリの clone>
      pwsh -File .\tools\publish.ps1 -DryRun     # 検査まで。何も書き換えない
      pwsh -File .\tools\publish.ps1 -Watch      # 検査 → コミット → push → CI を追う

    公開する中身は、**先にこの clone へ置いておく**（このスクリプトは外から何も取ってこない）。
    置いたものをそのまま検査し、変わっていればコミットして push する。

    名前・アドレス・公開先は、**その PC の git と gh の設定から取る**（設定ファイルは無い）。
    公開リポジトリであること（gh repo view の isPrivate が偽）を確かめてから push する。

    push のあとは公開側の CI が組んで包み、**版数が新しければタグを打って Release まで作る**。

    PowerShell 5.1 でも 7 でも動く。
    **UTF-8 の BOM 付きで保存すること**（5.1 は BOM が無いと日本語を読み違える）。

.PARAMETER Message
    コミットのメッセージ。省くと、版数が新しければ "chore: v<版数>"、同じなら "chore: 更新を反映"。

.PARAMETER DryRun
    検査まで行い、コミットする前に止まる。

.PARAMETER Watch
    push のあと、公開側の CI を gh run watch で追う。

.PARAMETER Yes
    確かめの問いに、すべて「はい」と答える。

.PARAMETER NoGh
    **試験用。** gh を使う確かめ（公開リポジトリか・名義）と fetch・push を飛ばす。
#>
[CmdletBinding()]
param(
    [string]$Message,
    [switch]$DryRun,
    [switch]$Watch,
    [switch]$Yes,
    [switch]$NoGh
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

# git の出力（UTF-8）を読み違えない
$previousEncoding = [Console]::OutputEncoding
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false
$OutputEncoding = New-Object System.Text.UTF8Encoding $false

function Write-Step([string]$text) { Write-Host ""; Write-Host "== $text" -ForegroundColor Cyan }
function Stop-Publish([string]$text) {
    Write-Host ""
    Write-Host "止めた: $text" -ForegroundColor Red
    [Console]::OutputEncoding = $previousEncoding
    exit 1
}
function Confirm-Step([string]$question) {
    if ($Yes) { return $true }
    $answer = Read-Host "$question [y/N]"
    return ($answer -eq 'y' -or $answer -eq 'Y')
}

# 外のコマンド（git / gh）を呼び、**標準出力だけ**を返す。
#
# **標準エラーは分けて受ける。** PowerShell 5.1 は、止まる設定（Stop）のまま
# 外のコマンドが標準エラーへ書くと例外にする。git は成功しても警告
# （「LF will be replaced by CRLF」など）を書くので、5.1 だけ途中で落ちていた。
# 呼ぶあいだだけ Continue にし、成否は終了コードで見る
function Invoke-Tool {
    param([string]$Exe, [string[]]$Arguments, [switch]$AllowFail)
    $saved = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $all = @(& $Exe @Arguments 2>&1)
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $saved
    }
    $out = @($all | Where-Object { $_ -isnot [System.Management.Automation.ErrorRecord] } | ForEach-Object { "$_" })
    $err = @($all | Where-Object { $_ -is [System.Management.Automation.ErrorRecord] } | ForEach-Object { "$_" })
    $global:LASTEXITCODE = $code
    if ($code -ne 0 -and -not $AllowFail) {
        Stop-Publish ("$Exe " + ($Arguments -join ' ') + " が失敗した:`n" + (($err + $out) -join "`n"))
    }
    return $out
}

# git を呼ぶ。**失敗したら止める**。ファイル名の日本語を \343… のように崩さない
function Invoke-Git {
    return Invoke-Tool -Exe git -Arguments (@('-c', 'core.quotepath=false') + $args)
}

# 標準出力を 1 つの文字列にする（前後の空白を落とす）
function Join-Output($lines) { return ((@($lines) -join "`n")).Trim() }

try {
    # ------------------------------------------------------------ 1. 場所
    Write-Step "1. 公開リポジトリの clone の中か"
    if (-not (Get-Command git -ErrorAction SilentlyContinue)) { Stop-Publish "git が無い" }
    if (-not $NoGh -and -not (Get-Command gh -ErrorAction SilentlyContinue)) { Stop-Publish "gh が無い" }

    # **このスクリプトの 1 つ上が clone の根。** どこから起動しても、その clone を相手にする
    $top = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
    Set-Location -LiteralPath $top
    $gitTop = Join-Output (Invoke-Tool -Exe git -Arguments @('rev-parse', '--show-toplevel') -AllowFail)
    if ($LASTEXITCODE -ne 0 -or -not $gitTop) { Stop-Publish "$top は git の作業フォルダではない" }
    $gitTop = [System.IO.Path]::GetFullPath($gitTop)
    if ($gitTop.TrimEnd('\', '/') -ne $top.TrimEnd('\', '/')) {
        Stop-Publish "tools\publish.ps1 が clone の根の直下に無い（根: $gitTop）"
    }
    $branch = Join-Output (Invoke-Tool -Exe git -Arguments @('symbolic-ref', '--short', 'HEAD') -AllowFail)
    if ($branch -ne 'main') { Stop-Publish "枝が main ではない: $branch" }

    if (-not $NoGh) {
        Invoke-Git fetch origin main | Out-Null
        $behind = Join-Output (Invoke-Git rev-list --count HEAD..origin/main)
        if ($behind -ne '0') {
            Stop-Publish "origin/main より $behind 個遅れている。置いた中身を退避し、git pull してから置き直す"
        }
    }
    Write-Host "作業フォルダ: $top（main）"

    # ------------------------------------------------------------ 2. 公開先
    Write-Step "2. 公開先（公開リポジトリであること）"
    $origin = Join-Output (Invoke-Git remote get-url origin)
    if ($origin -notmatch 'github\.com[:/]([^/]+)/([^/]+?)(\.git)?$') { Stop-Publish "origin が GitHub ではない: $origin" }
    $repo = "$($Matches[1])/$($Matches[2])"
    if (-not $NoGh) {
        $view = Join-Output (Invoke-Tool -Exe gh -Arguments @('repo', 'view', $repo, '--json', 'nameWithOwner,isPrivate'))
        $info = $view | ConvertFrom-Json
        if ($info.isPrivate) { Stop-Publish "$repo は非公開のリポジトリ。公開用の clone で実行する" }
        if ($info.nameWithOwner -ne $repo) { Stop-Publish "origin（$repo）と gh の答え（$($info.nameWithOwner)）が違う" }
    }
    Write-Host "公開リポジトリ: $repo"

    # ------------------------------------------------------------ 3. 名義
    Write-Step "3. 名義"
    $userName = Join-Output (Invoke-Tool -Exe git -Arguments @('config', 'user.name') -AllowFail)
    $userEmail = Join-Output (Invoke-Tool -Exe git -Arguments @('config', 'user.email') -AllowFail)
    if (-not $userName -or -not $userEmail) { Stop-Publish "git の user.name / user.email が設定されていない" }
    $login = '（試験のため見ない）'
    if (-not $NoGh) {
        $login = Join-Output (Invoke-Tool -Exe gh -Arguments @('api', 'user', '--jq', '.login'))
    }
    Write-Host "push する名義  : $login"
    Write-Host "コミットの名前 : $userName <$userEmail>"

    # ------------------------------------------------------------ 4. 版
    Write-Step "4. 配る版か"
    $cargoPath = Join-Path $top 'Cargo.toml'
    if (-not (Test-Path -LiteralPath $cargoPath)) { Stop-Publish "Cargo.toml が無い（中身を置いたか確かめる）" }
    $cargo = Get-Content -LiteralPath $cargoPath -Encoding UTF8
    $versionLine = $cargo | Where-Object { $_ -match '^version\s*=' } | Select-Object -First 1
    if (-not $versionLine) { Stop-Publish "Cargo.toml に version が無い" }
    $version = ($versionLine -split '"')[1]
    $tag = "v$version"
    Write-Host "版: $version"
    $released = $false
    if (-not $NoGh) {
        $released = [bool](Join-Output (Invoke-Git ls-remote --tags origin "refs/tags/$tag"))
    } else {
        $released = [bool](Join-Output (Invoke-Git tag -l $tag))
    }
    if ($released) {
        Write-Host "$tag は公開側で配布済み。CI は組むが、リリースはしない（版数を上げれば配る）" -ForegroundColor Yellow
        if (-not (Confirm-Step "このまま反映しますか")) { Stop-Publish "やめた" }
    } else {
        Write-Host "$tag はまだ無い。push すると CI がタグを打って Release を作る" -ForegroundColor Green
    }

    # ------------------------------------------------------------ 5. 中身の検査
    Write-Step "5. 中身の検査"
    $problems = New-Object System.Collections.Generic.List[string]

    # **公開しないものが紛れていないか。** 中身を 1 つ上の階層ごと置いてしまったときに出る
    foreach ($name in @('migration-v2', 'doc', 'design', 'poc', 'tools\build-public-copy.py', 'tools\measure')) {
        if (Test-Path -LiteralPath (Join-Path $top $name)) { $problems.Add("$name が在る（公開する中身ではない。置き方を確かめる）") }
    }

    $textExtensions = @('.rs', '.toml', '.md', '.py', '.txt', '.yml', '.yaml', '.ps1', '.sh', '.iss', '.svg', '.json', '.bat', '.lock', '.swift')
    # この PC の名前（短すぎるものは普通の語と重なるので見ない）
    $words = New-Object System.Collections.Generic.List[string]
    if ($env:USERNAME -and $env:USERNAME.Length -ge 4) { $words.Add($env:USERNAME) }
    if ($env:USERPROFILE) { $words.Add($env:USERPROFILE) }
    $secretPatterns = @(
        'ghp_[A-Za-z0-9]{20,}', 'github_pat_[A-Za-z0-9_]{20,}', 'gho_[A-Za-z0-9]{20,}',
        'AKIA[0-9A-Z]{16}', 'BEGIN [A-Z ]*PRIVATE KEY'
    )
    # **コミットされうるものだけを見る**（追跡中と、.gitignore に外されていない新しいもの）
    $listed = @(Invoke-Git ls-files --cached --others --exclude-standard)
    $files = @($listed | Where-Object { $_ } | Sort-Object -Unique)
    foreach ($relative in $files) {
        $path = Join-Path $top $relative
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { continue }   # 消したもの
        $extension = [System.IO.Path]::GetExtension($relative).ToLowerInvariant()
        if ($textExtensions -notcontains $extension) { continue }
        $bytes = [System.IO.File]::ReadAllBytes($path)
        $hasBom = ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF)
        if ($extension -eq '.sh') {
            if ($hasBom) { $problems.Add("$relative`: BOM が付いている（.sh は BOM 禁止）") }
            if ([Array]::IndexOf($bytes, [byte]13) -ge 0) { $problems.Add("$relative`: CR が混じっている（.sh は LF のみ）") }
        }
        $start = 0
        if ($hasBom) { $start = 3 }
        $text = [System.Text.Encoding]::UTF8.GetString($bytes, $start, $bytes.Length - $start)
        if ($text.IndexOf([char]0xFEFF) -ge 0) { $problems.Add("$relative`: 本文に BOM が残っている（2 重の BOM）") }
        foreach ($word in $words) {
            if ($text.IndexOf($word, [StringComparison]::OrdinalIgnoreCase) -ge 0) { $problems.Add("$relative`: この PC の名前（$word）が入っている") }
        }
        foreach ($pattern in $secretPatterns) {
            if ($text -match $pattern) { $problems.Add("$relative`: 秘密らしきもの（$pattern）") }
        }
    }
    if ($problems.Count -gt 0) {
        $problems | ForEach-Object { Write-Host "  $_" -ForegroundColor Red }
        Stop-Publish "検査で $($problems.Count) 件見つかった。直した中身を置き直す"
    }
    Write-Host "問題なし（$($files.Count) ファイル）"

    if ($DryRun) {
        Write-Host ""
        Write-Host "-DryRun のため、ここで止める（何も書き換えていない）" -ForegroundColor Yellow
        return
    }

    # ------------------------------------------------------------ 6. 確かめる
    Write-Step "6. 変わったもの"
    Invoke-Git add -A | Out-Null
    $changed = @(Invoke-Git diff --cached --name-status)
    $ahead = '0'
    if (-not $NoGh) { $ahead = Join-Output (Invoke-Git rev-list --count origin/main..HEAD) }
    if ($changed.Count -eq 0) {
        if ($ahead -eq '0') {
            Write-Host "変わったものも、まだ push していないコミットも無い。何もしない"
            return
        }
        Write-Host "変わったものは無いが、まだ push していないコミットが $ahead 個ある"
    } else {
        # **/MIR で写していたころと違い、ここでは消さない。** 公開側から消したいものは、置くときに消しておく
        $changed | Select-Object -First 60 | ForEach-Object { Write-Host "  $_" }
        if ($changed.Count -gt 60) { Write-Host "  …ほか $($changed.Count - 60) 件" }
        Write-Host (Join-Output (Invoke-Git diff --cached --shortstat))
        if (-not (Confirm-Step "この内容でコミットしますか")) {
            # 置いた中身は消さない（索引に載せたのを戻すだけ）
            Invoke-Git reset -q | Out-Null
            Stop-Publish "やめた（置いた中身はそのまま。コミットはしていない）"
        }

        # -------------------------------------------------------- 7. コミット
        Write-Step "7. コミット"
        if (-not $Message) {
            if ($released) { $Message = "chore: 更新を反映" } else { $Message = "chore: $tag" }
        }
        Invoke-Git commit -q -m $Message | Out-Null
        Write-Host (Join-Output (Invoke-Git log --oneline -1))
    }

    # ------------------------------------------------------------ 8. push
    Write-Step "8. push"
    if ($NoGh) {
        Write-Host "-NoGh のため push しない"
        return
    }
    if (-not (Confirm-Step "$repo の main へ push しますか")) {
        Write-Host "push しなかった。あとで git push origin main する（コミットは残っている）" -ForegroundColor Yellow
        return
    }
    Invoke-Git push origin main | Out-Null
    Write-Host "push した。CI: https://github.com/$repo/actions/workflows/ci.yml" -ForegroundColor Green
    if (-not $released) { Write-Host "CI が通ると $tag のタグと Release が出来る" }

    if ($Watch) {
        Start-Sleep -Seconds 10
        $runId = Join-Output (Invoke-Tool -Exe gh -Arguments @('run', 'list', '--repo', $repo, '--workflow', 'ci.yml', '--branch', 'main', '--limit', '1', '--json', 'databaseId', '--jq', '.[0].databaseId'))
        # 追うあいだは画面に出す（Invoke-Tool は出力を受け取ってしまう）
        if ($runId) { & gh run watch $runId --repo $repo }
    }
}
finally {
    [Console]::OutputEncoding = $previousEncoding
}
