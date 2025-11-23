param(
    [string] $InstallDir,
    [string] $Repo = 'LeagueToolkit/wadtools'
)

$ErrorActionPreference = 'Stop'

function Write-Info {
    param([string] $Message)
    Write-Host "[wadtools-installer] $Message"
}

function Ensure-Tls12 {
    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    } catch {
        # Ignore if not supported
    }
}

function Get-DefaultInstallDir {
    if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        return Join-Path $env:USERPROFILE '.wadtools\bin'
    }
    return Join-Path $env:LOCALAPPDATA 'wadtools\bin'
}

function Test-PathInUserEnv {
    param([string] $Dir)
    $current = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ([string]::IsNullOrWhiteSpace($current)) { return $false }
    $segments = $current.Split(';') | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
    return $segments | Where-Object { $_.TrimEnd('\\') -ieq $Dir.TrimEnd('\\') } | ForEach-Object { $true } | Select-Object -First 1
}

function Add-PathForCurrentUser {
    param([string] $Dir)
    if (Test-PathInUserEnv -Dir $Dir) {
        return
    }
    $current = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ([string]::IsNullOrWhiteSpace($current)) {
        $newPath = $Dir
    } else {
        $needsSep = $current.Trim().Length -gt 0 -and -not $current.Trim().EndsWith(';')
        if ($needsSep) { $current = "$current;" }
        $newPath = "$current$Dir"
    }
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    # Update current session too
    if (-not ($env:Path.Split(';') | Where-Object { $_.TrimEnd('\\') -ieq $Dir.TrimEnd('\\') })) {
        $env:Path = "$env:Path;$Dir"
    }
}

function Resolve-Asset {
    param([object[]] $Assets)
    if (-not $Assets -or $Assets.Count -eq 0) { return $null }
    # Prefer a direct Windows .exe first
    $exe = $Assets | Where-Object { $_.name -match '(?i)^(wadtools).*?(win|windows).*?\.exe$' } | Select-Object -First 1
    if ($exe) { return $exe }
    # Then look for a Windows zip
    $zip = $Assets | Where-Object { $_.name -match '(?i)^(wadtools).*?(win|windows).*?\.zip$' } | Select-Object -First 1
    if ($zip) { return $zip }
    # Fallback: any .exe
    $anyExe = $Assets | Where-Object { $_.name -match '(?i)\.exe$' } | Select-Object -First 1
    if ($anyExe) { return $anyExe }
    # Fallback: any asset named like wadtools
    $any = $Assets | Where-Object { $_.name -match '(?i)wadtools' } | Select-Object -First 1
    return $any
}

function Download-LatestAsset {
    param([string] $RepoSlug)
    Ensure-Tls12
    $api = "https://api.github.com/repos/$RepoSlug/releases/latest"
    $headers = @{ 'User-Agent' = 'wadtools-installer'; 'Accept' = 'application/vnd.github+json' }
    Write-Info "Fetching latest release metadata from GitHub: $RepoSlug"
    $release = Invoke-RestMethod -Uri $api -Headers $headers -Method Get
    if (-not $release) { throw "Failed to fetch release metadata from $api" }
    $asset = Resolve-Asset -Assets $release.assets
    if (-not $asset) { throw "Could not find a Windows asset in the latest release." }
    return $asset
}

function Install-FromAsset {
    param(
        [object] $Asset,
        [string] $TargetDir
    )
    $tempDir = New-Item -ItemType Directory -Path (Join-Path ([IO.Path]::GetTempPath()) ("wadtools-installer-" + [Guid]::NewGuid().ToString('N'))) -Force
    try {
        $downloadPath = Join-Path $tempDir.FullName $Asset.name
        Write-Info "Downloading asset: $($Asset.name)"
        Invoke-WebRequest -Uri $Asset.browser_download_url -OutFile $downloadPath -UseBasicParsing -Headers @{ 'User-Agent' = 'wadtools-installer' }

        New-Item -ItemType Directory -Path $TargetDir -Force | Out-Null
        $targetExe = Join-Path $TargetDir 'wadtools.exe'

        if ($downloadPath -match '(?i)\.zip$') {
            Write-Info 'Extracting zip contents'
            $extractDir = Join-Path $tempDir.FullName 'extracted'
            Expand-Archive -Path $downloadPath -DestinationPath $extractDir -Force
            $exeCandidate = Get-ChildItem -Path $extractDir -Recurse -File -Include *.exe | Where-Object { $_.Name -match '(?i)^wadtools.*\.exe$' } | Select-Object -First 1
            if (-not $exeCandidate) {
                $exeCandidate = Get-ChildItem -Path $extractDir -Recurse -File -Include *.exe | Select-Object -First 1
            }
            if (-not $exeCandidate) { throw 'No executable found inside the archive.' }
            Copy-Item -Path $exeCandidate.FullName -Destination $targetExe -Force
        } else {
            # Assume it's already an exe
            Copy-Item -Path $downloadPath -Destination $targetExe -Force
        }

        # Ensure executable is unblocked
        try { Unblock-File -Path $targetExe } catch { }

        return $targetExe
    } finally {
        try { Remove-Item -Path $tempDir.FullName -Recurse -Force -ErrorAction SilentlyContinue } catch { }
    }
}

function Main {
    if ($env:OS -ne 'Windows_NT') {
        throw 'This installer only supports Windows.'
    }

    if ([string]::IsNullOrWhiteSpace($InstallDir)) {
        $InstallDir = Get-DefaultInstallDir
    }

    Write-Info "Install directory: $InstallDir"

    $asset = Download-LatestAsset -RepoSlug $Repo
    Write-Info ("Latest release asset: " + $asset.name)

    $exePath = Install-FromAsset -Asset $asset -TargetDir $InstallDir

    Add-PathForCurrentUser -Dir $InstallDir
    Write-Info "Added to user PATH (if not already present): $InstallDir"

    Write-Info "Installed: $exePath"
    try {
        $version = & $exePath --version 2>$null
        if ($LASTEXITCODE -eq 0 -and -not [string]::IsNullOrWhiteSpace($version)) {
            Write-Info ("wadtools version: " + ($version | Select-Object -First 1))
        }
    } catch { }

    Write-Host "\nwadtools installation completed successfully." -ForegroundColor Green
    Write-Host "If your shell does not see 'wadtools' yet, open a new terminal session."
}

Main


