# P06Preflight.ps1 - read-only clean-boot preflight for the P0-6 gate.
#
# The point of this file is refusal. A real-driver run on a kernel that still has
# the WinDivert driver loaded, or with a LightSpeed process alive, measures a
# different system from the field failure (docs/audit/stability-verification-
# 2026-11-23.md section 3.2 F2/F3). So the harness REFUSES: it never stops/starts
# the WinDivert service, never unloads the driver, never kills a process, and
# never launches the app.
#
# Every operation here is read-only:
#   sc.exe query WinDivert / sc.exe qc WinDivert   (query, not control)
#   Get-Service / Get-CimInstance Win32_SystemDriver
#   Get-Process / Get-CimInstance Win32_Process
#   netsh.exe wfp show state file=<evidence>       (read + write our own file)
#   boot facts from Win32_OperatingSystem
# The pre-state and post-state are compared so a run can prove the harness
# itself changed nothing.

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

function Get-P06LightSpeedProcesses {
    param([string]$InstallRoot = $script:P06DefaultInstalledRoot)
    $found = @()
    $knownNames = @('lightspeed', 'lightspeed-gui', 'lightspeed-client', 'lightspeed-proxy', 'lightspeed-gui-updater')
    try {
        $processes = @(Get-Process -ErrorAction SilentlyContinue)
    } catch {
        $processes = @()
    }
    foreach ($process in $processes) {
        $name = $null
        try { $name = $process.ProcessName } catch { $name = $null }
        if ([string]::IsNullOrWhiteSpace($name)) { continue }
        if ($knownNames -notcontains $name.ToLowerInvariant()) { continue }
        $path = $null
        try { $path = $process.Path } catch { $path = $null }
        $found += [ordered]@{
            pid = $process.Id
            name = $name
            path = $path
            source = 'Get-Process'
        }
    }
    try {
        $cim = @(Get-CimInstance -ClassName Win32_Process -ErrorAction SilentlyContinue | Where-Object {
            $_.Name -match '^lightspeed' -or ($null -ne $_.ExecutablePath -and $_.ExecutablePath -like ($InstallRoot + '*'))
        })
    } catch {
        $cim = @()
    }
    foreach ($entry in $cim) {
        $already = $false
        foreach ($existing in $found) {
            if ($existing.pid -eq $entry.ProcessId) { $already = $true }
        }
        if ($already) { continue }
        $found += [ordered]@{
            pid = $entry.ProcessId
            name = $entry.Name
            path = $entry.ExecutablePath
            source = 'Win32_Process'
        }
    }
    return @($found)
}

function Get-P06ServiceFacts {
    $scQuery = Invoke-P06Native -FilePath 'sc.exe' -Arguments @('query', 'WinDivert')
    $scQc = Invoke-P06Native -FilePath 'sc.exe' -Arguments @('qc', 'WinDivert')
    $state = 'unknown'
    $win32Exit = $null
    if ($null -ne $scQuery.exit_code -and $scQuery.exit_code -eq 0) {
        $match = [regex]::Match($scQuery.output, 'STATE\s*:\s*(\d+)\s+(\w+)')
        if ($match.Success) {
            $win32Exit = [int]$match.Groups[1].Value
            $state = $match.Groups[2].Value.ToUpperInvariant()
        }
    } elseif ($null -ne $scQuery.exit_code -and $scQuery.exit_code -eq 1060) {
        $state = 'NOT_INSTALLED'
    } elseif ($null -ne $scQuery.error) {
        $state = 'QUERY_ERROR'
    }
    $driverPath = $null
    $match = [regex]::Match($scQc.output, 'BINARY_PATH_NAME\s*:\s*(.+)')
    if ($match.Success) {
        $driverPath = $match.Groups[1].Value.Trim()
    }
    $startType = $null
    $match = [regex]::Match($scQc.output, 'START_TYPE\s*:\s*\d+\s+(\w+)')
    if ($match.Success) {
        $startType = $match.Groups[1].Value.ToUpperInvariant()
    }
    $cimDriver = $null
    try {
        $candidate = Get-CimInstance -ClassName Win32_SystemDriver -ErrorAction Stop |
            Where-Object { $_.Name -eq 'WinDivert' } |
            Select-Object -First 1
        if ($null -ne $candidate) {
            $cimDriver = [ordered]@{
                name = $candidate.Name
                state = $candidate.State
                status = $candidate.Status
                started = $candidate.Started
                path_name = $candidate.PathName
                start_mode = $candidate.StartMode
            }
        }
    } catch {
        $cimDriver = [ordered]@{ error = $_.Exception.Message }
    }
    $service = $null
    try {
        $candidate = Get-Service -Name 'WinDivert' -ErrorAction SilentlyContinue
        if ($null -ne $candidate) {
            $service = [ordered]@{
                name = $candidate.Name
                status = $candidate.Status.ToString()
                start_type = $candidate.StartType.ToString()
            }
        }
    } catch {
        $service = $null
    }
    $driverLoaded = ($state -eq 'RUNNING')
    return [ordered]@{
        sc_query = $scQuery
        sc_qc = $scQc
        state = $state
        win32_exit_code = $win32Exit
        driver_path = $driverPath
        start_type = $startType
        driver_loaded = $driverLoaded
        cim_driver = $cimDriver
        service = $service
    }
}

function Get-P06WfpSnapshotOnce {
    param(
        [string]$WorkDir,
        [switch]$SkipEnumeration
    )
    $snapshot = [ordered]@{
        attempted = (-not $SkipEnumeration)
        available = $false
        elevated = (Test-P06Elevated)
        state_file = $null
        state_file_size = $null
        error = $null
        win32_exit_code = $null
        totals = [ordered]@{
            providers = $null
            sublayers = $null
            filters = $null
            callouts = $null
            layers = $null
        }
        windivert = [ordered]@{
            providers = $null
            subLayers = $null
            filters = $null
            callouts = $null
        }
    }
    if ($SkipEnumeration) {
        $snapshot.error = 'WFP enumeration skipped by request'
        return $snapshot
    }
    try {
        if (-not (Test-Path -LiteralPath $WorkDir)) {
            New-Item -ItemType Directory -Path $WorkDir -Force | Out-Null
        }
        # Keep the state file path short: netsh writes XML next to the caller and
        # long paths are fragile on 5.1. The GUID suffix keeps two back-to-back
        # read-only enumerations from colliding on the same second.
        $stateFile = Join-Path $WorkDir ('wfp-state-' + (Get-Date).ToUniversalTime().ToString('HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8) + '.xml')
        $capture = Invoke-P06Native -FilePath 'netsh.exe' -Arguments @('wfp', 'show', 'state', ('file=' + $stateFile))
        $snapshot.win32_exit_code = $capture.exit_code
        $snapshot.error = $capture.error
        if (-not (Test-Path -LiteralPath $stateFile -PathType Leaf)) {
            if ([string]::IsNullOrWhiteSpace($snapshot.error)) {
                $snapshot.error = 'netsh wfp show state produced no file'
            }
            return $snapshot
        }
        $snapshot.state_file = $stateFile
        $raw = [System.IO.File]::ReadAllText($stateFile)
        $snapshot.state_file_size = $raw.Length
        $sections = @('providers', 'subLayers', 'filters', 'callouts', 'layers')
        foreach ($section in $sections) {
            $totals = $null
            $top = [regex]::Match($raw, ('<' + $section + '\s+numItems="(\d+)"'))
            if ($top.Success) {
                $totals = [int]$top.Groups[1].Value
            }
            $key = $section
            if ($section -eq 'subLayers') { $key = 'sublayers' }
            $snapshot.totals[$key] = $totals
            if ($section -eq 'layers') { continue }
            $openTag = '<' + $section + ' '
            $closeTag = '</' + $section + '>'
            $start = $raw.IndexOf($openTag, [System.StringComparison]::Ordinal)
            $end = $raw.IndexOf($closeTag, [System.StringComparison]::Ordinal)
            if ($start -ge 0 -and $end -gt $start) {
                $body = $raw.Substring($start, $end - $start)
                $names = [regex]::Matches($body, '<name>([^<]*)</name>')
                $winDivert = 0
                foreach ($name in $names) {
                    if ($name.Groups[1].Value -match 'WinDivert') { $winDivert++ }
                }
                $snapshot.windivert[$section] = $winDivert
            }
        }
        $snapshot.available = $true
    } catch {
        $snapshot.error = $_.Exception.Message
    }
    return $snapshot
}

# `netsh wfp show state` occasionally fails transiently (BFE busy); retry once
# so a read-only enumeration flake is not reported as unavailable evidence.
function Get-P06WfpSnapshot {
    param(
        [string]$WorkDir,
        [switch]$SkipEnumeration,
        [int]$Attempts = 2
    )
    $last = $null
    for ($attempt = 1; $attempt -le [Math]::Max(1, $Attempts); $attempt++) {
        $last = Get-P06WfpSnapshotOnce -WorkDir $WorkDir -SkipEnumeration:$SkipEnumeration
        if ([bool](Get-P06Field -Object $last -Name 'available' -Default $false)) { return $last }
        if ($SkipEnumeration) { break }
    }
    return $last
}

function Get-P06KernelState {
    param(
        [string]$InstallRoot = $script:P06DefaultInstalledRoot,
        [string]$WorkDir,
        [switch]$SkipWfp
    )
    $boot = Get-P06BootFacts
    $service = Get-P06ServiceFacts
    $processes = Get-P06LightSpeedProcesses -InstallRoot $InstallRoot
    $wfp = Get-P06WfpSnapshot -WorkDir $WorkDir -SkipEnumeration:$SkipWfp
    $dataDir = Join-Path $env:LOCALAPPDATA 'Lightspeed'
    $lifecycleFile = Join-Path $dataDir 'lifecycle.jsonl'
    $uncleanMarker = Join-Path $dataDir 'lifecycle.unclean'
    $lifecycle = [ordered]@{
        data_dir = $dataDir
        lifecycle_jsonl_present = (Test-Path -LiteralPath $lifecycleFile -PathType Leaf)
        lifecycle_jsonl_size = $(if (Test-Path -LiteralPath $lifecycleFile -PathType Leaf) { (Get-Item -LiteralPath $lifecycleFile).Length } else { $null })
        lifecycle_jsonl_mtime_utc = $(if (Test-Path -LiteralPath $lifecycleFile -PathType Leaf) { (Get-Item -LiteralPath $lifecycleFile).LastWriteTimeUtc.ToString('o') } else { $null })
        unclean_marker_present = (Test-Path -LiteralPath $uncleanMarker -PathType Leaf)
    }
    $reasons = @()
    if ($service.state -eq 'RUNNING') {
        $reasons += 'WinDivert service RUNNING'
    }
    if (@($processes).Count -gt 0) {
        $pids = @($processes | ForEach-Object { $_.pid }) -join ','
        $reasons += ('LightSpeed process present (pid ' + $pids + ')')
    }
    $dirty = (@($reasons).Count -gt 0)
    return [ordered]@{
        checked_at_utc = (Get-P06UtcNow)
        boot = $boot
        service = $service
        driver_loaded = $service.driver_loaded
        process_count = @($processes).Count
        processes = @($processes)
        wfp = $wfp
        lifecycle = $lifecycle
        dirty = $dirty
        clean = (-not $dirty)
        dirty_reasons = @($reasons)
        kernel_state = $(if ($dirty) { 'dirty' } else { 'clean' })
    }
}

function Get-P06KernelFingerprint {
    param($KernelState)
    # The no-state-change proof is about the things this harness could wrongly
    # mutate or that would invalidate a run: the service state, whether the
    # driver is loaded, and the LightSpeed pids. WFP object counts are recorded
    # separately because unrelated system activity can legitimately add or
    # remove filters; they must not masquerade as the harness changing state.
    $service = Get-P06Field -Object $KernelState -Name 'service' -Default $null
    $processes = @(Get-P06Field -Object $KernelState -Name 'processes' -Default @())
    $pids = @($processes | ForEach-Object { [string]$_.pid }) -join ','
    return ('state={0};driver={1};pids={2}' -f `
        (Get-P06Field -Object $service -Name 'state' -Default 'unknown'),
        (Get-P06Field -Object $KernelState -Name 'driver_loaded' -Default $null),
        $pids)
}

function Get-P06WfpFingerprint {
    param($KernelState)
    $wfp = Get-P06Field -Object $KernelState -Name 'wfp' -Default $null
    $totals = Get-P06Field -Object $wfp -Name 'totals' -Default $null
    $windivert = Get-P06Field -Object $wfp -Name 'windivert' -Default $null
    return ('totals={0}|{1}|{2}|{3}|{4};win={5}|{6}|{7}|{8}' -f `
        (Get-P06Field -Object $totals -Name 'providers' -Default 'na'),
        (Get-P06Field -Object $totals -Name 'sublayers' -Default 'na'),
        (Get-P06Field -Object $totals -Name 'filters' -Default 'na'),
        (Get-P06Field -Object $totals -Name 'callouts' -Default 'na'),
        (Get-P06Field -Object $totals -Name 'layers' -Default 'na'),
        (Get-P06Field -Object $windivert -Name 'providers' -Default 'na'),
        (Get-P06Field -Object $windivert -Name 'subLayers' -Default 'na'),
        (Get-P06Field -Object $windivert -Name 'filters' -Default 'na'),
        (Get-P06Field -Object $windivert -Name 'callouts' -Default 'na'))
}

function Invoke-P06Preflight {
    param(
        [string]$InstallRoot = $script:P06DefaultInstalledRoot,
        [string]$WorkDir,
        [switch]$SkipWfp
    )
    Write-P06Section 'P0-6 clean-boot preflight (read-only)'
    Write-P06Log 'Reading kernel/service/process/boot state; no stop, start, unload, kill or app launch is performed.'
    $pre = Get-P06KernelState -InstallRoot $InstallRoot -WorkDir $WorkDir -SkipWfp:$SkipWfp
    $preFingerprint = Get-P06KernelFingerprint -KernelState $pre
    # Re-read immediately: a read-only preflight must not change the state it read.
    $post = Get-P06KernelState -InstallRoot $InstallRoot -WorkDir $WorkDir -SkipWfp:$SkipWfp
    $postFingerprint = Get-P06KernelFingerprint -KernelState $post
    $unchanged = ($preFingerprint -eq $postFingerprint)
    # WFP counts are reported and compared for evidence, but unrelated system
    # activity may legitimately change them; they are not the proof that this
    # read-only harness changed nothing (service/driver/pids are).
    $wfpPreFingerprint = Get-P06WfpFingerprint -KernelState $pre
    $wfpPostFingerprint = Get-P06WfpFingerprint -KernelState $post
    $wfpConsistent = ($wfpPreFingerprint -eq $wfpPostFingerprint)
    if (-not $wfpConsistent) {
        Write-P06Log 'WFP counts differed between the two read-only reads (reported, not a block; service/driver/pids are the no-mutation proof).' 'WARN'
    }
    $service = $pre.service
    Write-P06Log ('sc query WinDivert: state={0} win32_exit={1} start_type={2}' -f (Get-P06Field -Object $service -Name 'state' -Default 'unknown'), (Get-P06Field -Object $service -Name 'win32_exit_code' -Default $null), (Get-P06Field -Object $service -Name 'start_type' -Default $null))
    Write-P06Log ('driver path: {0}' -f (Get-P06Field -Object $service -Name 'driver_path' -Default 'not reported'))
    Write-P06Log ('boot: {0} (id {1}, uptime {2}s)' -f (Get-P06Field -Object $pre.boot -Name 'boot_time_utc' -Default 'unknown'), (Get-P06Field -Object $pre.boot -Name 'boot_id' -Default 'unknown'), (Get-P06Field -Object $pre.boot -Name 'uptime_seconds' -Default 'unknown'))
    Write-P06Log ('LightSpeed processes: {0}' -f $pre.process_count)
    $wfp = $pre.wfp
    if (Get-P06Field -Object $wfp -Name 'available' -Default $false) {
        $wd = Get-P06Field -Object $wfp -Name 'windivert' -Default $null
        Write-P06Log ('WFP: WinDivert provider={0} sublayers={1} filters={2} callouts={3} (read-only netsh enumeration)' -f (Get-P06Field -Object $wd -Name 'providers' -Default '?'), (Get-P06Field -Object $wd -Name 'subLayers' -Default '?'), (Get-P06Field -Object $wd -Name 'filters' -Default '?'), (Get-P06Field -Object $wd -Name 'callouts' -Default '?'))
    } else {
        Write-P06Log ('WFP: unavailable ({0})' -f (Get-P06Field -Object $wfp -Name 'error' -Default 'no reason'))
    }
    Write-P06Log ('lifecycle.jsonl present: {0}; unclean marker: {1}' -f (Get-P06Field -Object $pre.lifecycle -Name 'lifecycle_jsonl_present' -Default '?'), (Get-P06Field -Object $pre.lifecycle -Name 'unclean_marker_present' -Default '?'))
    if (-not $unchanged) {
        Write-P06Log 'Kernel fingerprint changed between two read-only reads; refusing to trust this preflight.' 'BLOCK'
    }
    foreach ($reason in @($pre.dirty_reasons)) {
        Write-P06Log ('dirty: ' + $reason) 'BLOCK'
    }
    if (-not $pre.dirty) {
        Write-P06Log 'kernel clean: no WinDivert service RUNNING and no LightSpeed process' 'PASS'
    }
    return [ordered]@{
        schema = 'p0-6-preflight/v1'
        checked_at_utc = (Get-P06UtcNow)
        install_root = $InstallRoot
        read_only = $true
        forbidden_operations_performed = @()
        pre_state = $pre
        post_state = $post
        pre_fingerprint = $preFingerprint
        post_fingerprint = $postFingerprint
        wfp_pre_fingerprint = $wfpPreFingerprint
        wfp_post_fingerprint = $wfpPostFingerprint
        wfp_pre_post_equal = $wfpConsistent
        wfp_pre_available = (Get-P06Field -Object $pre.wfp -Name 'available' -Default $false)
        wfp_post_available = (Get-P06Field -Object $post.wfp -Name 'available' -Default $false)
        no_state_change = $unchanged
        kernel_state = $pre.kernel_state
        clean = $pre.clean
        dirty = $pre.dirty
        dirty_reasons = @($pre.dirty_reasons)
        block = (-not $pre.clean -or -not $unchanged)
        block_reason = $(if ($pre.dirty) { 'dirty kernel: ' + (@($pre.dirty_reasons) -join '; ') } elseif (-not $unchanged) { 'kernel state changed during read-only preflight' } else { $null })
        wfp_counts = $wfp.windivert
        wfp_totals = $wfp.totals
        wfp_available = $wfp.available
        boot = $pre.boot
    }
}
