# P06Rows.ps1 - P0-6 row scaffolding and dry-run matrix.
#
# The rows are the P0 gate's acceptance matrix (docs/audit/stability-verification-
# 2026-11-23.md section 4 A1-A12 and roadmap section 5/6.2). In this round the
# harness can only scaffold them: the host kernel is dirty and the owner OD-6
# decision defers the real-driver rows to a clean boot. Therefore:
#   * NO row may be PASS in this round;
#   * an unrun row is U, a precondition-blocked row is BLOCKED;
#   * a row whose only remediation would be a reboot is a FAIL by definition
#     (the matrix may not go green by rebooting).
#
# Every row record carries the fields the audit requires: build hash, timestamps,
# handle ids, pin state and, where applicable, the WFP counts observed at the
# phase boundary.

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

function Get-P06RowDefinitions {
    return @(
        [ordered]@{
            id = 'A1'
            title = 'Clean-boot first start'
            phase = 'P0'
            requires = @('clean_boot', 'app_launch', 'game_presence')
            needs_clean_boot = $true
            needs_app_launch = $true
            steps = @(
                'clean boot; no WinDivert service RUNNING, no LightSpeed process',
                'record identity block and WFP baseline (0 diverting filters/callouts at idle)',
                'launch the exact candidate artifact; game present; relay connect',
                'capture two healthy judged windows'
            )
            assertions = @(
                'pin opens exactly once',
                'two healthy judged windows with injected > 0 and from_relay > 0',
                'WFP baseline: zero diverting filters/callouts at idle',
                'app_start identity equals the artifact identity block'
            )
            evidence = @('lifecycle.jsonl', 'WFP pre/post snapshots', 'UI state dump', 'run.json identity block')
        }
        [ordered]@{
            id = 'A2'
            title = 'Traffic-bearing settlement/POI transition'
            phase = 'P0'
            requires = @('clean_boot', 'app_launch', 'two_endpoints', 'fixture')
            needs_clean_boot = $true
            needs_app_launch = $true
            steps = @(
                'two reachable game-server endpoints that both send and receive',
                'drive a mid-session endpoint switch A -> B',
                'capture the lifecycle trace across the transition'
            )
            assertions = @(
                'ZERO capture teardowns in the transition window: no windivert_close for intercept/inject/shadow/legacy and no interceptor_state leaving boosting',
                'no windivert_open with reason server_transition/stale_server/relay_switch',
                'registration ledger: old session superseded, exactly one active session',
                'at least two healthy two-way judged windows after the switch'
            )
            evidence = @('lifecycle.jsonl', 'handle/registration ledger', 'WFP snapshot after switch', 'relay-side session log')
        }
        [ordered]@{
            id = 'A4'
            title = 'Stop while degraded (field shape)'
            phase = 'P0'
            requires = @('clean_boot', 'app_launch', 'fixture')
            needs_clean_boot = $true
            needs_app_launch = $true
            steps = @(
                'drive four consecutive no-reply windows -> relay demotion',
                'Stop then immediate Start (<= 1.5 s); repeat at 3 s and 10 s'
            )
            assertions = @(
                'reopen succeeds every time (zero FWP_E_IN_USE, zero failed opens) or a documented truthful non-reboot remedy exists',
                'every handle id pairs one close per open',
                'error card clears on the successful retry',
                'no reboot required to pass'
            )
            evidence = @('lifecycle.jsonl', 'WFP snapshots per attempt', 'UI captures')
        }
        [ordered]@{
            id = 'A5'
            title = 'Fast-restart timing matrix'
            phase = 'P0'
            requires = @('clean_boot', 'app_launch', 'fixture')
            needs_clean_boot = $true
            needs_app_launch = $true
            steps = @(
                'Optimize at 250 ms, 1 s, 2 s, 5 s after Stop, ten repetitions each',
                'record the close->open age and FWP code per attempt'
            )
            assertions = @(
                'every open either succeeds or fails into a documented state',
                'no non-deterministic mix across repetitions',
                'handle ids increment correctly and pair one close per open',
                'zero FWP_E_IN_USE outside the documented state'
            )
            evidence = @('lifecycle.jsonl with timing annotations', 'harness timing log', 'WFP snapshots')
        }
        [ordered]@{
            id = 'A6'
            title = 'Repeated long-session transitions'
            phase = 'P1-reserved/P0-soak'
            requires = @('clean_boot', 'app_launch', 'long_session', 'two_endpoints')
            needs_clean_boot = $true
            needs_app_launch = $true
            steps = @(
                '>= 60 minutes boosting with a working return path',
                '>= 10 server transitions, >= 20 in-place re-registrations, >= 5 relay switches',
                'periodic handle/thread/RSS and WFP object snapshots'
            )
            assertions = @(
                'zero FWP_E_IN_USE and zero failed opens',
                'handle/fd/thread counts stable, no monotonic growth',
                'registration active-session count <= 1 throughout',
                'zero error cards, healthy-window share maintained after each transition'
            )
            evidence = @('60-minute trace', 'periodic WFP/handle snapshots', 'resource samples')
        }
        [ordered]@{
            id = 'A8'
            title = 'Stale error recovery (functional and UI truth)'
            phase = 'P0'
            requires = @('clean_boot', 'app_launch', 'fixture')
            needs_clean_boot = $true
            needs_app_launch = $true
            steps = @(
                'induce a controlled StartBoost failure',
                'then perform a successful StartBoost',
                'capture the UI semantic dump before and after'
            )
            assertions = @(
                'error card reflects the failure while it is live',
                'on success the same-class card clears within 1 s (ui_error clear with cleared_by)',
                'unrelated errors survive the clear',
                'a superseded registration generation never paints a failure card'
            )
            evidence = @('ui_error lifecycle from lifecycle.jsonl', 'UI semantic dumps before/after', 'screenshots')
        }
        [ordered]@{
            id = 'A11'
            title = 'Dirty-kernel recovery (the inversion)'
            phase = 'P0'
            requires = @('dirty_kernel', 'app_launch')
            needs_clean_boot = $false
            needs_app_launch = $true
            steps = @(
                'start from the state the product itself leaves: driver loaded, WFP provider 1 / sublayers 20, no process holding handles',
                'launch the candidate and attempt interception'
            )
            assertions = @(
                'product opens cleanly on first attempt (or after a bounded logged retry) WITHOUT reboot or sc stop',
                'if it cannot: detect the state, name it accurately, offer a bounded automated remedy, log the evidence',
                '"restart Windows" is never the only remedy',
                'a reboot can never convert this row: reboot-to-pass is FAIL by definition'
            )
            evidence = @('pre-state WFP/service snapshot', 'lifecycle.jsonl retry records', 'post-state snapshot', 'UI')
        }
        [ordered]@{
            id = 'A12'
            title = 'Build/artifact identity'
            phase = 'P0'
            requires = @('clean_boot', 'app_launch', 'identity_gate')
            needs_clean_boot = $true
            needs_app_launch = $true
            steps = @(
                'record the candidate GUI/proxy/WinDivert.dll/WinDivert64.sys SHA256 and release tag/commit',
                'compare to the published release asset sha256.sum',
                'verify the app logs the same identity at startup',
                'verify the shipped installer would contain the same binary'
            )
            assertions = @(
                'identity gate matches installed/candidate artifacts to the published release',
                'app_start in lifecycle.jsonl logs the same version/commit/driver hashes',
                'a mismatch BLOCKS the run instead of warning',
                'release notes can carry the tested hashes'
            )
            evidence = @('identity.json', 'app_start line from lifecycle.jsonl', 'release sha256.sum', 'package diff')
        }
    )
}

function New-P06RowRecord {
    param(
        $Definition,
        $IdentityGate,
        $Preflight,
        $LifecycleAudit,
        [string]$Status,
        [string]$StatusReason,
        $IdentityPrerequisite = $null
    )
    $expected = Get-P06Field -Object $IdentityGate -Name 'expected' -Default $null
    $installed = Get-P06Field -Object $IdentityGate -Name 'installed' -Default $null
    $gui = Get-P06Field -Object $installed -Name 'gui_exe' -Default $null
    $boot = Get-P06Field -Object $Preflight -Name 'boot' -Default $null
    $wfpCounts = $null
    $wfpAvailable = $false
    if ($null -ne $Preflight) {
        $wfpCounts = Get-P06Field -Object $Preflight -Name 'wfp_counts' -Default $null
        $wfpAvailable = [bool](Get-P06Field -Object $Preflight -Name 'wfp_available' -Default $false)
    }
    $started = Get-P06UtcNow
    $record = [ordered]@{
        schema = 'p0-6-row/v1'
        row_id = (Get-P06Field -Object $Definition -Name 'id' -Default '?')
        title = (Get-P06Field -Object $Definition -Name 'title' -Default '?')
        phase = (Get-P06Field -Object $Definition -Name 'phase' -Default '?')
        status = $Status
        status_reason = $StatusReason
        status_is_final = ($Status -eq 'PASS' -or $Status -eq 'FAIL')
        started_utc = $started
        finished_utc = (Get-P06UtcNow)
        artifact = [ordered]@{
            identity_gate_status = (Get-P06Field -Object $IdentityGate -Name 'status' -Default 'unavailable')
            gui_exe_sha256 = (Get-P06Field -Object $gui -Name 'sha256' -Default $null)
            version = (Get-P06Field -Object $expected -Name 'version' -Default $null)
            commit = (Get-P06Field -Object $expected -Name 'release_target_commit' -Default $null)
            build_string = (Get-P06Field -Object $expected -Name 'build_string' -Default $null)
            expected_release_zip_sha256 = (Get-P06Field -Object $expected -Name 'release_zip_sha256' -Default $null)
        }
        boot = [ordered]@{
            boot_time_utc = (Get-P06Field -Object $boot -Name 'boot_time_utc' -Default $null)
            boot_id = (Get-P06Field -Object $boot -Name 'boot_id' -Default $null)
            uptime_seconds = (Get-P06Field -Object $boot -Name 'uptime_seconds' -Default $null)
        }
        environment = [ordered]@{
            kernel_state = (Get-P06Field -Object $Preflight -Name 'kernel_state' -Default 'unknown')
            kernel_clean = (Get-P06Field -Object $Preflight -Name 'clean' -Default $null)
            dirty_reasons = @(Get-P06Field -Object $Preflight -Name 'dirty_reasons' -Default @())
            app_launched = $false
            state_changing_actions = @()
        }
        handle_ids = [ordered]@{
            open = @()
            close = @()
            active = @()
            note = 'no real-driver run in P0-6a; handle ids are recorded only by a run'
        }
        pin_state = [ordered]@{
            observed = $false
            active = $null
            open_ids = @()
            close_ids = @()
        }
        wfp_counts = [ordered]@{
            applicable = ($wfpAvailable)
            snapshot_role = 'pre_run_observed'
            windivert = $wfpCounts
            available = $wfpAvailable
        }
        assertions = @(Get-P06Field -Object $Definition -Name 'assertions' -Default @())
        steps = @(Get-P06Field -Object $Definition -Name 'steps' -Default @())
        evidence_required = @(Get-P06Field -Object $Definition -Name 'evidence' -Default @())
        lifecycle = [ordered]@{
            consumed = ($null -ne $LifecycleAudit)
            record_count = $(if ($null -ne $LifecycleAudit) { Get-P06Field -Object $LifecycleAudit -Name 'record_count' -Default 0 } else { 0 })
            a2_transition = $(if ($null -ne $LifecycleAudit) { Get-P06Field -Object $LifecycleAudit -Name 'a2_transition' -Default $null } else { $null })
        }
        identity_prerequisite = $IdentityPrerequisite
        reboot_to_pass = $false
        notes = @()
    }
    if ((Get-P06Field -Object $Definition -Name 'id' -Default '') -eq 'A11') {
        $record.notes += 'A reboot can never turn A11 green: requiring a reboot to pass is a FAIL by definition.'
    }
    return $record
}

function Invoke-P06RowMatrix {
    param(
        $IdentityGate,
        $Preflight,
        $LifecycleAudit,
        [string]$RowLogPath,
        [string]$SummaryPath,
        [switch]$AllowStateChangingRows
    )
    $definitions = Get-P06RowDefinitions
    if (Test-Path -LiteralPath $RowLogPath) { Remove-Item -LiteralPath $RowLogPath -Force }
    $records = @()
    $identityStatus = [string](Get-P06Field -Object $IdentityGate -Name 'status' -Default 'unavailable')
    $kernelClean = [bool](Get-P06Field -Object $Preflight -Name 'clean' -Default $false)
    $dirtyReasons = @(Get-P06Field -Object $Preflight -Name 'dirty_reasons' -Default @())
    foreach ($definition in $definitions) {
        $id = [string](Get-P06Field -Object $definition -Name 'id' -Default '?')
        $needsApp = [bool](Get-P06Field -Object $definition -Name 'needs_app_launch' -Default $true)
        $needsClean = [bool](Get-P06Field -Object $definition -Name 'needs_clean_boot' -Default $false)
        $status = 'U'
        $reason = $null
        $identityPrereq = $null
        if ($identityStatus -ne 'match') {
            $status = 'BLOCKED'
            $reason = 'artifact identity is not established (' + $identityStatus + '); identity first, no row may run'
        } elseif ($id -eq 'A12') {
            # A12 is the one row with a part that is verifiable now: the artifact
            # identity gate. The runtime startup-log half is not run, so the row
            # itself remains U, never PASS.
            $status = 'U'
            $identityPrereq = [ordered]@{
                identity_gate = $identityStatus
                installed_matches_published_release = ($identityStatus -eq 'match')
                expected_build_string = (Get-P06Field -Object (Get-P06Field -Object $IdentityGate -Name 'expected' -Default $null) -Name 'build_string' -Default $null)
                runtime_app_start_identity_logged = $false
                release_zip_members_verified = $(if ($null -ne (Get-P06Field -Object $IdentityGate -Name 'release_zip_verification' -Default $null)) { [bool](Get-P06Field -Object (Get-P06Field -Object $IdentityGate -Name 'release_zip_verification' -Default $null) -Name 'ok' -Default $false) } else { $null })
            }
            $reason = 'identity gate is executable and recorded, but the app_start startup-log half requires a launch of the exact candidate (deferred to the clean boot); row cannot be PASS'
        } elseif ($id -eq 'A11') {
            $status = 'BLOCKED'
            $reason = 'A11 needs the product-created dirty state AND an authorised app launch. P0-6a launched no app and this host dirty state is not attributable to the product; the row must be re-run from a clean boot to the known product state. A reboot can never turn this row green.'
        } elseif ($needsClean -and -not $kernelClean) {
            $status = 'BLOCKED'
            $reason = 'dirty kernel (' + (@($dirtyReasons) -join '; ') + '); the row must run after a clean boot and the harness must never stop the service or unload the driver to force a run'
        } elseif ($needsApp -and -not $AllowStateChangingRows) {
            $status = 'U'
            $reason = 'not run: P0-6a scaffolding does not launch the app or exercise traffic; it needs the owner-authorised clean-boot round'
        }
        if ($status -eq 'PASS') {
            throw ('row {0} produced PASS in scaffolding mode; a row may only pass after a real run and re-run' -f $id)
        }
        $record = New-P06RowRecord -Definition $definition -IdentityGate $IdentityGate -Preflight $Preflight -LifecycleAudit $LifecycleAudit -Status $status -StatusReason $reason -IdentityPrerequisite $identityPrereq
        $records += $record
        Write-P06Jsonl -Path $RowLogPath -Record $record
        Write-P06Log ('row {0} {1}: {2}' -f $record.row_id, $record.status, $record.title)
    }
    $counts = [ordered]@{ PASS = 0; FAIL = 0; U = 0; BLOCKED = 0 }
    foreach ($record in $records) {
        $counts[$record.status] = [int]$counts[$record.status] + 1
    }
    $uList = @()
    $blockedList = @()
    foreach ($record in $records) {
        if ($record.status -eq 'U') {
            $uList += [ordered]@{ row_id = $record.row_id; title = $record.title; reason = $record.status_reason; what_would_verify_it = @($record.evidence_required) }
        }
        if ($record.status -eq 'BLOCKED') {
            $blockedList += [ordered]@{ row_id = $record.row_id; title = $record.title; reason = $record.status_reason }
        }
    }
    $summary = [ordered]@{
        schema = 'p0-6-row-summary/v1'
        ran_at_utc = (Get-P06UtcNow)
        mode = 'scaffolding/dry-run'
        row_count = @($records).Count
        counts = $counts
        pass_count = $counts.PASS
        real_driver_gate = 'NOT PASSED'
        gate_verdict = 'NOT_PASSED'
        gate_statement = 'No real-driver row was executed. The P0-6 gate is NOT passed and no stability claim is supported by this evidence.'
        reboot_to_pass_rows = @($records | Where-Object { $_.reboot_to_pass }).Count
        u_list = $uList
        blocked_list = $blockedList
        rows = @($records | ForEach-Object { [ordered]@{ row_id = $_.row_id; title = $_.title; status = $_.status; status_reason = $_.status_reason } })
    }
    if (Test-Path -LiteralPath $SummaryPath) { Remove-Item -LiteralPath $SummaryPath -Force }
    Write-P06JsonFile -Path $SummaryPath -Object $summary
    return [ordered]@{
        rows = @($records)
        summary = $summary
        row_log_path = $RowLogPath
        summary_path = $SummaryPath
    }
}
