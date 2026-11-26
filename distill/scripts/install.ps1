# distill one-click installer (Windows)
# Usage: irm https://synology-minio.youxi123.com/starlink-artifacts/distill/install.ps1 | iex
#
# Overridable environment variables:
#   INSTALL_DIR       default $env:USERPROFILE\.snappywatt\bin
#   SNAPPYWATT_HOME   default $env:USERPROFILE\.snappywatt
#   BINARY_NAME       downloaded binary name (default distill)
#   INSTALL_NAME      installed command name (default same as BINARY_NAME)
#   VERSION           release channel/tag (default latest; pin e.g. $env:VERSION='v1.1.1')
#   S3_BASE/S3_BUCKET/S3_PATH   private mirror overrides

& {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'

    $S3_BASE      = if ($env:S3_BASE)      { $env:S3_BASE }      else { 'https://synology-minio.youxi123.com' }
    $S3_BUCKET    = if ($env:S3_BUCKET)    { $env:S3_BUCKET }    else { 'starlink-artifacts' }
    $S3_PATH      = if ($env:S3_PATH)      { $env:S3_PATH }      else { 'distill' }
    $BINARY_NAME  = if ($env:BINARY_NAME)  { $env:BINARY_NAME }  else { 'distill' }
    $INSTALL_NAME = if ($env:INSTALL_NAME) { $env:INSTALL_NAME } else { $BINARY_NAME }
    $VERSION      = if ($env:VERSION)      { $env:VERSION }      else { 'latest' }

    $SNAPPYWATT_HOME = if ($env:SNAPPYWATT_HOME) { $env:SNAPPYWATT_HOME } else { Join-Path $env:USERPROFILE '.snappywatt' }
    $INSTALL_DIR     = if ($env:INSTALL_DIR)     { $env:INSTALL_DIR }     else { Join-Path $SNAPPYWATT_HOME 'bin' }

    function Info($msg) { Write-Host "[INFO] $msg" -ForegroundColor Green }
    function Warn($msg) { Write-Host "[WARN] $msg" -ForegroundColor Yellow }
    function Die($msg)  { Write-Host "[ERROR] $msg" -ForegroundColor Red; exit 1 }

    function Get-Platform {
        $arch = $env:PROCESSOR_ARCHITEW6432
        if (-not $arch) { $arch = $env:PROCESSOR_ARCHITECTURE }
        switch ($arch) {
            'AMD64' { return 'x86_64-pc-windows-msvc' }
            'ARM64' { Die "aarch64-pc-windows-msvc has no build artifact yet" }
            'x86'   { Die "32-bit Windows is not supported (need 64-bit)" }
            default { Die "Unsupported Windows arch: '$arch'" }
        }
    }

    function Download-Binary {
        param([string]$Platform, [string]$OutPath)
        $url = "${S3_BASE}/${S3_BUCKET}/${S3_PATH}/${VERSION}/${Platform}/${BINARY_NAME}.gz"
        $cacheBust = "?t=$([DateTimeOffset]::Now.ToUnixTimeSeconds())"

        Info "Platform: $Platform"
        Info "Download: $url"

        $tmpGz = "$OutPath.gz"
        try {
            Invoke-WebRequest -Uri "$url$cacheBust" -OutFile $tmpGz -UseBasicParsing
        } catch {
            Die "Download failed: $_"
        }

        Info "Extracting..."
        $in = $null; $gz = $null; $out = $null
        try {
            $in  = [System.IO.File]::OpenRead($tmpGz)
            $gz  = New-Object System.IO.Compression.GZipStream($in, [System.IO.Compression.CompressionMode]::Decompress)
            $out = [System.IO.File]::Create($OutPath)
            $gz.CopyTo($out)
        } catch {
            Die "Extract failed: $_"
        } finally {
            if ($out) { $out.Dispose() }
            if ($gz)  { $gz.Dispose() }
            if ($in)  { $in.Dispose() }
        }
        Remove-Item $tmpGz -Force -ErrorAction SilentlyContinue
    }

    function Install-Binary {
        param([string]$SrcPath)
        if (-not (Test-Path $INSTALL_DIR)) {
            New-Item -ItemType Directory -Path $INSTALL_DIR -Force | Out-Null
        }
        $dst = Join-Path $INSTALL_DIR "$INSTALL_NAME.exe"
        Info "Installing to: $dst"
        Move-Item -Force -Path $SrcPath -Destination $dst
    }

    function Ensure-UserPath {
        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        if ([string]::IsNullOrEmpty($userPath)) { $userPath = '' }
        $segments = $userPath -split ';' | Where-Object { $_ -ne '' }

        if ($segments -notcontains $INSTALL_DIR) {
            $newPath = ($segments + $INSTALL_DIR) -join ';'
            [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
            Info "Added to User PATH: $INSTALL_DIR"
            Warn "Open a new terminal window for PATH to take effect"
        } else {
            Info "User PATH already contains $INSTALL_DIR"
        }

        if (($env:Path -split ';') -notcontains $INSTALL_DIR) {
            $env:Path = "$env:Path;$INSTALL_DIR"
        }
    }

    Write-Host ""
    Write-Host "=========================================="
    Write-Host "    $INSTALL_NAME installer (Windows)"
    Write-Host "=========================================="
    Write-Host ""

    $platform = Get-Platform
    $tmpExe = Join-Path ([System.IO.Path]::GetTempPath()) "$INSTALL_NAME-install-$([Guid]::NewGuid().ToString('N')).exe"

    try {
        Download-Binary -Platform $platform -OutPath $tmpExe
        Install-Binary -SrcPath $tmpExe
        Ensure-UserPath

        $exe = Join-Path $INSTALL_DIR "$INSTALL_NAME.exe"
        try {
            $ver = & $exe --version 2>&1
            Info "Installed successfully: $ver"
        } catch {
            Warn "Installed to $exe, but --version call failed (verify manually)"
        }
        Info "Install dir: $INSTALL_DIR"
        Info "Custom path: `$env:INSTALL_DIR='D:\tools'; irm ${S3_BASE}/${S3_BUCKET}/${S3_PATH}/install.ps1 | iex"
    } finally {
        Remove-Item -Force -ErrorAction SilentlyContinue $tmpExe, "$tmpExe.gz"
    }
}
