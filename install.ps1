# matchmaker Windows Installation Script
$ErrorActionPreference = "Stop"

# Configuration
$Repo = "Squirreljetpack/matchmaker"
$BinaryName = "mm.exe"
$AssetFileName = "matchmaker-cli-x86_64-pc-windows-msvc.zip"

# 1. Determine Install Directory Priority
$CargoBin = Join-Path $Home ".cargo\bin"
$LocalBin = Join-Path $Home ".local\bin"
$ProgramDir = Join-Path $env:LOCALAPPDATA "Programs\matchmaker"

# Get current PATHs to check existence
$CurrentPath = [Environment]::GetEnvironmentVariable("Path", "User") + ";" + [Environment]::GetEnvironmentVariable("Path", "Machine")
$PathArray = $CurrentPath.Split(';', [System.StringSplitOptions]::RemoveEmptyEntries)

if ($PathArray -contains $CargoBin) {
    $InstallDir = $CargoBin
} elseif ($PathArray -contains $LocalBin -or (Test-Path $LocalBin)) {
    $InstallDir = $LocalBin
} else {
    $InstallDir = $ProgramDir
}

# Ensure the directory exists
if (!(Test-Path $InstallDir)) {
    Write-Host "Creating directory: $InstallDir" -ForegroundColor Gray
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
}

Write-Host "Target installation directory: $InstallDir" -ForegroundColor Cyan

# 2. Fetch Latest Release Version from GitHub
try {
    $ReleaseUrl = "https://api.github.com/repos/$Repo/releases/latest"
    $Release = Invoke-RestMethod -Uri $ReleaseUrl -UseBasicParsing
    $Version = $Release.tag_name
    Write-Host "Found latest version: $Version" -ForegroundColor Green
} catch {
    Write-Host "Error: Failed to fetch latest release from GitHub." -ForegroundColor Red
    exit 1
}

# 3. Download and Extract
$DownloadUrl = "https://github.com/$Repo/releases/download/$Version/$AssetFileName"
$TempDir = Join-Path $env:TEMP "matchmaker-$(New-Guid)"
$ZipFile = Join-Path $TempDir $AssetFileName

New-Item -ItemType Directory -Path $TempDir | Out-Null

Write-Host "Downloading $AssetFileName..." -ForegroundColor Gray
Invoke-WebRequest -Uri $DownloadUrl -OutFile $ZipFile -UseBasicParsing

Write-Host "Extracting files..." -ForegroundColor Gray
Expand-Archive -Path $ZipFile -DestinationPath $TempDir -Force

# 4. Install Binary
$FoundBin = Get-ChildItem -Path $TempDir -Filter $BinaryName -Recurse | Select-Object -First 1

if ($FoundBin) {
    $TargetPath = Join-Path $InstallDir $BinaryName
    if (Test-Path $TargetPath) {
        Write-Host "Replacing existing installation..." -ForegroundColor Yellow
        Remove-Item $TargetPath -Force
    }
    Move-Item -Path $FoundBin.FullName -Destination $InstallDir -Force
} else {
    Write-Host "Error: Could not find $BinaryName in the archive." -ForegroundColor Red
    exit 1
}

# 5. Cleanup
Remove-Item -Path $TempDir -Recurse -Force

# 6. PATH Management Prompt
if ($PathArray -notcontains $InstallDir) {
    Write-Host ""
    Write-Host "ATTENTION: $InstallDir is not in your PATH." -ForegroundColor Yellow
    $Response = Read-Host "Would you like to add this directory to your User PATH automatically? (y/n)"
    
    if ($Response -eq 'y' -or $Response -eq 'Y') {
        try {
            # Get the current User Path safely
            $OldPath = [Environment]::GetEnvironmentVariable("Path", "User")
            
            # If OldPath is null or empty, just set it to the InstallDir
            if ([string]::IsNullOrEmpty($OldPath)) {
                $NewPath = $InstallDir
            } else {
                # Ensure we have a semicolon separator
                $NewPath = $OldPath.TrimEnd(';') + ";" + $InstallDir
            }

            [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
            Write-Host "Success! PATH updated. Please restart your terminal for changes to take effect." -ForegroundColor Green
        } catch {
            Write-Host "Failed to update PATH automatically." -ForegroundColor Red
            Write-Host "Reason: $($_.Exception.Message)" -ForegroundColor Gray
            Write-Host "Manual Step: Add '$InstallDir' to your User Environment Variables."
        }
    }
}

Write-Host "`nSuccessfully installed matchmaker!" -ForegroundColor Green
Write-Host "To get started, run: " -NoNewline
Write-Host "mm --help" -ForegroundColor Cyan