# P06Common.ps1 - shared helpers for the P0-6 WinDivert stability harness.
#
# Windows-native PowerShell 5.1 only. Every helper here is read-only: the
# harness may read hashes, service state, process lists, boot facts and
# lifecycle logs, but must never start/stop the app, the WinDivert service or
# the driver, and must never unload a kernel object. State-changing row
# execution belongs to a later, owner-authorised clean-boot round.
#
# Set-StrictMode is applied here and inherited by every dot-sourced script.

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

$script:P06HarnessVersion = '1.0.0-p0-6a'
$script:P06SchemaVersion = 'p0-6a/v1'
$script:P06ScriptRoot = $PSScriptRoot
$script:P06RepoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path
$script:P06DefaultEvidenceDir = Join-Path $script:P06RepoRoot 'wat\evidence\2026-11-23\p0-6'
$script:P06DefaultInstalledRoot = 'C:\Program Files\lightspeed-gui'
$script:P06TranscriptPath = $null

# -- time / logging -----------------------------------------------------------

function Get-P06UtcNow {
    return (Get-Date).ToUniversalTime().ToString('o')
}

function Get-P06UtcStamp {
    return (Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssZ')
}

function Set-P06Transcript {
    param([string]$Path)
    $script:P06TranscriptPath = $Path
}

function Write-P06Log {
    param(
        [string]$Message,
        [ValidateSet('INFO', 'PASS', 'FAIL', 'BLOCK', 'WARN', 'UNAVAILABLE')]
        [string]$Level = 'INFO'
    )
    $line = '[{0}] {1} {2}' -f (Get-P06UtcNow), $Level, $Message
    Write-Host $line
    if ($null -ne $script:P06TranscriptPath) {
        try {
            [System.IO.File]::AppendAllText(
                $script:P06TranscriptPath,
                $line + [Environment]::NewLine,
                (New-Object System.Text.UTF8Encoding($false))
            )
        } catch {
            # A transcript failure must never change a gate result.
            Write-Host "[WARN] transcript write failed: $($_.Exception.Message)"
        }
    }
}

function Write-P06Section {
    param([string]$Title)
    Write-P06Log ('-' * 4 + ' ' + $Title + ' ' + '-' * 4)
}

# -- safe field access (Set-StrictMode tolerant) ------------------------------

function Get-P06Field {
    param(
        $Object,
        [string]$Name,
        $Default = $null
    )
    if ($null -eq $Object) { return $Default }
    if ($Object -is [System.Collections.IDictionary]) {
        if ($Object.Contains($Name)) { return $Object[$Name] }
        return $Default
    }
    $property = $Object.PSObject.Properties[$Name]
    if ($null -eq $property) { return $Default }
    return $property.Value
}

# -- hashing / files ----------------------------------------------------------

function Get-P06Sha256 {
    param([string]$Path)
    if ([string]::IsNullOrWhiteSpace($Path)) { return $null }
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $null }
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToUpperInvariant()
}

function Get-P06Sha256Text {
    param([string]$Text)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($Text)
        return ([System.BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '')
    } finally {
        $sha.Dispose()
    }
}

function Get-P06FileFacts {
    param([string]$Path)
    $exists = Test-Path -LiteralPath $Path -PathType Leaf
    $record = [ordered]@{
        path = $Path
        exists = [bool]$exists
        size = $null
        sha256 = $null
        file_version = $null
        product_version = $null
        company = $null
        description = $null
    }
    if (-not $exists) { return $record }
    $item = Get-Item -LiteralPath $Path
    $record.size = $item.Length
    $record.sha256 = Get-P06Sha256 -Path $Path
    try {
        $version = $item.VersionInfo
        # A resource-less binary yields empty strings, not $null; keep the fact.
        $record.file_version = $version.FileVersion
        $record.product_version = $version.ProductVersion
        $record.company = $version.CompanyName
        $record.description = $version.FileDescription
    } catch {
        $record.file_version = $null
    }
    return $record
}

function Write-P06JsonFile {
    param(
        [string]$Path,
        $Object
    )
    $json = $Object | ConvertTo-Json -Depth 14
    # PS 5.1's Split-Path has no -LiteralPath in the -Parent parameter set.
    $directory = [System.IO.Path]::GetDirectoryName($Path)
    if (-not [string]::IsNullOrWhiteSpace($directory) -and -not (Test-Path -LiteralPath $directory)) {
        New-Item -ItemType Directory -Path $directory -Force | Out-Null
    }
    [System.IO.File]::WriteAllText($Path, $json + [Environment]::NewLine, (New-Object System.Text.UTF8Encoding($false)))
}

function Write-P06Jsonl {
    param(
        [string]$Path,
        $Record
    )
    $json = $Record | ConvertTo-Json -Compress -Depth 14
    $directory = [System.IO.Path]::GetDirectoryName($Path)
    if (-not [string]::IsNullOrWhiteSpace($directory) -and -not (Test-Path -LiteralPath $directory)) {
        New-Item -ItemType Directory -Path $directory -Force | Out-Null
    }
    [System.IO.File]::AppendAllText(
        $Path,
        $json + [Environment]::NewLine,
        (New-Object System.Text.UTF8Encoding($false))
    )
}

# -- native command capture (read-only) ---------------------------------------

function Invoke-P06Native {
    param(
        [string]$FilePath,
        [string[]]$Arguments = @()
    )
    $record = [ordered]@{
        command = $FilePath
        arguments = @($Arguments)
        exit_code = $null
        output = ''
        error = $null
    }
    try {
        $output = & $FilePath @Arguments 2>&1 | Out-String
        $record.exit_code = $LASTEXITCODE
        $record.output = $output.TrimEnd()
    } catch {
        $record.error = $_.Exception.Message
    }
    return $record
}

# -- boot / host facts --------------------------------------------------------

function Get-P06BootFacts {
    $os = Get-CimInstance -ClassName Win32_OperatingSystem -ErrorAction Stop
    $bootUtc = $os.LastBootUpTime.ToUniversalTime()
    $nowUtc = (Get-Date).ToUniversalTime()
    $uptime = [Math]::Max(0, [int]($nowUtc - $bootUtc).TotalSeconds)
    $bootIdSeed = ('{0}|{1}|{2}' -f $env:COMPUTERNAME, $bootUtc.ToString('o'), $os.SerialNumber)
    return [ordered]@{
        host = $env:COMPUTERNAME
        user = "$env:USERDOMAIN\$env:USERNAME"
        os_caption = $os.Caption
        os_version = $os.Version
        os_build = $os.BuildNumber
        boot_time_utc = $bootUtc.ToString('o')
        boot_id = (Get-P06Sha256Text -Text $bootIdSeed).Substring(0, 16).ToLowerInvariant()
        uptime_seconds = $uptime
        boot_same_day = ($bootUtc.ToLocalTime().Date -eq (Get-Date).Date)
    }
}

function Test-P06Elevated {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return [bool]$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

# -- repository / candidate facts ---------------------------------------------

function Get-P06GitFacts {
    $head = Invoke-P06Native -FilePath 'git' -Arguments @('rev-parse', 'HEAD')
    $branch = Invoke-P06Native -FilePath 'git' -Arguments @('rev-parse', '--abbrev-ref', 'HEAD')
    $status = Invoke-P06Native -FilePath 'git' -Arguments @('status', '--porcelain', '--untracked-files=no')
    $dirty = $false
    if ($null -ne $status.exit_code -and $status.exit_code -eq 0) {
        $dirty = -not [string]::IsNullOrWhiteSpace($status.output)
    }
    return [ordered]@{
        head_commit = $(if ($head.exit_code -eq 0) { $head.output.Trim() } else { $null })
        branch = $(if ($branch.exit_code -eq 0) { $branch.output.Trim() } else { $null })
        worktree_dirty = $dirty
        status_exit = $status.exit_code
    }
}

function Get-P06HarnessFacts {
    $files = @(
        'P06Common.ps1',
        'P06Identity.ps1',
        'P06Preflight.ps1',
        'P06Lifecycle.ps1',
        'P06Rows.ps1',
        'Invoke-P06Gate.ps1',
        'published-baseline.json'
    )
    $hashes = [ordered]@{}
    foreach ($file in $files) {
        $path = Join-Path $script:P06ScriptRoot $file
        $hashes[$file] = Get-P06Sha256 -Path $path
    }
    return [ordered]@{
        harness_version = $script:P06HarnessVersion
        schema_version = $script:P06SchemaVersion
        script_root = $script:P06ScriptRoot
        file_sha256 = $hashes
    }
}

function Get-P06RegistryVersion {
    $roots = @(
        'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
        'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
    )
    $found = @()
    foreach ($root in $roots) {
        if (-not (Test-Path -LiteralPath $root)) { continue }
        foreach ($child in (Get-ChildItem -LiteralPath $root -ErrorAction SilentlyContinue)) {
            $properties = Get-ItemProperty -LiteralPath $child.PSPath -ErrorAction SilentlyContinue
            if ($null -eq $properties) { continue }
            $name = Get-P06Field -Object $properties -Name 'DisplayName' -Default ''
            if ($name -match 'lightspeed') {
                $found += [ordered]@{
                    display_name = $name
                    display_version = (Get-P06Field -Object $properties -Name 'DisplayVersion' -Default $null)
                    install_location = (Get-P06Field -Object $properties -Name 'InstallLocation' -Default $null)
                    registry_key = $child.PSPath
                }
            }
        }
    }
    if ($found.Count -eq 0) { return $null }
    return $found
}
