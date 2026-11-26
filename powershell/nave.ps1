# ==============================================================================
# Nave Terminal for PowerShell (5.1 & 7+)
# Clean, fast, palette-driven, 100% universal (zero Nerd Fonts / broken emojis)
# ==============================================================================

[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::InputEncoding  = [System.Text.Encoding]::UTF8
$script:ESC = [char]27
$script:NaveConfigDir = Join-Path $env:USERPROFILE ".config\nave"
if (-not (Test-Path $script:NaveConfigDir)) {
    New-Item -ItemType Directory -Path $script:NaveConfigDir -Force | Out-Null
}

# Unicode constants (safe across all fonts)
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

$script:NaveFirstPrompt = $true
$script:NaveLastHistId = -1

$script:NavePalette = "cyber"
$palFile = Join-Path $script:NaveConfigDir "palette"
if (Test-Path $palFile) {
    $savedPal = (Get-Content $palFile -Raw).Trim()
    if ($savedPal) { $script:NavePalette = $savedPal }
}

$script:NavePalettes = @{
    cyber = @{
        frame="240"; user="81"; host="111"; dir="39"; git="141"
        warn="215"; info="81"; ok="78"; err="203"; dur="221"
        muted="242"; node="114"; docker="75"; path="117"; str="221"
        ps_cmd="#50FA7B"; ps_param="#7DCFFF"; ps_str="#E0AF68"; ps_var="#BB9AF7"; ps_err="#F7768E"
    }
    emerald = @{
        frame="239"; user="48"; host="79"; dir="42"; git="114"
        warn="221"; info="50"; ok="82"; err="203"; dur="185"
        muted="242"; node="119"; docker="44"; path="85"; str="192"
        ps_cmd="#5FFF00"; ps_param="#00FF87"; ps_str="#FFD75F"; ps_var="#87D787"; ps_err="#FF5F5F"
    }
    sunset = @{
        frame="240"; user="215"; host="209"; dir="204"; git="177"
        warn="221"; info="216"; ok="114"; err="196"; dur="228"
        muted="243"; node="150"; docker="111"; path="217"; str="222"
        ps_cmd="#87D787"; ps_param="#FFAF5F"; ps_str="#FFD75F"; ps_var="#D787FF"; ps_err="#FF5F5F"
    }
    nord = @{
        frame="240"; user="110"; host="109"; dir="116"; git="146"
        warn="222"; info="110"; ok="108"; err="174"; dur="223"
        muted="243"; node="108"; docker="110"; path="152"; str="223"
        ps_cmd="#A3BE8C"; ps_param="#88C0D0"; ps_str="#EBCB8B"; ps_var="#B48EAD"; ps_err="#BF616A"
    }
    stealth = @{
        frame="238"; user="250"; host="245"; dir="255"; git="248"
        warn="221"; info="250"; ok="252"; err="203"; dur="246"
        muted="240"; node="247"; docker="247"; path="252"; str="249"
        ps_cmd="#E5E9F0"; ps_param="#D8DEE9"; ps_str="#BCBCBC"; ps_var="#A8A8A8"; ps_err="#FF5F5F"
    }
}

$script:NaveC = $script:NavePalettes["cyber"]

function _Nave-Fg([string]$code, [string]$text, [switch]$Bold) {
    $b = if ($Bold) { ";1" } else { "" }
    return "$($script:ESC)[38;5;${code}${b}m${text}$($script:ESC)[0m"
}

function _Nave-ApplyPalette([string]$pal) {
    if (-not $script:NavePalettes.ContainsKey($pal)) {
        Write-Host (_Nave-Fg "203" "Paleta desconhecida: $pal (opcoes: cyber, emerald, sunset, nord, stealth)")
        return $false
    }
    $script:NavePalette = $pal
    $script:NaveC = $script:NavePalettes[$pal]

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

# Configure PSReadLine (autosuggestions + history prefix search + no bell)
if (Get-Module -ListAvailable -Name PSReadLine) {
    Import-Module PSReadLine -ErrorAction SilentlyContinue
    try { Set-PSReadLineOption -BellStyle None -ErrorAction SilentlyContinue } catch {}
    try { Set-PSReadLineOption -PredictionSource History -ErrorAction SilentlyContinue } catch {}
    try {
        Set-PSReadLineKeyHandler -Key UpArrow -Function HistorySearchBackward
        Set-PSReadLineKeyHandler -Key DownArrow -Function HistorySearchForward
    } catch {}
}

[void](_Nave-ApplyPalette $script:NavePalette)

function nave-theme {
    param([string]$Name)
    if ([string]::IsNullOrWhiteSpace($Name) -or $Name -eq "list") {
        $c = $script:NaveC
        Write-Host (_Nave-Fg $c.dir "$($script:CH_TL)$($script:CH_H) Paletas disponiveis (atual: $($script:NavePalette))")
        foreach ($p in @("cyber", "emerald", "sunset", "nord", "stealth")) {
            $mark = if ($p -eq $script:NavePalette) { "$($script:CH_ARR) " } else { "  " }
            Write-Host ((_Nave-Fg $c.frame "$($script:CH_V) ") + $mark + (_Nave-Fg $c.ok $p -Bold))
        }
        Write-Host (_Nave-Fg $c.dir "$($script:CH_BL)$($script:CH_H) Uso: nave-theme <nome>")
        return
    }
    if (_Nave-ApplyPalette $Name) {
        Set-Content -Path (Join-Path $script:NaveConfigDir "palette") -Value $Name -NoNewline
        Write-Host (_Nave-Fg $script:NaveC.ok "$($script:CH_CHK) Paleta alterada para $Name" -Bold)
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
    if (-not $branch) {
        $branch = (& git rev-parse --short HEAD 2>$null)
        if ($branch) { $branch = "@$branch" }
    }
    if (-not $branch) { return $null }

    $action = ""
    if ((Test-Path (Join-Path $gitDir "rebase-merge")) -or (Test-Path (Join-Path $gitDir "rebase-apply"))) {
        $action = "REBASE"
    } elseif (Test-Path (Join-Path $gitDir "MERGE_HEAD")) {
        $action = "MERGE"
    } elseif (Test-Path (Join-Path $gitDir "CHERRY_PICK_HEAD")) {
        $action = "CHERRY-PICK"
    }

    $statusLines = @(& git status --porcelain=v1 --branch 2>$null)
    $aheadBehind = ""
    if ($statusLines.Count -gt 0) {
        if ($statusLines[0] -match 'ahead (\d+)')  { $aheadBehind += "$($script:CH_UP)$($Matches[1])" }
        if ($statusLines[0] -match 'behind (\d+)') { $aheadBehind += "$($script:CH_DN)$($Matches[1])" }
    }

    $staged = ""; $dirty = ""; $untracked = ""
    foreach ($line in $statusLines) {
        if ($line.StartsWith("## ")) { continue }
        if ($line.Length -lt 2) { continue }
        $x = $line[0]; $y = $line[1]
        if ($x -eq '?') {
            $untracked = "?"
        } else {
            if ($x -ne ' ') { $staged = "+" }
            if ($y -ne ' ') { $dirty = "*" }
        }
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
    if ($env:VIRTUAL_ENV) {
        $parts += _Nave-Fg $c.ok ("venv:" + (Split-Path $env:VIRTUAL_ENV -Leaf))
    } elseif ($env:CONDA_DEFAULT_ENV) {
        $parts += _Nave-Fg $c.ok ("conda:" + $env:CONDA_DEFAULT_ENV)
    }
    if ((Test-Path "package.json") -and (Get-Command node -ErrorAction SilentlyContinue)) {
        $nver = (& node -v 2>$null)
        if ($nver) { $parts += _Nave-Fg $c.node "node:$nver" }
    }
    if ((Test-Path "Dockerfile") -or (Test-Path "docker-compose.yml") -or (Test-Path "docker-compose.yaml") -or (Test-Path "compose.yaml")) {
        $parts += _Nave-Fg $c.docker "[docker]"
    }
    return ($parts -join "  ")
}

function prompt {
    $lastOk = $?
    $lastCode = $global:LASTEXITCODE

    if ($script:NaveFirstPrompt) {
        $script:NaveFirstPrompt = $false
    } else {
        Write-Host ""
    }

    # Duration from last executed PowerShell history item
    $durStr = ""
    $hist = Get-History -Count 1 -ErrorAction SilentlyContinue
    if ($hist -and $hist.Id -ne $script:NaveLastHistId) {
        $script:NaveLastHistId = $hist.Id
        $elapsed = ($hist.EndExecutionTime - $hist.StartExecutionTime).TotalSeconds
        $durStr = _Nave-FormatDuration $elapsed
    }

    $c = $script:NaveC
    $isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    $uColor = if ($isAdmin) { $c.err } else { $c.user }

    $userHost = (_Nave-Fg $uColor $env:USERNAME.ToLower()) + (_Nave-Fg $c.muted "@") + (_Nave-Fg $c.host $env:COMPUTERNAME.ToLower())
    $dirShort = _Nave-ShortPath (Get-Location).Path
    $dirPart  = _Nave-Fg $c.dir $dirShort -Bold

    $line1 = (_Nave-Fg $c.frame "$($script:CH_TL)$($script:CH_H)") + " $userHost  $dirPart"
    $plainLen = 3 + $env:USERNAME.Length + 1 + $env:COMPUTERNAME.Length + 2 + $dirShort.Length

    $gitPart = _Nave-GitInfo
    if ($gitPart) {
        $line1 += "  $gitPart"
        $plainLen += 14
    }

    $ctxPart = _Nave-ContextInfo
    if ($ctxPart) {
        $line1 += "  $ctxPart"
        $plainLen += 14
    }

    if ($durStr) {
        $line1 += "  " + (_Nave-Fg $c.dur $durStr)
        $plainLen += 2 + $durStr.Length
    }

    $hasErr = (-not $lastOk) -or ($lastCode -and $lastCode -ne 0)
    if ($hasErr) {
        $codeDisp = if ($lastCode -and $lastCode -ne 0) { $lastCode } else { 1 }
        $line1 += "  " + (_Nave-Fg $c.err "$($script:CH_MUL)$codeDisp" -Bold)
        $plainLen += 4
    }

    # Right-aligned timestamp on line 1 when terminal is wide enough
    $timeText = "[" + (Get-Date -Format "HH:mm:ss") + "]"
    $winW = 0
    try { $winW = [Console]::WindowWidth } catch {}
    if ($winW -gt ($plainLen + $timeText.Length + 6)) {
        $gap = $winW - $plainLen - $timeText.Length - 2
        if ($gap -gt 2) {
            $line1 += (" " * $gap) + (_Nave-Fg $c.muted $timeText)
        }
    }

    $arrColor = if ($hasErr) { $c.err } else { $c.ok }
    $line2 = (_Nave-Fg $c.frame "$($script:CH_BL)$($script:CH_H)") + " " + (_Nave-Fg $arrColor $script:CH_ARR -Bold) + " "

    $Host.UI.RawUI.WindowTitle = "$($env:USERNAME)@$($env:COMPUTERNAME): $dirShort"
    return "$line1`n$line2"
}

# ------------------------------------------------------------------------------
# Toolkit Commands (sysinfo, ginfo, ports, killport, mkcd, nave-welcome, nave-help)
# ------------------------------------------------------------------------------
function _Nave-Bar([int]$pct, [int]$width = 16) {
    if ($pct -lt 0) { $pct = 0 }
    if ($pct -gt 100) { $pct = 100 }
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

    $totalDisk = [Math]::Round($disk.Size / 1GB, 0)
    $usedDisk  = [Math]::Round(($disk.Size - $disk.FreeSpace) / 1GB, 0)
    $diskPct   = [int][Math]::Round(($usedDisk / $totalDisk) * 100)

    $upSpan = (Get-Date) - $os.LastBootUpTime
    $upStr  = "$([int]$upSpan.TotalHours)h $($upSpan.Minutes)m"
    $tcpCount = (Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue).Count

    $frm = _Nave-Fg $c.frame $script:CH_V
    Write-Host ((_Nave-Fg $c.dir "$($script:CH_TL)$($script:CH_H) $($env:USERNAME)@$($env:COMPUTERNAME) ") + (_Nave-Fg $c.frame "[paleta: $($script:NavePalette)]") + (_Nave-Fg $c.dir (" " + ([string]$script:CH_H * 18))))
    Write-Host "$frm " (_Nave-Fg $c.git "OS:      ") $os.Caption
    Write-Host "$frm " (_Nave-Fg $c.git "Shell:   ") "PowerShell $($PSVersionTable.PSVersion)"
    Write-Host "$frm " (_Nave-Fg $c.git "Uptime:  ") $upStr
    Write-Host "$frm " (_Nave-Fg $c.git "Memory:  ") (_Nave-Bar $ramPct 16) " ${usedRam}Gi / ${totalRam}Gi (${ramPct}%)"
    Write-Host "$frm " (_Nave-Fg $c.git "Disk C:: ") (_Nave-Bar $diskPct 16) " ${usedDisk}G / ${totalDisk}G (${diskPct}%)"
    Write-Host "$frm " (_Nave-Fg $c.git "Network: ") "$tcpCount portas TCP em escuta"
    Write-Host (_Nave-Fg $c.dir ("$($script:CH_BL)" + ([string]$script:CH_H * 52)))
}

function ginfo {
    if (-not (Get-Command git -ErrorAction SilentlyContinue)) { Write-Host "Git nao encontrado."; return }
    $top = (& git rev-parse --show-toplevel 2>$null)
    if ($LASTEXITCODE -ne 0 -or -not $top) { Write-Host "Nao e um repositorio Git."; return }
    $c = $script:NaveC
    $repo = Split-Path $top -Leaf
    $br = (& git symbolic-ref --short HEAD 2>$null)
    if (-not $br) { $br = (& git rev-parse --short HEAD 2>$null) }
    $remote = (& git remote get-url origin 2>$null)
    if (-not $remote) { $remote = "sem remote" }
    $stashes = @(& git stash list 2>$null).Count
    $changed = @(& git status --porcelain 2>$null).Count

    $frm = _Nave-Fg $c.frame $script:CH_V
    Write-Host (_Nave-Fg $c.dir ("$($script:CH_TL)$($script:CH_H) Git: $repo " + ([string]$script:CH_H * 34)))
    Write-Host "$frm " (_Nave-Fg $c.git "Branch: ") "$br  " (_Nave-Fg $c.muted "(alterados: $changed, stashes: $stashes)")
    Write-Host "$frm " (_Nave-Fg $c.git "Remote: ") $remote
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
    Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue |
        Sort-Object LocalPort -Unique |
        ForEach-Object {
            $proc = (Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue).ProcessName
            if (-not $proc) { $proc = "-" }
            Write-Host ("  {0,-20} {1,-8} {2}" -f "$($_.LocalAddress):$($_.LocalPort)", $_.OwningProcess, $proc)
        }
    Write-Host (_Nave-Fg $c.dir ("$($script:CH_BL)" + ([string]$script:CH_H * 57)))
}

function killport {
    param([Parameter(Mandatory=$true)][int]$Port)
    $conns = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue
    if (-not $conns) {
        Write-Host (_Nave-Fg $script:NaveC.warn "Nenhum processo encontrado na porta $Port.")
        return
    }
    foreach ($pidVal in ($conns.OwningProcess | Select-Object -Unique)) {
        $pname = (Get-Process -Id $pidVal -ErrorAction SilentlyContinue).ProcessName
        try {
            Stop-Process -Id $pidVal -Force -ErrorAction Stop
            Write-Host (_Nave-Fg $script:NaveC.ok "$($script:CH_CHK) Finalizado PID $pidVal ($pname) na porta $Port.")
        } catch {
            Write-Host (_Nave-Fg $script:NaveC.err "$($script:CH_MUL) Erro ao finalizar PID $pidVal ($pname).")
        }
    }
}

function mkcd {
    param([Parameter(Mandatory=$true)][string]$Path)
    New-Item -ItemType Directory -Path $Path -Force | Out-Null
    Set-Location $Path
}

function which {
    param([Parameter(Mandatory=$true)][string]$Name)
    (Get-Command $Name -ErrorAction SilentlyContinue).Source
}

function nave-welcome {
    param([string]$Mode)
    $f = Join-Path $script:NaveConfigDir "welcome"
    switch ($Mode) {
        "on"  { Set-Content $f "1" -NoNewline; Write-Host (_Nave-Fg $script:NaveC.ok "Welcome banner ativado.") }
        "off" { Set-Content $f "0" -NoNewline; Write-Host (_Nave-Fg $script:NaveC.warn "Welcome banner desativado.") }
        default {
            $st = if ((Test-Path $f) -and ((Get-Content $f -Raw).Trim() -eq "0")) { "off" } else { "on" }
            Write-Host "Welcome banner: $st (uso: nave-welcome on|off)"
        }
    }
}

function nave-help {
    $c = $script:NaveC
    $frm = _Nave-Fg $c.frame $script:CH_V
    Write-Host (_Nave-Fg $c.dir ("$($script:CH_TL)$($script:CH_H) Nave Terminal (PowerShell) — Guia Rapido " + ([string]$script:CH_H * 14)))
    Write-Host "$frm " (_Nave-Fg $c.ok "nave-theme <nome>    ") "Troca paleta (cyber|emerald|sunset|nord|stealth)"
    Write-Host "$frm " (_Nave-Fg $c.ok "nave-welcome on|off  ") "Barra compacta de RAM/SSD ao abrir"
    Write-Host "$frm " (_Nave-Fg $c.ok "sysinfo / ginfo      ") "Painel do sistema / painel do repositorio Git"
    Write-Host "$frm " (_Nave-Fg $c.ok "ports / killport <p> ") "Lista portas TCP abertas e encerra processo"
    Write-Host "$frm " (_Nave-Fg $c.ok "mkcd <dir> / which   ") "Cria e entra na pasta / localiza executavel"
    Write-Host (_Nave-Fg $c.dir ("$($script:CH_BL)" + ([string]$script:CH_H * 57)))
}

# Compact greeting on startup
$welcomeFile = Join-Path $script:NaveConfigDir "welcome"
$showWelcome = -not ((Test-Path $welcomeFile) -and ((Get-Content $welcomeFile -Raw).Trim() -eq "0"))
if ($showWelcome -and $Host.Name -eq "ConsoleHost") {
    $osW = Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue
    $dkW = Get-CimInstance Win32_LogicalDisk -Filter "DeviceID='C:'" -ErrorAction SilentlyContinue
    if ($osW -and $dkW) {
        $ramP = [int][Math]::Round((($osW.TotalVisibleMemorySize - $osW.FreePhysicalMemory) / $osW.TotalVisibleMemorySize) * 100)
        $dskP = [int][Math]::Round((($dkW.Size - $dkW.FreeSpace) / $dkW.Size) * 100)
        $c = $script:NaveC
        Write-Host ((_Nave-Fg $c.frame "$($script:CH_TL)$($script:CH_H) ") + (_Nave-Fg $c.dir "nave" -Bold) + "  RAM " + (_Nave-Bar $ramP 10) + " ${ramP}%  $($script:CH_MID)  SSD " + (_Nave-Bar $dskP 10) + " ${dskP}%  $($script:CH_MID)  " + (_Nave-Fg $c.muted "digite 'nave-help' para atalhos"))
    }
}
