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

    function Step($n, $msg) { Write-Host "`n[$n/4] $msg" -ForegroundColor Cyan }
    function Info($msg)     { Write-Host "    $msg" -ForegroundColor DarkGray }
    function Ok($msg)       { Write-Host "    OK  $msg" -ForegroundColor Green }
    function Warn($msg)     { Write-Host "    !   $msg" -ForegroundColor Yellow }
    function Kv($k, $v)     { Write-Host ("    {0,-14} {1}" -f $k, $v) }
    function Die($msg) {
        Write-Host "`n  X  $msg`n" -ForegroundColor Red
        throw "moon installer aborted"
    }

    Write-Host ''
    Write-Host '  moon' -ForegroundColor Blue -NoNewline
    Write-Host '  chat with local language models, from your terminal' -ForegroundColor DarkGray
    Write-Host ''

    # -- 1. platform ---------------------------------------------------------

    Step 1 'Detecting platform'

    if ($PSVersionTable.PSVersion.Major -ge 6 -and -not $IsWindows) {
        Die 'This installer is for Windows. On Linux and macOS run install.sh instead.'
    }
    # Windows PowerShell 5.1 still defaults to TLS 1.0, which GitHub rejects
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $archRaw = $env:PROCESSOR_ARCHITEW6432
    if (-not $archRaw) { $archRaw = $env:PROCESSOR_ARCHITECTURE }
    switch ($archRaw) {
        'AMD64' { $arch = 'x86_64' }
        'ARM64' { $arch = 'x86_64'; Warn 'No native ARM64 build yet: installing the x86_64 binary, which Windows runs under emulation' }
        default { Die "Unsupported architecture: $archRaw" }
    }

    Kv 'OS:'           'windows'
    Kv 'Architecture:' $arch
    Ok 'Platform detected'

    # -- 2. version ----------------------------------------------------------

    Step 2 'Resolving version'

    $version = $env:MOON_VERSION
    if (-not $version) {
        Info 'Querying GitHub releases API...'
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
    $installDir   = if ($env:MOON_INSTALL_DIR) { $env:MOON_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\moon' }
    $exe          = Join-Path $installDir "$Binary.exe"

    # Detect an existing installation
    $current = $null
    if (Test-Path $exe) {
        try { $current = ((& $exe --version 2>$null) -split '\s+')[-1] } catch { $current = $null }
    }

    if (-not $current) {
        $mode = 'install'
        Kv 'Version:' "v$version"
    } elseif ($current -eq $version) {
        $mode = 'reinstall'
        Kv 'Version:' "v$version (already installed)"
    } else {
        $mode = 'upgrade'
        Kv 'Version:' "v$current  ->  v$version"
    }
    Kv 'Package:'    $package
    Kv 'Install to:' $exe
    Ok 'Version resolved'

    # -- 3. download & verify ------------------------------------------------

    Step 3 'Downloading'

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ('moon-install-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null

    try {
        $zip = Join-Path $tmp $package
        Info "URL: $url"
        try {
            Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
        } catch {
            Die "Download failed. Check that release v$version has the asset ${package}:`n  https://github.com/$Repo/releases/tag/v$version"
        }
        Ok "Downloaded $package"

        $sums = Join-Path $tmp 'checksums.txt'
        $haveSums = $true
        try { Invoke-WebRequest -Uri $checksumsUrl -OutFile $sums -UseBasicParsing } catch { $haveSums = $false }

        if (-not $haveSums) {
            Warn "checksums.txt not available for v$version"
        } else {
            $line = Get-Content $sums | Where-Object { $_ -match ('\s' + [regex]::Escape($package) + '$') } | Select-Object -First 1
            if (-not $line) {
                Warn "No checksum entry for $package - skipping"
            } else {
                $expected = (($line -split '\s+')[0]).ToLower()
                $actual   = (Get-FileHash -Path $zip -Algorithm SHA256).Hash.ToLower()
                if ($actual -ne $expected) {
                    Die "Checksum mismatch!`n  expected: $expected`n  got:      $actual"
                }
                Ok 'SHA-256 verified'
            }
        }

        # -- 4. install ------------------------------------------------------

        Step 4 'Installing'

        Info 'Extracting archive...'
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
            'upgrade'   { Ok "Upgraded    $exe  (v$current -> v$version)" }
            'reinstall' { Ok "Reinstalled $exe  (v$version)" }
            default     { Ok "Installed   $exe  (v$version)" }
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
        Write-Host ''
        Warn "Added $installDir to your user PATH. Open a new terminal to pick it up."
    }

    # -- done ----------------------------------------------------------------

    Write-Host ''
    Write-Host '  All done!' -ForegroundColor Green
    Write-Host ''
    Write-Host '  Start:   ' -NoNewline; Write-Host 'moon' -ForegroundColor Cyan
    Write-Host '  Ask:     ' -NoNewline; Write-Host 'moon ask "explain the borrow checker"' -ForegroundColor Cyan
    Write-Host '  Config:  ' -NoNewline; Write-Host 'moon config init' -ForegroundColor Cyan -NoNewline
    Write-Host '   (%USERPROFILE%\.config\moon\config.toml)' -ForegroundColor DarkGray
    Write-Host ''
    Write-Host '  moon talks to Ollama at http://localhost:11434 out of the box.' -ForegroundColor DarkGray
    Write-Host '  Have it running with a model pulled:  ollama pull qwen2.5-coder:14b' -ForegroundColor DarkGray
    Write-Host '  Use Windows Terminal or another terminal with ANSI colour support.' -ForegroundColor DarkGray
    Write-Host ''
}

Install-Moon
