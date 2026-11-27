# ==============================================================================
# Nave Terminal v3 for PowerShell (5.1 & 7+)
# Features: 5 live palettes + OSC 11/12 bg/cursor sync, 4 layouts, TrueColor
# gradient art sysinfo, hr divider, GPU shader toggle, nave-hud & nave-warp
# ==============================================================================

[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::InputEncoding  = [System.Text.Encoding]::UTF8
$script:ESC = [char]27
$script:BEL = [char]7

$script:NaveConfigDir = Join-Path $env:USERPROFILE ".config\nave"
if (-not (Test-Path $script:NaveConfigDir)) {
    New-Item -ItemType Directory -Path $script:NaveConfigDir -Force | Out-Null
}

# Universal Unicode constants
$script:CH_TL   = [char]0x256D  # ╭
$script:CH_BL   = [char]0x2570  # ╰
$script:CH_H    = [char]0x2500  # ─
$script:CH_V    = [char]0x2502  # │
$script:CH_LTEE = [char]0x251C  # ├
$script:CH_ARR  = [char]0x276F  # ❯
$script:CH_PM   = [char]0x00B1  # ±
$script:CH_UP   = [char]0x2191  # ↑
$script:CH_DN   = [char]0x2193  # ↓
$script:CH_EQ   = [char]0x2261  # ≡
$script:CH_MUL  = [char]0x00D7  # ×
$script:CH_ELL  = [char]0x2026  # …
$script:CH_MID  = [char]0x00B7  # ·
$script:CH_BLK  = [char]0x2588  # █
$script:CH_SHD  = [char]0x2591  # ░
$script:CH_CHK  = [char]0x2713  # ✓
$script:CH_DIA  = [char]0x25C6  # ◆
$script:CH_TRI  = [char]0x25B2  # ▲
$script:CH_SL   = [char]0x2571  # ╱
$script:CH_BSL  = [char]0x2572  # ╲
$script:CH_TOP  = [char]0x2580  # ▀
$script:CH_DV   = [char]0x2551  # ║
$script:CH_BR   = [char]0x256F  # ╯

$script:NaveFirstPrompt = $true
$script:NaveLastHistId = -1

function _Nave-LoadSetting([string]$name, [string]$defaultVal) {
    $f = Join-Path $script:NaveConfigDir $name
    if (Test-Path $f) {
        $v = (Get-Content $f -Raw).Trim()
        if ($v) { return $v }
    }
    return $defaultVal
}

$script:NavePalette = _Nave-LoadSetting "palette" "cyber"
$script:NaveLayout  = _Nave-LoadSetting "layout"  "framed"
$script:NaveBgSync  = (_Nave-LoadSetting "bg_sync" "1") -eq "1"

$script:NavePalettes = @{
    cyber = @{
        frame="240"; user="51"; host="135"; dir="45"; git="171"
        warn="214"; info="51"; ok="48"; err="197"; dur="220"
        muted="244"; node="83"; docker="39"
        bg="#08090F"; cursor="#00F0FF"
        g1=@(0,245,255); g2=@(124,77,255); g3=@(255,46,151)
        ps_cmd="#00FF88"; ps_param="#00F0FF"; ps_str="#FFD600"; ps_var="#C084FC"; ps_err="#FF2E63"
    }
    emerald = @{
        frame="239"; user="48"; host="51"; dir="46"; git="118"
        warn="226"; info="51"; ok="82"; err="197"; dur="190"
        muted="244"; node="84"; docker="45"
        bg="#070C09"; cursor="#00FF88"
        g1=@(0,255,136); g2=@(0,229,255); g3=@(178,255,89)
        ps_cmd="#00FF88"; ps_param="#00F5D4"; ps_str="#FFE600"; ps_var="#B2FF59"; ps_err="#FF2E63"
    }
    sunset = @{
        frame="240"; user="208"; host="204"; dir="197"; git="220"
        warn="226"; info="214"; ok="48"; err="196"; dur="226"
        muted="244"; node="118"; docker="45"
        bg="#0C070A"; cursor="#FF5E3A"
        g1=@(255,214,0); g2=@(255,94,58); g3=@(255,30,142)
        ps_cmd="#00FF88"; ps_param="#FF9F1C"; ps_str="#FFD600"; ps_var="#FF2E97"; ps_err="#FF1E56"
    }
    nord = @{
        frame="240"; user="87"; host="75"; dir="45"; git="141"
        warn="221"; info="87"; ok="85"; err="203"; dur="220"
        muted="244"; node="84"; docker="39"
        bg="#080B12"; cursor="#38E8FF"
        g1=@(56,232,255); g2=@(108,140,255); g3=@(192,132,252)
        ps_cmd="#34FFB4"; ps_param="#38E8FF"; ps_str="#FFD75F"; ps_var="#C084FC"; ps_err="#FF5F73"
    }
    stealth = @{
        frame="239"; user="255"; host="141"; dir="135"; git="177"
        warn="220"; info="141"; ok="255"; err="197"; dur="183"
        muted="243"; node="141"; docker="135"
        bg="#08080B"; cursor="#A855F7"
        g1=@(248,250,252); g2=@(168,85,247); g3=@(236,72,153)
        ps_cmd="#F8FAFC"; ps_param="#C084FC"; ps_str="#EC4899"; ps_var="#A855F7"; ps_err="#FF2E63"
    }
    dracula = @{
        frame="239"; user="212"; host="183"; dir="117"; git="183"
        warn="228"; info="117"; ok="84"; err="210"; dur="228"
        muted="103"; node="84"; docker="117"
        bg="#10111A"; cursor="#FF73C6"
        g1=@(255,115,198); g2=@(184,130,255); g3=@(107,227,245)
        ps_cmd="#5CF298"; ps_param="#6BE3F5"; ps_str="#F2E58F"; ps_var="#C79EFF"; ps_err="#FF617D"
    }
}

$script:NaveC = $script:NavePalettes["cyber"]

function _Nave-Fg([string]$code, [string]$text, [switch]$Bold) {
    $b = if ($Bold) { ";1" } else { "" }
    return "$($script:ESC)[38;5;${code}${b}m${text}$($script:ESC)[0m"
}

function _Nave-GradLine([string]$text, [switch]$Bold) {
    $c = $script:NaveC
    $g1 = $c.g1; $g2 = $c.g2; $g3 = $c.g3
    $bpre = if ($Bold) { "1;" } else { "" }
    $len = $text.Length
    $denom = if ($len -gt 1) { $len - 1 } else { 1 }
    $sb = New-Object System.Text.StringBuilder
    for ($i = 0; $i -lt $len; $i++) {
        $ch = $text[$i]
        if ($ch -eq ' ') { [void]$sb.Append(' '); continue }
        $t = $i / [double]$denom
        if ($t -le 0.5) {
            $u = $t * 2.0
            $r = [int]($g1[0] + ($g2[0] - $g1[0]) * $u)
            $g = [int]($g1[1] + ($g2[1] - $g1[1]) * $u)
            $b = [int]($g1[2] + ($g2[2] - $g1[2]) * $u)
        } else {
            $u = ($t - 0.5) * 2.0
            $r = [int]($g2[0] + ($g3[0] - $g2[0]) * $u)
            $g = [int]($g2[1] + ($g3[1] - $g2[1]) * $u)
            $b = [int]($g2[2] + ($g3[2] - $g2[2]) * $u)
        }
        [void]$sb.Append("$($script:ESC)[${bpre}38;2;${r};${g};${b}m${ch}")
    }
    [void]$sb.Append("$($script:ESC)[0m")
    return $sb.ToString()
}

function _Nave-ApplyPalette([string]$pal) {
    if (-not $script:NavePalettes.ContainsKey($pal)) {
        Write-Host (_Nave-Fg "203" "Paleta desconhecida: $pal (opcoes: cyber, emerald, sunset, nord, stealth, dracula)")
        return $false
    }
    $script:NavePalette = $pal
    $script:NaveC = $script:NavePalettes[$pal]

    if ($script:NaveBgSync -and $Host.Name -eq "ConsoleHost" -and -not [Console]::IsOutputRedirected) {
        [Console]::Write("$($script:ESC)]11;$($script:NaveC.bg)$($script:BEL)$($script:ESC)]12;$($script:NaveC.cursor)$($script:BEL)")
    }

    if (Get-Module -Name PSReadLine) {
        try {
            Set-PSReadLineOption -Colors @{
                Command          = $script:NaveC.ps_cmd
                Parameter        = $script:NaveC.ps_param
                String           = $script:NaveC.ps_str
                Variable         = $script:NaveC.ps_var
                Error            = $script:NaveC.ps_err
                InlinePrediction = "#64748B"
            } -ErrorAction SilentlyContinue
        } catch {}
    }
    return $true
}

if (Get-Module -ListAvailable -Name PSReadLine) {
    Import-Module PSReadLine -ErrorAction SilentlyContinue
    try { Set-PSReadLineOption -EditMode Windows -BellStyle None -ErrorAction SilentlyContinue } catch {}
    try { Set-PSReadLineOption -PredictionSource History -ErrorAction SilentlyContinue } catch {}
    try {
        Set-PSReadLineKeyHandler -Key UpArrow -Function HistorySearchBackward
        Set-PSReadLineKeyHandler -Key DownArrow -Function HistorySearchForward
        Set-PSReadLineKeyHandler -Chord Shift+Enter -Function AddLine
        Set-PSReadLineKeyHandler -Chord Alt+Enter -Function AddLine
        Set-PSReadLineKeyHandler -Chord Ctrl+Spacebar -Function MenuComplete
        Set-PSReadLineKeyHandler -Chord Ctrl+l -Function SelectAll
        Set-PSReadLineKeyHandler -Chord Alt+l -Function ClearScreen
    } catch {}
}

[void](_Nave-ApplyPalette $script:NavePalette)

function nave-theme {
    param([string]$Name)
    if ([string]::IsNullOrWhiteSpace($Name) -or $Name -eq "list") {
        $c = $script:NaveC
        Write-Host (_Nave-Fg $c.dir "$($script:CH_TL)$($script:CH_H) Paletas disponiveis (atual: $($script:NavePalette))")
        foreach ($p in @("cyber", "emerald", "sunset", "nord", "stealth", "dracula")) {
            $mark = if ($p -eq $script:NavePalette) { "$($script:CH_ARR) " } else { "  " }
            Write-Host ((_Nave-Fg $c.frame "$($script:CH_V) ") + $mark + (_Nave-Fg $c.ok $p -Bold))
        }
        Write-Host (_Nave-Fg $c.dir "$($script:CH_BL)$($script:CH_H) Uso: nave-theme <nome>")
        return
    }
    if (_Nave-ApplyPalette $Name) {
        Set-Content -Path (Join-Path $script:NaveConfigDir "palette") -Value $Name -NoNewline
        Write-Host ((_Nave-Fg $script:NaveC.ok "$($script:CH_CHK) Paleta alterada para $Name " -Bold) + (_Nave-Fg $script:NaveC.muted "(bg $($script:NaveC.bg))"))
    }
}

function nave-layout {
    param([string]$Mode)
    if ([string]::IsNullOrWhiteSpace($Mode) -or $Mode -eq "list") {
        $c = $script:NaveC
        Write-Host (_Nave-Fg $c.dir "$($script:CH_TL)$($script:CH_H) Layouts disponiveis (atual: $($script:NaveLayout))")
        foreach ($l in @("framed", "rail", "hud", "compact")) {
            $mark = if ($l -eq $script:NaveLayout) { "$($script:CH_ARR) " } else { "  " }
            Write-Host ((_Nave-Fg $c.frame "$($script:CH_V) ") + $mark + (_Nave-Fg $c.ok $l -Bold))
        }
        Write-Host (_Nave-Fg $c.dir "$($script:CH_BL)$($script:CH_H) Uso: nave-layout <framed|rail|hud|compact>")
        return
    }
    if ($Mode -in @("framed", "rail", "hud", "compact")) {
        $script:NaveLayout = $Mode
        Set-Content -Path (Join-Path $script:NaveConfigDir "layout") -Value $Mode -NoNewline
        Write-Host (_Nave-Fg $script:NaveC.ok "$($script:CH_CHK) Layout alterado para $Mode" -Bold)
    } else {
        Write-Host (_Nave-Fg "203" "Layout desconhecido: $Mode (opcoes: framed, rail, hud, compact)")
    }
}

function nave-bg {
    param([string]$Mode)
    switch ($Mode) {
        "on"  { $script:NaveBgSync = $true;  Set-Content (Join-Path $script:NaveConfigDir "bg_sync") "1" -NoNewline; [void](_Nave-ApplyPalette $script:NavePalette); Write-Host (_Nave-Fg $script:NaveC.ok "Background sync (OSC 11/12) ativado.") }
        "off" { $script:NaveBgSync = $false; Set-Content (Join-Path $script:NaveConfigDir "bg_sync") "0" -NoNewline; if (-not [Console]::IsOutputRedirected) { [Console]::Write("$($script:ESC)]111$($script:BEL)$($script:ESC)]112$($script:BEL)") }; Write-Host (_Nave-Fg $script:NaveC.warn "Background sync desativado.") }
        default { Write-Host "Background sync: $(if ($script:NaveBgSync) {'on'} else {'off'}) (uso: nave-bg on|off)" }
    }
}

function nave-shader {
    param([string]$Mode = "status")
    $wtPath = Join-Path $env:LOCALAPPDATA "Packages\Microsoft.WindowsTerminal_8wekyb3d8bbwe\LocalState\settings.json"
    if (-not (Test-Path $wtPath)) { Write-Host "settings.json do Windows Terminal nao encontrado."; return }
    $data = Get-Content $wtPath -Raw | ConvertFrom-Json
    $targets = @("Ubuntu", "Windows PowerShell", "PowerShell")
    if ($Mode -eq "status") {
        foreach ($p in $data.profiles.list) {
            if ($p.name -in $targets) {
                $cur = if ($p.PSObject.Properties['experimental.pixelShaderPath']) { $p.'experimental.pixelShaderPath' } else { "off" }
                Write-Host ("  {0,-20} -> {1}" -f $p.name, $cur)
            }
        }
        return
    }
    if ($Mode -notin @("glow", "crt", "off")) { Write-Host "Uso: nave-shader <glow|crt|off|status>"; return }
    $shaderPath = Join-Path $script:NaveConfigDir "shaders\nave-$Mode.hlsl"
    foreach ($p in $data.profiles.list) {
        if ($p.name -in $targets) {
            if ($Mode -eq "off") {
                $p.PSObject.Properties.Remove('experimental.pixelShaderPath')
            } else {
                $p | Add-Member -NotePropertyName 'experimental.pixelShaderPath' -NotePropertyValue $shaderPath -Force
            }
        }
    }
    $data | ConvertTo-Json -Depth 12 | Set-Content -Path $wtPath -Encoding UTF8
    Write-Host (_Nave-Fg $script:NaveC.ok "$($script:CH_CHK) Windows Terminal shader alterado para: $Mode" -Bold)
}

function hr {
    param([Parameter(ValueFromRemainingArguments=$true)][string[]]$Title)
    $w = 80
    try { if ([Console]::WindowWidth -gt 20) { $w = [Console]::WindowWidth } } catch {}
    $label = ($Title -join " ").Trim()
    $ts = Get-Date -Format "HH:mm"
    if ($label) {
        $prefix = "$([string]$script:CH_H * 3) $label "
        $suffix = " $ts $([string]$script:CH_H * 3)"
        $fill = [Math]::Max(3, $w - $prefix.Length - $suffix.Length)
        Write-Host (_Nave-GradLine ($prefix + ([string]$script:CH_H * $fill) + $suffix) -Bold)
    } else {
        Write-Host (_Nave-GradLine ([string]$script:CH_H * $w))
    }
}

function _Nave-ShortPath([string]$fullPath) {
    $homeDir = $env:USERPROFILE
    $p = $fullPath
    if ($p.StartsWith($homeDir, [System.StringComparison]::OrdinalIgnoreCase)) {
        $p = "~" + $p.Substring($homeDir.Length)
    }
    $p = $p -replace '\\', '/'
    $parts = $p.Split('/') | Where-Object { $_ -ne "" }
    if ($parts.Count -gt 4) {
        $tail = ($parts[($parts.Count - 3)..($parts.Count - 1)]) -join '/'
        return "$($script:CH_ELL)/$tail"
    }
    return $p
}

function _Nave-FormatDuration([double]$secs) {
    if ($secs -lt 2) { return "" }
    $s = [int][Math]::Floor($secs)
    $out = ""
    if ($s -ge 86400) { $out += "$([int][Math]::Floor($s / 86400))d"; $s = $s % 86400 }
    if ($s -ge 3600)  { $out += "$([int][Math]::Floor($s / 3600))h"; $s = $s % 3600 }
    if ($s -ge 60)    { $out += "$([int][Math]::Floor($s / 60))m"; $s = $s % 60 }
    if ($s -gt 0 -or $out.Length -gt 0) { $out += "${s}s" }
    return $out
}

function _Nave-GitInfo {
    if (-not (Get-Command git -ErrorAction SilentlyContinue)) { return $null }
    $gitDir = (& git rev-parse --git-dir 2>$null)
    if ($LASTEXITCODE -ne 0 -or -not $gitDir) { return $null }
    $branch = (& git symbolic-ref --short HEAD 2>$null)
    if (-not $branch) { $branch = (& git rev-parse --short HEAD 2>$null); if ($branch) { $branch = "@$branch" } }
    if (-not $branch) { return $null }

    $action = ""
    if ((Test-Path (Join-Path $gitDir "rebase-merge")) -or (Test-Path (Join-Path $gitDir "rebase-apply"))) { $action = "REBASE" }
    elseif (Test-Path (Join-Path $gitDir "MERGE_HEAD")) { $action = "MERGE" }
    elseif (Test-Path (Join-Path $gitDir "CHERRY_PICK_HEAD")) { $action = "CHERRY-PICK" }

    $statusLines = @(& git status --porcelain=v1 --branch 2>$null)
    $aheadBehind = ""
    if ($statusLines.Count -gt 0) {
        if ($statusLines[0] -match 'ahead (\d+)')  { $aheadBehind += "$($script:CH_UP)$($Matches[1])" }
        if ($statusLines[0] -match 'behind (\d+)') { $aheadBehind += "$($script:CH_DN)$($Matches[1])" }
    }
    $staged = ""; $dirty = ""; $untracked = ""
    foreach ($line in $statusLines) {
        if ($line.StartsWith("## ") -or $line.Length -lt 2) { continue }
        $x = $line[0]; $y = $line[1]
        if ($x -eq '?') { $untracked = "?" }
        else { if ($x -ne ' ') { $staged = "+" }; if ($y -ne ' ') { $dirty = "*" } }
        if ($staged -and $dirty -and $untracked) { break }
    }
    $stashCount = (& git rev-list --walk-reflogs --count refs/stash 2>$null)
    $c = $script:NaveC
    $out = _Nave-Fg $c.git "$($script:CH_PM) $branch"
    if ($action) { $out += " " + (_Nave-Fg $c.err $action -Bold) }
    $flags = "$staged$dirty$untracked"
    if ($flags) { $out += " " + (_Nave-Fg $c.warn $flags) }
    if ($aheadBehind) { $out += " " + (_Nave-Fg $c.info $aheadBehind) }
    if ($stashCount -and [int]$stashCount -gt 0) { $out += " " + (_Nave-Fg $c.muted "$($script:CH_EQ)$stashCount") }
    return $out
}

function _Nave-ContextInfo {
    $c = $script:NaveC
    $parts = @()
    if ($env:VIRTUAL_ENV) { $parts += _Nave-Fg $c.ok ("venv:" + (Split-Path $env:VIRTUAL_ENV -Leaf)) }
    elseif ($env:CONDA_DEFAULT_ENV) { $parts += _Nave-Fg $c.ok ("conda:" + $env:CONDA_DEFAULT_ENV) }
    if ((Test-Path "package.json") -and (Get-Command node -ErrorAction SilentlyContinue)) {
        $nver = (& node -v 2>$null); if ($nver) { $parts += _Nave-Fg $c.node "node:$nver" }
    }
    if ((Test-Path "Dockerfile") -or (Test-Path "docker-compose.yml") -or (Test-Path "docker-compose.yaml") -or (Test-Path "compose.yaml")) {
        $parts += _Nave-Fg $c.docker "[docker]"
    }
    return ($parts -join "  ")
}

function _Nave-VisLen([string]$s) {
    return ([regex]::Replace($s, "\x1b\[[0-9;]*m", "")).Length
}

function prompt {
    $lastOk = $?
    $lastCode = $global:LASTEXITCODE

    if ($script:NaveFirstPrompt) {
        $script:NaveFirstPrompt = $false
    } elseif ($script:NaveLayout -ne "compact") {
        Write-Host ""
    }

    $durStr = ""
    $hist = Get-History -Count 1 -ErrorAction SilentlyContinue
    if ($hist -and $hist.Id -ne $script:NaveLastHistId) {
        $script:NaveLastHistId = $hist.Id
        $durStr = _Nave-FormatDuration (($hist.EndExecutionTime - $hist.StartExecutionTime).TotalSeconds)
    }

    $c = $script:NaveC
    $isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    $uColor = if ($isAdmin) { $c.err } else { $c.user }

    $userHost = (_Nave-Fg $uColor $env:USERNAME.ToLower()) + (_Nave-Fg $c.muted "@") + (_Nave-Fg $c.host $env:COMPUTERNAME.ToLower())
    $dirShort = _Nave-ShortPath (Get-Location).Path
    $dirPart  = _Nave-Fg $c.dir $dirShort -Bold
    $gitPart  = _Nave-GitInfo
    $ctxPart  = _Nave-ContextInfo

    $hasErr = (-not $lastOk) -or ($lastCode -and $lastCode -ne 0)
    $errPart = if ($hasErr) { $cd = if ($lastCode -and $lastCode -ne 0) { $lastCode } else { 1 }; _Nave-Fg $c.err "$($script:CH_MUL)$cd" -Bold } else { "" }
    $durPart = if ($durStr) { _Nave-Fg $c.dur $durStr } else { "" }
    $arrColor = if ($hasErr) { $c.err } else { $c.ok }
    $timeText = "[" + (Get-Date -Format "HH:mm:ss") + "]"

    $Host.UI.RawUI.WindowTitle = "$($env:USERNAME)@$($env:COMPUTERNAME): $dirShort"

    if ($script:NaveLayout -eq "compact") {
        $segs = "$userHost $dirPart"
        if ($gitPart) { $segs += " $gitPart" }
        if ($errPart) { $segs += " $errPart" }
        return "$segs " + (_Nave-Fg $arrColor $script:CH_ARR -Bold) + " "
    }

    if ($script:NaveLayout -eq "hud") {
        $sep = " " + (_Nave-Fg $c.frame $script:CH_V) + " "
        $line1 = (_Nave-Fg $c.frame "$($script:CH_TL)$($script:CH_H)") + " " + (_Nave-Fg $c.dir $script:CH_DIA) + " $userHost$sep$dirPart"
        if ($gitPart) { $line1 += "$sep$gitPart" }
        if ($ctxPart) { $line1 += "$sep$ctxPart" }
        if ($durPart) { $line1 += "$sep$durPart" }
        if ($errPart) { $line1 += "$sep$errPart" }
    } else {
        $line1 = (_Nave-Fg $c.frame "$($script:CH_TL)$($script:CH_H)") + " $userHost  $dirPart"
        if ($gitPart) { $line1 += "  $gitPart" }
        if ($ctxPart) { $line1 += "  $ctxPart" }
        if ($durPart) { $line1 += "  $durPart" }
        if ($errPart) { $line1 += "  $errPart" }
    }

    $winW = 0
    try { $winW = [Console]::WindowWidth } catch {}
    $vis = _Nave-VisLen $line1
    if ($winW -gt ($vis + $timeText.Length + 5)) {
        $gap = $winW - $vis - $timeText.Length - 2
        if ($script:NaveLayout -eq "rail" -and $gap -ge 3) {
            $line1 += " " + (_Nave-Fg $c.frame ([string]$script:CH_H * ($gap - 1))) + " " + (_Nave-Fg $c.muted $timeText)
        } elseif ($gap -gt 2) {
            $line1 += (" " * $gap) + (_Nave-Fg $c.muted $timeText)
        }
    }

    $line2 = (_Nave-Fg $c.frame "$($script:CH_BL)$($script:CH_H)") + " " + (_Nave-Fg $arrColor $script:CH_ARR -Bold) + " "
    return "$line1`n$line2"
}

function _Nave-Bar([int]$pct, [int]$width = 14) {
    if ($pct -lt 0) { $pct = 0 }; if ($pct -gt 100) { $pct = 100 }
    $filled = [int][Math]::Round(($pct * $width) / 100)
    $empty = $width - $filled
    $c = $script:NaveC
    $col = $c.ok
    if ($pct -ge 70) { $col = $c.warn }
    if ($pct -ge 88) { $col = $c.err }
    $bf = if ($filled -gt 0) { [string]$script:CH_BLK * $filled } else { "" }
    $be = if ($empty -gt 0)  { [string]$script:CH_SHD * $empty }  else { "" }
    return (_Nave-Fg $col $bf) + (_Nave-Fg $c.frame $be)
}

function sysinfo {
    $c = $script:NaveC
    $os = Get-CimInstance Win32_OperatingSystem
    $disk = Get-CimInstance Win32_LogicalDisk -Filter "DeviceID='C:'"
    $totalRam = [Math]::Round($os.TotalVisibleMemorySize / 1MB, 1)
    $freeRam  = [Math]::Round($os.FreePhysicalMemory / 1MB, 1)
    $usedRam  = [Math]::Round($totalRam - $freeRam, 1)
    $ramPct   = [int][Math]::Round(($usedRam / $totalRam) * 100)
    $totalDisk = [Math]::Round($disk.Size / 1GB, 1)
    $usedDisk  = [Math]::Round(($disk.Size - $disk.FreeSpace) / 1GB, 1)
    $diskPctF  = [Math]::Round(($usedDisk / $totalDisk) * 100, 1)
    $diskPct   = [int][Math]::Round($diskPctF)
    $upSpan = (Get-Date) - $os.LastBootUpTime
    $upStr  = "$([int]$upSpan.TotalHours)h $($upSpan.Minutes)m"
    $tcpCount = (Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue).Count

    $art = @(
        "         $($script:CH_TRI)         "
        "        $($script:CH_SL)$($script:CH_BLK)$($script:CH_BSL)        "
        "       $($script:CH_SL)$([string]$script:CH_BLK * 3)$($script:CH_BSL)       "
        "      $($script:CH_SL)$($script:CH_BLK) $($script:CH_DIA) $($script:CH_BLK)$($script:CH_BSL)      "
        "     $($script:CH_SL)$([string]$script:CH_BLK * 7)$($script:CH_BSL)     "
        "    $($script:CH_SL)$([string]$script:CH_BLK * 2) $($script:CH_TOP)$($script:CH_BLK)$($script:CH_TOP) $([string]$script:CH_BLK * 2)$($script:CH_BSL)    "
        "   $($script:CH_BL)$([string]$script:CH_H * 2)$($script:CH_BR)  $($script:CH_DV)  $($script:CH_BL)$([string]$script:CH_H * 2)$($script:CH_BR)   "
        "    N $($script:CH_MID) A $($script:CH_MID) V $($script:CH_MID) E  "
    )
    $frm = _Nave-Fg $c.frame $script:CH_V
    $info = @(
        ((_Nave-Fg $c.dir "$($script:CH_TL)$($script:CH_H) $($env:USERNAME)@$($env:COMPUTERNAME) " -Bold) + (_Nave-Fg $c.frame "[$($script:NavePalette) $($script:CH_MID) $($script:NaveLayout)]"))
        "$frm " + (_Nave-Fg $c.git "OS:      ") + $os.Caption
        "$frm " + (_Nave-Fg $c.git "Shell:   ") + "PowerShell $($PSVersionTable.PSVersion)"
        "$frm " + (_Nave-Fg $c.git "Uptime:  ") + $upStr
        "$frm " + (_Nave-Fg $c.git "Memory:  ") + (_Nave-Bar $ramPct 14) + " ${usedRam}Gi/${totalRam}Gi (${ramPct}%)"
        "$frm " + (_Nave-Fg $c.git "Disk C:: ") + (_Nave-Bar $diskPct 14) + " ${usedDisk}G/${totalDisk}G (${diskPctF}%)"
        "$frm " + (_Nave-Fg $c.git "Net:     ") + "$tcpCount portas TCP em escuta"
        (_Nave-Fg $c.dir ("$($script:CH_BL)" + ([string]$script:CH_H * 46)))
    )

    $showArt = $true
    try { if ([Console]::WindowWidth -gt 0 -and [Console]::WindowWidth -lt 76) { $showArt = $false } } catch {}
    for ($i = 0; $i -lt $info.Count; $i++) {
        if ($showArt) { Write-Host ((_Nave-GradLine $art[$i] -Bold) + "  " + $info[$i]) }
        else { Write-Host $info[$i] }
    }
}

function nave-hud  { if (Get-Command wsl.exe -ErrorAction SilentlyContinue) { & wsl.exe -e bash -lc "~/.local/bin/nave-hud $($script:NavePalette)" } else { sysinfo } }
function nave-warp { if (Get-Command wsl.exe -ErrorAction SilentlyContinue) { & wsl.exe -e bash -lc "~/.local/bin/nave-warp" } else { Write-Host "Requer WSL/Python instalado." } }
function nave-edit {
    if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) { Write-Host "Requer WSL instalado."; return }
    $wslArgs = @()
    foreach ($a in $args) {
        if ($a -match '^[A-Za-z]:\\' -or (Test-Path $a -ErrorAction SilentlyContinue)) {
            $full = try { (Resolve-Path $a -ErrorAction Stop).Path } catch { [System.IO.Path]::GetFullPath($a) }
            $conv = (& wsl.exe wslpath -u $full 2>$null)
            if ($conv) { $wslArgs += $conv.Trim() } else { $wslArgs += $a }
        } else {
            $wslArgs += $a
        }
    }
    & wsl.exe bash -lc 'exec "$HOME/.local/bin/nave-edit" "$@"' -- @wslArgs
}
Set-Alias -Name ne -Value nave-edit -Scope Global -ErrorAction SilentlyContinue
function nave-cursor-reset {
    $exe = Join-Path $HOME ".config\nave\nave-hidemouse.exe"
    if (Test-Path $exe) { & $exe --reset; Write-Host "Cursor do Windows restaurado." -ForegroundColor Green }
}

function ginfo {
    if (-not (Get-Command git -ErrorAction SilentlyContinue)) { Write-Host "Git nao encontrado."; return }
    $top = (& git rev-parse --show-toplevel 2>$null)
    if ($LASTEXITCODE -ne 0 -or -not $top) { Write-Host "Nao e um repositorio Git."; return }
    $c = $script:NaveC
    $repo = Split-Path $top -Leaf
    $br = (& git symbolic-ref --short HEAD 2>$null); if (-not $br) { $br = (& git rev-parse --short HEAD 2>$null) }
    $remote = (& git remote get-url origin 2>$null); if (-not $remote) { $remote = "sem remote" }
    $stashes = @(& git stash list 2>$null).Count
    $changed = @(& git status --porcelain 2>$null).Count
    $frm = _Nave-Fg $c.frame $script:CH_V
    Write-Host (_Nave-Fg $c.dir ("$($script:CH_TL)$($script:CH_H) Git: $repo " + ([string]$script:CH_H * 34)))
    Write-Host ("$frm " + (_Nave-Fg $c.git "Branch: ") + "$br  " + (_Nave-Fg $c.muted "(alterados: $changed, stashes: $stashes)"))
    Write-Host ("$frm " + (_Nave-Fg $c.git "Remote: ") + $remote)
    Write-Host (_Nave-Fg $c.frame ("$($script:CH_LTEE)" + ([string]$script:CH_H * 50)))
    & git log --color=always --pretty="format:  %C(yellow)%h%C(reset) %s %C(dim)(%cr, %an)%C(reset)" -n 6
    Write-Host ""
    if ($changed -gt 0) {
        Write-Host (_Nave-Fg $c.frame ("$($script:CH_LTEE)" + ([string]$script:CH_H * 50)))
        & git -c color.status=always status -sb | ForEach-Object { Write-Host "  $_" }
    }
    Write-Host (_Nave-Fg $c.dir ("$($script:CH_BL)" + ([string]$script:CH_H * 50)))
}

function ports {
    $c = $script:NaveC
    Write-Host (_Nave-Fg $c.dir ("$($script:CH_TL)$($script:CH_H) Portas TCP em escuta " + ([string]$script:CH_H * 36)))
    Write-Host ("  {0,-20} {1,-8} {2}" -f "ENDERECO:PORTA", "PID", "PROCESSO")
    Write-Host (_Nave-Fg $c.frame ("$($script:CH_LTEE)" + ([string]$script:CH_H * 57)))
    Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Sort-Object LocalPort -Unique | ForEach-Object {
        $proc = (Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue).ProcessName; if (-not $proc) { $proc = "-" }
        Write-Host ("  {0,-20} {1,-8} {2}" -f "$($_.LocalAddress):$($_.LocalPort)", $_.OwningProcess, $proc)
    }
    Write-Host (_Nave-Fg $c.dir ("$($script:CH_BL)" + ([string]$script:CH_H * 57)))
}

function killport {
    param([Parameter(Mandatory=$true)][int]$Port)
    $conns = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue
    if (-not $conns) { Write-Host (_Nave-Fg $script:NaveC.warn "Nenhum processo encontrado na porta $Port."); return }
    foreach ($pidVal in ($conns.OwningProcess | Select-Object -Unique)) {
        $pname = (Get-Process -Id $pidVal -ErrorAction SilentlyContinue).ProcessName
        try { Stop-Process -Id $pidVal -Force -ErrorAction Stop; Write-Host (_Nave-Fg $script:NaveC.ok "$($script:CH_CHK) Finalizado PID $pidVal ($pname) na porta $Port.") }
        catch { Write-Host (_Nave-Fg $script:NaveC.err "$($script:CH_MUL) Erro ao finalizar PID $pidVal ($pname).") }
    }
}

function mkcd { param([Parameter(Mandatory=$true)][string]$Path) New-Item -ItemType Directory -Path $Path -Force | Out-Null; Set-Location $Path }
function which { param([Parameter(Mandatory=$true)][string]$Name) (Get-Command $Name -ErrorAction SilentlyContinue).Source }

function nave-welcome {
    param([string]$Mode)
    $f = Join-Path $script:NaveConfigDir "welcome"
    switch ($Mode) {
        "on"  { Set-Content $f "1" -NoNewline; Write-Host (_Nave-Fg $script:NaveC.ok "Welcome banner ativado.") }
        "off" { Set-Content $f "0" -NoNewline; Write-Host (_Nave-Fg $script:NaveC.warn "Welcome banner desativado.") }
        default { Write-Host "Welcome banner: $(if ((_Nave-LoadSetting 'welcome' '0') -eq '1') {'on'} else {'off'}) (uso: nave-welcome on|off)" }
    }
}

function nave-help {
    $c = $script:NaveC
    $frm = _Nave-Fg $c.frame $script:CH_V
    Write-Host (_Nave-Fg $c.dir ("$($script:CH_TL)$($script:CH_H) Nave Terminal (PowerShell) — Guia Rapido " + ([string]$script:CH_H * 14)))
    Write-Host ("$frm " + (_Nave-Fg $c.ok "nave-theme <nome>    ") + "Paleta + fundo ao vivo (cyber|emerald|sunset|nord|stealth|dracula)")
    Write-Host ("$frm " + (_Nave-Fg $c.ok "nave-layout <modo>   ") + "Layout do prompt (framed|rail|hud|compact)")
    Write-Host ("$frm " + (_Nave-Fg $c.ok "nave-shader <modo>   ") + "Shader GPU Windows Terminal (glow|crt|off)")
    Write-Host ("$frm " + (_Nave-Fg $c.ok "nave-bg / welcome    ") + "Liga/desliga fundo OSC 11 ou welcome bar")
    Write-Host ("$frm " + (_Nave-Fg $c.ok "sysinfo / hr [tit]   ") + "Arte gradiente + sistema / regua divisoria")
    Write-Host ("$frm " + (_Nave-Fg $c.ok "nave-hud / nave-warp ") + "Cockpit em tempo real / starfield 3D")
    Write-Host ("$frm " + (_Nave-Fg $c.ok "ne / nave-edit <arq> ") + "Editor nativo Rust estilo VS Code (multi-cursor, Git)")
    Write-Host ("$frm " + (_Nave-Fg $c.ok "ginfo / ports / mkcd ") + "Git painel / portas TCP (+ killport) / mkcd")
    Write-Host (_Nave-Fg $c.dir ("$($script:CH_BL)" + ([string]$script:CH_H * 57)))
}

if ((_Nave-LoadSetting "welcome" "0") -eq "1" -and $Host.Name -eq "ConsoleHost") {
    $osW = Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue
    $dkW = Get-CimInstance Win32_LogicalDisk -Filter "DeviceID='C:'" -ErrorAction SilentlyContinue
    if ($osW -and $dkW) {
        $ramP = [int][Math]::Round((($osW.TotalVisibleMemorySize - $osW.FreePhysicalMemory) / $osW.TotalVisibleMemorySize) * 100)
        $dskPF = [Math]::Round((($dkW.Size - $dkW.FreeSpace) / $dkW.Size) * 100, 1)
        $dskP = [int][Math]::Round($dskPF)
        $c = $script:NaveC
        Write-Host ((_Nave-Fg $c.frame "$($script:CH_TL)$($script:CH_H) ") + (_Nave-Fg $c.dir "nave" -Bold) + "  RAM " + (_Nave-Bar $ramP 10) + " ${ramP}%  $($script:CH_MID)  SSD " + (_Nave-Bar $dskP 10) + " ${dskPF}%  $($script:CH_MID)  " + (_Nave-Fg $c.muted "digite 'nave-help' para atalhos"))
    }
}
