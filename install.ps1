#!/usr/bin/env pwsh
# moon installer for Windows
#
# Usage (latest):
#   irm https://raw.githubusercontent.com/towerforge/moon/main/install.ps1 | iex
#
# Env overrides:
#   $env:MOON_VERSION     = "0.2.0"           install a specific version
#   $env:MOON_INSTALL_DIR = "C:\tools\moon"   install to a custom directory
#                                             (default: %LOCALAPPDATA%\Programs\moon)
#   $env:MOON_FORCE       = "1"               install even if moon is already
#                                             there and can update itself
#   $env:NO_COLOR         = "1"               no colour, whatever the console is
#
# Everything lives inside Install-Moon so that `irm | iex` leaves no variables
# or preferences behind in the calling session.

function Install-Moon {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest crawls with the progress bar on

    $Repo     = 'towerforge/moon'
    $Binary   = 'moon'
    $Api      = "https://api.github.com/repos/$Repo"
    $Releases = "https://github.com/$Repo/releases/download"

    # -- the Moon palette ----------------------------------------------------
    # The same tokens as the TUI (crates/tui/src/theme.rs). Windows Terminal and
    # PowerShell 7 take the 24-bit escapes and the box drawing that goes with
    # them; an older console gets the nearest of its sixteen colours and ASCII,
    # which is also what survives a 5.1 that read this file as ANSI.

    $esc   = [char]27
    $fancy = [bool](($env:WT_SESSION -or $PSVersionTable.PSVersion.Major -ge 7) -and -not $env:NO_COLOR)
    $reset = if ($fancy) { "$($esc)[0m" } else { '' }
    $tok = @{
        moon  = @{ ansi = "$($esc)[38;2;143;184;255m"; console = 'Cyan' }
        ink   = @{ ansi = "$($esc)[38;2;243;236;227m"; console = 'White' }
        muted = @{ ansi = "$($esc)[38;2;169;167;184m"; console = 'Gray' }
        line  = @{ ansi = "$($esc)[38;2;74;74;96m";    console = 'DarkGray' }
        ok    = @{ ansi = "$($esc)[38;2;143;217;160m"; console = 'Green' }
        alert = @{ ansi = "$($esc)[38;2;255;143;143m"; console = 'Red' }
    }
    $dash  = if ($fancy) { [char]0x2504 } else { '-' }
    $dot   = if ($fancy) { [char]0x00B7 } else { '-' }
    $tick  = if ($fancy) { [char]0x2713 } else { '+' }
    $cross = if ($fancy) { [char]0x2717 } else { 'X' }

    function Paint($text, $token, [switch]$NoNewline) {
        if ($fancy) {
            Write-Host "$($tok[$token].ansi)$text$reset" -NoNewline:$NoNewline
        } else {
            Write-Host $text -ForegroundColor $tok[$token].console -NoNewline:$NoNewline
        }
    }

    # -- the pieces every block is drawn with --------------------------------

    $ruleW = 66
    $cmdW  = 24

    function Rule($num, $title) {
        $label = if ($num) { "$num $dot $title" } else { $title }
        $fill  = [Math]::Max(0, $ruleW - $label.Length - 3)
        Write-Host ''
        Paint "  $dash " 'line' -NoNewline
        Paint $label 'moon' -NoNewline
        Paint (' ' + ([string]$dash * $fill)) 'line'
    }

    function Kv($k, $v) {
        Paint ('     ' + $k.PadRight(13)) 'muted' -NoNewline
        Paint $v 'ink'
    }
    function Note($msg) { Paint "     $msg" 'muted' }
    function Cmd($c, $what) {
        Paint ('     ' + $c.PadRight($cmdW)) 'moon' -NoNewline
        Paint $what 'muted'
    }
    function Ok($msg)   { Paint "     $tick " 'ok' -NoNewline; Paint $msg 'ink' }
    function Warn($msg) { Paint "     ! " 'alert' -NoNewline; Paint $msg 'muted' }
    function Die($msg) {
        Write-Host ''
        Paint "  $cross $msg" 'alert'
        Write-Host ''
        throw "moon installer aborted"
    }

    # -- the mark, the same four rows the TUI opens with ---------------------

    Write-Host ''
    if ($fancy) {
        # half blocks: each character is two pixels of the crescent, stacked
        Paint '   ▄█     ' 'moon' -NoNewline; Paint '   moon' 'moon'
        Paint '  ███     ' 'moon' -NoNewline; Paint '   chat with local language models,' 'muted'
        Paint '  ████▄▄▄█' 'moon' -NoNewline; Paint '   from your terminal' 'muted'
        Paint '   ▀████▀ ' 'moon' -NoNewline; Paint "   installer $dot github.com/$Repo" 'line'
    } else {
        Paint '  moon' 'moon' -NoNewline
        Paint '  chat with local language models, from your terminal' 'muted'
        Paint "  installer - github.com/$Repo" 'line'
    }

    # -- 1. platform ---------------------------------------------------------

    Rule 1 'platform'

    if ($PSVersionTable.PSVersion.Major -ge 6 -and -not $IsWindows) {
        Die 'This installer is for Windows. On Linux and macOS run install.sh instead.'
    }
    # Windows PowerShell 5.1 still defaults to TLS 1.0, which GitHub rejects
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $archRaw = $env:PROCESSOR_ARCHITEW6432
    if (-not $archRaw) { $archRaw = $env:PROCESSOR_ARCHITECTURE }
    switch ($archRaw) {
        'AMD64' { $arch = 'x86_64' }
        'ARM64' { $arch = 'x86_64' }
        default { Die "Unsupported architecture: $archRaw" }
    }

    Kv 'os'   'windows'
    Kv 'arch' $arch
    if ($archRaw -eq 'ARM64') {
        Warn 'no native ARM64 build yet: the x86_64 binary runs under emulation'
    }

    # -- 2. release ----------------------------------------------------------

    Rule 2 'release'

    $installDir = if ($env:MOON_INSTALL_DIR) { $env:MOON_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\moon' }
    $exe        = Join-Path $installDir "$Binary.exe"

    # Detect an existing installation, here or anywhere on the PATH
    $current   = $null
    $existing  = $null
    $onPathExe = (Get-Command "$Binary.exe" -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    foreach ($candidate in @($exe, $onPathExe)) {
        if ($candidate -and (Test-Path $candidate)) {
            $existing = $candidate
            try { $current = ((& $candidate --version 2>$null) -split '\s+')[-1] } catch { $current = $null }
            break
        }
    }

    # moon updates itself, so this script is for the first install. Whether the
    # moon that is there can do it is asked of the binary and not of its version
    # number: one from before `moon update` existed is upgraded here as always.
    $selfUpdates = $false
    if ($current) {
        try {
            & $existing update --help *> $null
            $selfUpdates = ($LASTEXITCODE -eq 0)
        } catch { $selfUpdates = $false }
    }

    if ($current -and $selfUpdates -and -not $env:MOON_FORCE) {
        Kv 'version' "v$current"
        Kv 'path'    $existing
        Ok 'moon is already installed'
        Write-Host ''
        Paint '     It updates itself.' 'ink' -NoNewline
        Paint ' From here on:' 'muted'
        Write-Host ''
        Cmd 'moon update'            'the latest release'
        Cmd 'moon update --check'    'is there a new one?'
        Cmd 'moon update --to 0.2.0' 'that version, downgrades included'
        Cmd 'moon update --force'    'reinstall the one you have'
        Write-Host ''
        Note 'to install with this script anyway:'
        Note '$env:MOON_FORCE = "1"; irm https://raw.githubusercontent.com/towerforge/moon/main/install.ps1 | iex'
        Write-Host ''
        return
    }

    $version = $env:MOON_VERSION
    if (-not $version) {
        Note 'asking github for the latest release...'
        try {
            $latest = Invoke-RestMethod -Uri "$Api/releases/latest" -Headers @{ 'User-Agent' = 'moon-installer' } -UseBasicParsing
        } catch {
            Die 'Could not fetch the latest version from GitHub'
        }
        $version = $latest.tag_name -replace '^v', ''
        if (-not $version) { Die "No release found at https://github.com/$Repo/releases" }
    }

    $package      = "$Binary-windows-$arch.zip"
    $url          = "$Releases/v$version/$package"
    $checksumsUrl = "$Releases/v$version/checksums.txt"

    if (-not $current) {
        $mode = 'install'
        Kv 'version' "v$version"
    } elseif ($current -eq $version) {
        $mode = 'reinstall'
        Kv 'version' "v$version  (already installed)"
    } else {
        $mode = 'upgrade'
        Kv 'version' "v$current  ->  v$version"
    }
    Kv 'package'    $package
    Kv 'install to' $exe

    # -- 3. download ---------------------------------------------------------

    Rule 3 'download'

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ('moon-install-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null

    try {
        $zip = Join-Path $tmp $package
        Note $url
        try {
            Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
        } catch {
            Die "Download failed. Check that release v$version has the asset ${package}:`n  https://github.com/$Repo/releases/tag/v$version"
        }
        $size = '{0:N1} MB' -f ((Get-Item $zip).Length / 1MB)
        Ok "downloaded $package  ($size)"

        $sums = Join-Path $tmp 'checksums.txt'
        $haveSums = $true
        try { Invoke-WebRequest -Uri $checksumsUrl -OutFile $sums -UseBasicParsing } catch { $haveSums = $false }

        if (-not $haveSums) {
            Warn "checksums.txt not published for v${version}: not verified"
        } else {
            $line = Get-Content $sums | Where-Object { $_ -match ('\s' + [regex]::Escape($package) + '$') } | Select-Object -First 1
            if (-not $line) {
                Warn "no checksum entry for ${package}: not verified"
            } else {
                $expected = (($line -split '\s+')[0]).ToLower()
                $actual   = (Get-FileHash -Path $zip -Algorithm SHA256).Hash.ToLower()
                if ($actual -ne $expected) {
                    Die "Checksum mismatch!`n  expected: $expected`n  got:      $actual"
                }
                Ok 'sha-256 verified'
            }
        }

        # -- 4. install ------------------------------------------------------

        Rule 4 'install'

        Expand-Archive -Path $zip -DestinationPath $tmp -Force
        $extracted = Join-Path $tmp "$Binary.exe"
        if (-not (Test-Path $extracted)) { Die "Binary '$Binary.exe' not found inside the archive" }

        New-Item -ItemType Directory -Path $installDir -Force | Out-Null
        try {
            Copy-Item -Path $extracted -Destination $exe -Force
        } catch {
            Die "Could not write $exe. Is moon running? Close it and try again."
        }

        switch ($mode) {
            'upgrade'   { Ok "$exe  (v$current -> v$version)" }
            'reinstall' { Ok "$exe  (reinstalled v$version)" }
            default     { Ok "$exe  (v$version)" }
        }
    } finally {
        Remove-Item -Path $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }

    # PATH: add the install dir to the user PATH once, and to this session
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not $userPath) { $userPath = '' }
    $onPath = ($userPath -split ';') -contains $installDir -or ($env:Path -split ';') -contains $installDir
    if (-not $onPath) {
        $newPath = if ($userPath) { "$userPath;$installDir" } else { $installDir }
        [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
        $env:Path = "$env:Path;$installDir"
        Warn "added $installDir to your user PATH: open a new terminal to pick it up"
    }

    # -- ready ---------------------------------------------------------------

    Rule $null 'ready'

    Cmd 'moon'             'start chatting'
    Cmd 'moon ask "..."'   'one question, straight to stdout'
    Cmd 'moon config init' 'writes %USERPROFILE%\.config\moon\config.toml'
    Cmd 'moon update'      'when there is a new release'
    Write-Host ''
    Note 'moon talks to Ollama at http://localhost:11434 out of the box.'
    Note 'Have it running with a model pulled:  ollama pull qwen2.5-coder:14b'
    Note 'Use Windows Terminal or another terminal with ANSI colour support.'
    Write-Host ''
}

Install-Moon
