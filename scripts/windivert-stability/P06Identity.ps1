# P06Identity.ps1 - exact-artifact identity gate for the P0-6 real-driver gate.
#
# Identity first: every row record names the artifact it was run on, and a run
# may not start when the installed (or candidate) GUI / WinDivert.dll /
# WinDivert64.sys does not equal the published release artifact. A mismatch is
# a BLOCK, never a warning (docs/audit/stability-verification-2026-11-23.md
# section 4 rule 1; roadmap P0-6).
#
# The expected hashes come from the published release `sha256.sum` when the
# network is available, cross-checked against scripts/windivert-stability/
# published-baseline.json. When the network is unavailable the pinned baseline
# is the source (the audit allows "the local verified hashes"); it is recorded
# as such, never silently upgraded to a live check.
#
# Read-only: hashes, file versions, registry metadata, one optional release-zip
# download into a temp directory. Never touches the driver or the app.

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

function Get-P06PublishedBaseline {
    param([string]$Path)
    if ([string]::IsNullOrWhiteSpace($Path)) {
        $Path = Join-Path $script:P06ScriptRoot 'published-baseline.json'
    }
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return [ordered]@{
            available = $false
            path = $Path
            error = 'published baseline file not found'
            data = $null
        }
    }
    try {
        $data = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
        return [ordered]@{
            available = $true
            path = $Path
            sha256 = (Get-P06Sha256 -Path $Path)
            error = $null
            data = $data
        }
    } catch {
        return [ordered]@{
            available = $false
            path = $Path
            error = $_.Exception.Message
            data = $null
        }
    }
}

function Invoke-P06FetchText {
    param(
        [string]$Uri,
        [int]$TimeoutSec = 25
    )
    $record = [ordered]@{
        uri = $Uri
        ok = $false
        status_code = $null
        text = $null
        error = $null
    }
    try {
        # GitHub requires TLS 1.2; some 5.1 hosts still default to older TLS.
        try {
            [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        } catch {
            # Already-configured protocol is not an error.
        }
        $response = Invoke-WebRequest -Uri $Uri -UseBasicParsing -TimeoutSec $TimeoutSec `
            -Headers @{ 'User-Agent' = 'lightspeed-p0-6-harness/1.0' }
        $record.status_code = [int]$response.StatusCode
        if ($response.Content -is [byte[]]) {
            $record.text = [Text.Encoding]::UTF8.GetString($response.Content)
        } else {
            $record.text = [string]$response.Content
        }
        $record.ok = $true
    } catch {
        $record.error = $_.Exception.Message
    }
    return $record
}

function ConvertFrom-P06Sha256Sum {
    param([string]$Text)
    $entries = [ordered]@{}
    if ([string]::IsNullOrWhiteSpace($Text)) { return $entries }
    foreach ($line in ($Text -split "`r?`n")) {
        $trimmed = $line.Trim()
        if ($trimmed.Length -eq 0) { continue }
        $match = [regex]::Match($trimmed, '^([0-9a-fA-F]{64})\s+\*?(.+)$')
        if ($match.Success) {
            $entries[$match.Groups[2].Value.Trim()] = $match.Groups[1].Value.ToUpperInvariant()
        }
    }
    return $entries
}

function Get-P06ReleaseFacts {
    param(
        [string]$Tag = 'v1.12.5',
        [int]$TimeoutSec = 25
    )
    $apiUri = 'https://api.github.com/repos/ShibbityShwab/lightspeed/releases/tags/' + $Tag
    $record = [ordered]@{
        tag = $Tag
        source = 'none'
        online = $false
        api_uri = $apiUri
        api_ok = $false
        api_error = $null
        tag_name = $null
        target_commitish = $null
        draft = $null
        prerelease = $null
        assets = @()
        sha256_sum = [ordered]@{}
        sha256_sum_ok = $false
        sha256_sum_error = $null
        zip_asset = $null
        zip_sha256 = $null
        msi_asset = $null
        msi_sha256 = $null
        proxy_windows_asset = $null
        proxy_linux_asset = $null
        proxy_linux_sha256 = $null
    }
    $api = Invoke-P06FetchText -Uri $apiUri -TimeoutSec $TimeoutSec
    if (-not $api.ok) {
        $record.api_error = $api.error
        return $record
    }
    try {
        $parsed = $api.text | ConvertFrom-Json
    } catch {
        $record.api_error = 'release API JSON could not be parsed: ' + $_.Exception.Message
        return $record
    }
    $record.api_ok = $true
    $record.online = $true
    $record.source = 'live'
    $record.tag_name = Get-P06Field -Object $parsed -Name 'tag_name' -Default $Tag
    $record.target_commitish = Get-P06Field -Object $parsed -Name 'target_commitish' -Default $null
    $record.draft = Get-P06Field -Object $parsed -Name 'draft' -Default $null
    $record.prerelease = Get-P06Field -Object $parsed -Name 'prerelease' -Default $null
    $assetList = Get-P06Field -Object $parsed -Name 'assets' -Default @()
    foreach ($asset in @($assetList)) {
        $record.assets += [ordered]@{
            name = (Get-P06Field -Object $asset -Name 'name' -Default $null)
            size = (Get-P06Field -Object $asset -Name 'size' -Default $null)
            digest = (Get-P06Field -Object $asset -Name 'digest' -Default $null)
            url = (Get-P06Field -Object $asset -Name 'browser_download_url' -Default $null)
        }
    }

    $sumUri = 'https://github.com/ShibbityShwab/lightspeed/releases/download/' + $Tag + '/sha256.sum'
    $sum = Invoke-P06FetchText -Uri $sumUri -TimeoutSec $TimeoutSec
    if ($sum.ok) {
        $record.sha256_sum = ConvertFrom-P06Sha256Sum -Text $sum.text
        $record.sha256_sum_ok = $true
        # Get-P06Field on a dictionary returns the VALUE, so key lookup uses the
        # dictionary itself: assign the key first, then index it for the hash.
        $zipKey = 'lightspeed-gui-x86_64-pc-windows-msvc.zip'
        if ($record.sha256_sum.Contains($zipKey)) {
            $record.zip_asset = $zipKey
            $record.zip_sha256 = $record.sha256_sum[$zipKey]
        } else {
            foreach ($key in $record.sha256_sum.Keys) {
                if ($key -like '*gui*x86_64-pc-windows-msvc.zip') {
                    $record.zip_asset = $key
                    $record.zip_sha256 = $record.sha256_sum[$key]
                }
            }
        }
        $msiKey = 'lightspeed-gui-x86_64-pc-windows-msvc.msi'
        if ($record.sha256_sum.Contains($msiKey)) {
            $record.msi_asset = $msiKey
            $record.msi_sha256 = $record.sha256_sum[$msiKey]
        }
        $proxyKey = 'lightspeed-proxy-x86_64-unknown-linux-gnu.tar.xz'
        if ($record.sha256_sum.Contains($proxyKey)) {
            $record.proxy_linux_asset = $proxyKey
            $record.proxy_linux_sha256 = $record.sha256_sum[$proxyKey]
        }
        foreach ($key in $record.sha256_sum.Keys) {
            if ($key -like '*proxy*windows*') { $record.proxy_windows_asset = $key }
        }
    } else {
        $record.sha256_sum_error = $sum.error
    }
    return $record
}

function Get-P06InstalledProxy {
    param([string]$InstalledRoot)
    $candidates = @(
        (Join-Path $InstalledRoot 'bin\lightspeed-proxy.exe'),
        (Join-Path $InstalledRoot 'lightspeed-proxy.exe')
    )
    $existing = $null
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            $existing = $candidate
            break
        }
    }
    if ($null -eq $existing) {
        return [ordered]@{
            installed = $false
            path = $null
            sha256 = $null
            note = 'no Windows proxy binary is part of the v1.12.5 desktop artifact'
        }
    }
    return [ordered]@{
        installed = $true
        path = $existing
        sha256 = (Get-P06Sha256 -Path $existing)
        note = 'unexpected Windows proxy binary found in the install root'
    }
}

function Get-P06ArtifactSet {
    param(
        [string]$Root,
        [string]$Label = 'installed'
    )
    $binDir = Join-Path $Root 'bin'
    if (-not (Test-Path -LiteralPath $binDir -PathType Container)) { $binDir = $Root }
    $guiExe = Join-Path $binDir 'lightspeed-gui.exe'
    $dll = Join-Path $binDir 'WinDivert.dll'
    $sys = Join-Path $binDir 'WinDivert64.sys'
    return [ordered]@{
        label = $Label
        root = $Root
        bin_dir = $binDir
        gui_exe = (Get-P06FileFacts -Path $guiExe)
        windivert_dll = (Get-P06FileFacts -Path $dll)
        windivert_sys = (Get-P06FileFacts -Path $sys)
        proxy = (Get-P06InstalledProxy -InstalledRoot $Root)
        registry_version = (Get-P06RegistryVersion)
    }
}

function Invoke-P06ReleaseZipVerification {
    param(
        [string]$Tag,
        [string]$ExpectedZipSha256,
        [string]$ExpectedGuiSha256,
        [string]$ExpectedDllSha256,
        [string]$ExpectedSysSha256,
        [string]$WorkDir,
        [int]$TimeoutSec = 180
    )
    $result = [ordered]@{
        attempted = $true
        ok = $false
        zip_name = 'lightspeed-gui-x86_64-pc-windows-msvc.zip'
        zip_path = $null
        zip_sha256 = $null
        zip_sha256_matches = $false
        members = [ordered]@{}
        member_mismatches = @()
        error = $null
        downloaded_removed = $false
    }
    try {
        if (-not (Test-Path -LiteralPath $WorkDir)) {
            New-Item -ItemType Directory -Path $WorkDir -Force | Out-Null
        }
        $zipPath = Join-Path $WorkDir $result.zip_name
        $url = 'https://github.com/ShibbityShwab/lightspeed/releases/download/' + $Tag + '/' + $result.zip_name
        try {
            [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        } catch {
        }
        Invoke-WebRequest -Uri $url -UseBasicParsing -TimeoutSec $TimeoutSec -OutFile $zipPath
        $result.zip_path = $zipPath
        $result.zip_sha256 = (Get-P06Sha256 -Path $zipPath)
        $expectedZip = ([string]$ExpectedZipSha256).Trim().ToUpperInvariant()
        $result.zip_sha256_matches = ($expectedZip.Length -gt 0 -and $result.zip_sha256 -eq $expectedZip)
        $extractDir = Join-Path $WorkDir 'extracted'
        if (Test-Path -LiteralPath $extractDir) {
            Remove-Item -LiteralPath $extractDir -Recurse -Force
        }
        Expand-Archive -LiteralPath $zipPath -DestinationPath $extractDir -Force
        $expected = [ordered]@{
            'lightspeed-gui.exe' = $ExpectedGuiSha256
            'WinDivert.dll' = $ExpectedDllSha256
            'WinDivert64.sys' = $ExpectedSysSha256
        }
        foreach ($name in $expected.Keys) {
            $found = Get-ChildItem -LiteralPath $extractDir -Recurse -File -Filter $name -ErrorAction SilentlyContinue | Select-Object -First 1
            if ($null -eq $found) {
                $result.members[$name] = [ordered]@{ present = $false; sha256 = $null; matches = $false }
                $result.member_mismatches += $name
                continue
            }
            $hash = Get-P06Sha256 -Path $found.FullName
            $expectedHash = ([string]$expected[$name]).Trim().ToUpperInvariant()
            $matches = ($expectedHash.Length -gt 0 -and $hash -eq $expectedHash)
            $result.members[$name] = [ordered]@{
                present = $true
                relative_path = $found.FullName.Replace(($extractDir.TrimEnd('\') + '\'), '')
                size = $found.Length
                sha256 = $hash
                expected_sha256 = $expected[$name]
                matches = $matches
            }
            if (-not $matches) { $result.member_mismatches += $name }
        }
        $result.ok = ($result.zip_sha256_matches -and @($result.member_mismatches).Count -eq 0)
        Remove-Item -LiteralPath $extractDir -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $zipPath -Force -ErrorAction SilentlyContinue
        $result.downloaded_removed = (-not (Test-Path -LiteralPath $zipPath))
    } catch {
        $result.error = $_.Exception.Message
        $result.ok = $false
    }
    return $result
}

function Invoke-P06IdentityGate {
    param(
        [string]$InstalledRoot = $script:P06DefaultInstalledRoot,
        [string]$CandidateRoot = $null,
        [string]$BaselinePath = $null,
        [string]$Tag = 'v1.12.5',
        [switch]$NoNetwork,
        [switch]$VerifyReleaseZip,
        [string]$WorkDir = $null
    )
    $checkedAt = Get-P06UtcNow
    $baseline = Get-P06PublishedBaseline -Path $BaselinePath
    $baselineData = Get-P06Field -Object $baseline -Name 'data' -Default $null

    $expectedTag = Get-P06Field -Object $baselineData -Name 'tag' -Default 'v1.12.5'
    $expectedCommit = Get-P06Field -Object $baselineData -Name 'release_target_commit' -Default $null
    $expectedZipSha = Get-P06Field -Object $baselineData -Name 'release_zip_sha256' -Default $null
    $expectedMsiSha = Get-P06Field -Object $baselineData -Name 'release_msi_sha256' -Default $null
    $members = Get-P06Field -Object $baselineData -Name 'members' -Default $null
    $expectedGuiSha = $null
    $expectedDllSha = $null
    $expectedSysSha = $null
    if ($null -ne $members) {
        $expectedGuiSha = Get-P06Field -Object (Get-P06Field -Object $members -Name 'lightspeed-gui.exe' -Default $null) -Name 'sha256' -Default $null
        $expectedDllSha = Get-P06Field -Object (Get-P06Field -Object $members -Name 'WinDivert.dll' -Default $null) -Name 'sha256' -Default $null
        $expectedSysSha = Get-P06Field -Object (Get-P06Field -Object $members -Name 'WinDivert64.sys' -Default $null) -Name 'sha256' -Default $null
    }
    $proxyBaseline = Get-P06Field -Object $baselineData -Name 'proxy' -Default $null
    $expectedProxyLinuxSha = Get-P06Field -Object $proxyBaseline -Name 'linux_sha256' -Default $null

    $release = [ordered]@{
        source = 'pinned'
        online = $false
        tag = $expectedTag
        target_commitish = $expectedCommit
        zip_asset = (Get-P06Field -Object $baselineData -Name 'release_zip_asset' -Default $null)
        zip_sha256 = $expectedZipSha
        msi_asset = (Get-P06Field -Object $baselineData -Name 'release_msi_asset' -Default $null)
        msi_sha256 = $expectedMsiSha
        proxy_linux_asset = (Get-P06Field -Object $proxyBaseline -Name 'linux_asset' -Default $null)
        proxy_linux_sha256 = $expectedProxyLinuxSha
        proxy_windows_asset = (Get-P06Field -Object $proxyBaseline -Name 'windows_asset' -Default $null)
        api_ok = $false
        api_error = $null
        sha256_sum_ok = $false
        sha256_sum_error = $null
        asset_count = 0
        baseline_drift = @()
    }
    if (-not $NoNetwork) {
        $live = Get-P06ReleaseFacts -Tag $expectedTag
        $release.api_ok = $live.api_ok
        $release.api_error = $live.api_error
        $release.sha256_sum_ok = $live.sha256_sum_ok
        $release.sha256_sum_error = $live.sha256_sum_error
        $release.asset_count = @($live.assets).Count
        if ($live.online) {
            $release.source = 'live'
            $release.online = $true
            if ($null -ne $live.target_commitish -and $live.target_commitish -ne $expectedCommit) {
                $release.baseline_drift += ('release target commit {0} != baseline {1}' -f $live.target_commitish, $expectedCommit)
            }
            if ($null -ne $live.zip_sha256 -and $null -ne $expectedZipSha -and $live.zip_sha256 -ne $expectedZipSha) {
                $release.baseline_drift += ('release zip sha256 {0} != baseline {1}' -f $live.zip_sha256, $expectedZipSha)
            }
            if ($null -ne $live.msi_sha256 -and $null -ne $expectedMsiSha -and $live.msi_sha256 -ne $expectedMsiSha) {
                $release.baseline_drift += 'release msi sha256 differs from baseline'
            }
            if ($null -ne $live.proxy_linux_sha256 -and $null -ne $expectedProxyLinuxSha -and $live.proxy_linux_sha256 -ne $expectedProxyLinuxSha) {
                $release.baseline_drift += 'release proxy sha256 differs from baseline'
            }
            $release.target_commitish = $live.target_commitish
            $release.zip_sha256 = $live.zip_sha256
            $release.msi_sha256 = $live.msi_sha256
            $release.proxy_linux_sha256 = $live.proxy_linux_sha256
            $release.proxy_windows_asset = $live.proxy_windows_asset
        }
    }

    $installed = Get-P06ArtifactSet -Root $InstalledRoot -Label 'installed'
    $version = $null
    $registryVersion = Get-P06Field -Object $installed -Name 'registry_version' -Default $null
    if ($null -ne $registryVersion -and @($registryVersion).Count -gt 0) {
        $first = @($registryVersion)[0]
        $version = Get-P06Field -Object $first -Name 'display_version' -Default $null
    }
    if ([string]::IsNullOrWhiteSpace($version)) {
        $version = $expectedTag.TrimStart('v')
    }
    $shortCommit = $null
    if (-not [string]::IsNullOrWhiteSpace($expectedCommit) -and $expectedCommit.Length -ge 12) {
        $shortCommit = $expectedCommit.Substring(0, 12)
    } else {
        $shortCommit = $expectedCommit
    }
    $buildString = ('{0}@{1}' -f $version, $shortCommit)

    $comparisons = @()
    $pairs = @(
        @{ name = 'lightspeed-gui.exe'; installed = (Get-P06Field -Object $installed.gui_exe -Name 'sha256' -Default $null); expected = $expectedGuiSha },
        @{ name = 'WinDivert.dll'; installed = (Get-P06Field -Object $installed.windivert_dll -Name 'sha256' -Default $null); expected = $expectedDllSha },
        @{ name = 'WinDivert64.sys'; installed = (Get-P06Field -Object $installed.windivert_sys -Name 'sha256' -Default $null); expected = $expectedSysSha }
    )
    foreach ($pair in $pairs) {
        $result = 'missing'
        if ($null -ne $pair.installed -and $null -ne $pair.expected -and $pair.installed -eq $pair.expected) {
            $result = 'match'
        } elseif ($null -ne $pair.installed -and $null -ne $pair.expected) {
            $result = 'mismatch'
        } elseif ($null -eq $pair.installed) {
            $result = 'missing'
        }
        $comparisons += [ordered]@{
            name = $pair.name
            installed_sha256 = $pair.installed
            expected_sha256 = $pair.expected
            result = $result
        }
    }

    $candidate = $null
    if (-not [string]::IsNullOrWhiteSpace($CandidateRoot)) {
        $candidateSet = Get-P06ArtifactSet -Root $CandidateRoot -Label 'candidate'
        $candidateComparisons = @()
        $candidatePairs = @(
            @{ name = 'lightspeed-gui.exe'; installed = (Get-P06Field -Object $installed.gui_exe -Name 'sha256' -Default $null); candidate = (Get-P06Field -Object $candidateSet.gui_exe -Name 'sha256' -Default $null) },
            @{ name = 'WinDivert.dll'; installed = (Get-P06Field -Object $installed.windivert_dll -Name 'sha256' -Default $null); candidate = (Get-P06Field -Object $candidateSet.windivert_dll -Name 'sha256' -Default $null) },
            @{ name = 'WinDivert64.sys'; installed = (Get-P06Field -Object $installed.windivert_sys -Name 'sha256' -Default $null); candidate = (Get-P06Field -Object $candidateSet.windivert_sys -Name 'sha256' -Default $null) }
        )
        foreach ($pair in $candidatePairs) {
            $result = 'missing'
            if ($null -ne $pair.candidate -and $pair.candidate -eq $pair.installed) {
                $result = 'match'
            } elseif ($null -ne $pair.candidate) {
                $result = 'mismatch'
            }
            $candidateComparisons += [ordered]@{
                name = $pair.name
                candidate_sha256 = $pair.candidate
                installed_sha256 = $pair.installed
                result = $result
            }
        }
        $candidate = [ordered]@{
            root = $CandidateRoot
            artifacts = $candidateSet
            comparisons = $candidateComparisons
        }
    }

    $releaseZipVerification = $null
    if ($VerifyReleaseZip) {
        if ([string]::IsNullOrWhiteSpace($WorkDir)) {
            $WorkDir = Join-Path $env:TEMP ('lightspeed-p06-release-' + (Get-P06UtcStamp))
        }
        $releaseZipVerification = Invoke-P06ReleaseZipVerification `
            -Tag $expectedTag `
            -ExpectedZipSha256 $release.zip_sha256 `
            -ExpectedGuiSha256 $expectedGuiSha `
            -ExpectedDllSha256 $expectedDllSha `
            -ExpectedSysSha256 $expectedSysSha `
            -WorkDir $WorkDir
    }

    $reasons = @()
    foreach ($comparison in $comparisons) {
        if ($comparison.result -ne 'match') {
            $reasons += ('artifact {0} {1} vs published {2}' -f $comparison.name, $comparison.result, $expectedTag)
        }
    }
    foreach ($drift in @($release.baseline_drift)) {
        $reasons += 'baseline drift: ' + $drift
    }
    if ($null -ne $candidate) {
        foreach ($comparison in @($candidate.comparisons)) {
            if ($comparison.result -ne 'match') {
                $reasons += ('candidate {0} does not equal the installed artifact ({1})' -f $comparison.name, $comparison.result)
            }
        }
    }
    if ($null -ne $releaseZipVerification) {
        if (-not $releaseZipVerification.ok) {
            $reasons += 'release zip member verification failed'
        }
    }
    $baselineOk = (Get-P06Field -Object $baseline -Name 'available' -Default $false)
    if (-not $baselineOk -and (-not $release.online)) {
        $reasons += 'no published baseline and no live release facts; identity cannot be established'
    }

    $status = 'match'
    if (@($reasons).Count -gt 0) {
        $status = 'mismatch'
    } elseif (-not $release.online -and -not $baselineOk) {
        $status = 'unavailable'
    }

    $result = [ordered]@{
        schema = 'p0-6-identity/v1'
        checked_at_utc = $checkedAt
        installed_root = $InstalledRoot
        installed = $installed
        candidate = $candidate
        expected = [ordered]@{
            tag = $expectedTag
            release_target_commit = $expectedCommit
            build_string = $buildString
            version = $version
            short_commit = $shortCommit
            gui_exe_sha256 = $expectedGuiSha
            windivert_dll_sha256 = $expectedDllSha
            windivert_sys_sha256 = $expectedSysSha
            release_zip_sha256 = $expectedZipSha
            release_msi_sha256 = $expectedMsiSha
            proxy_linux_sha256 = $expectedProxyLinuxSha
        }
        release = $release
        comparisons = $comparisons
        release_zip_verification = $releaseZipVerification
        baseline = [ordered]@{
            available = $baselineOk
            path = (Get-P06Field -Object $baseline -Name 'path' -Default $null)
            sha256 = (Get-P06Field -Object $baseline -Name 'sha256' -Default $null)
            error = (Get-P06Field -Object $baseline -Name 'error' -Default $null)
        }
        status = $status
        gate = $(if ($status -eq 'match') { 'PASS' } else { 'BLOCK' })
        block_reason = $(if (@($reasons).Count -gt 0) { @($reasons) -join '; ' } else { $null })
        identity_source = $(if ($release.online) { 'live-release' } else { 'pinned-baseline' })
        agent_harness = (Get-P06HarnessFacts)
        boot = (Get-P06BootFacts)
        git = (Get-P06GitFacts)
        elevated = (Test-P06Elevated)
    }
    return $result
}
