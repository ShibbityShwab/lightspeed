# Invoke-P06Gate.ps1 - P0-6 WinDivert stability evidence gate (scaffolding round).
#
# Usage (PowerShell 5.1, from the worktree root):
#   .\scripts\windivert-stability\Invoke-P06Gate.ps1 -Mode SelfCheck
#   .\scripts\windivert-stability\Invoke-P06Gate.ps1 -Mode SelfCheck -VerifyReleaseZip
#   .\scripts\windivert-stability\Invoke-P06Gate.ps1 -Mode Replay -LifecyclePath <path>
#
# Modes:
#   SelfCheck (default) - identity gate + read-only clean-boot preflight + lifecycle
#                         consumer self-test + dry-run A1/A2/A4/A5/A6/A8/A11/A12
#                         matrix. Writes every artifact under the evidence dir.
#   DryRun              - alias of SelfCheck.
#   Replay              - evaluate the checked row assertions against a stored
#                         lifecycle.jsonl from a real run (no app launch).
#   Run                 - real-driver execution. NOT implemented in this round
#                         (owner OD-6: rows are deferred to a clean boot). It
#                         always exits BLOCKED and never launches the app.
#
# Exit codes:
#   0  self-check completed and the environment was clean (rows still U; no gate pass)
#   1  harness/internal error
#   2  BLOCKED: dirty kernel
#   3  BLOCKED: artifact identity mismatch/unavailable
#   4  BLOCKED: lifecycle consumer self-test failed
#   5  BLOCKED: unsupported mode / missing authorisation (Run never launches here)
#
# Safety: this script does not start/stop the app or the WinDivert service, does
# not unload the driver, and does not touch WFP state. Its only writes are its own
# evidence files and, with -VerifyReleaseZip, a temporary download that is removed.

[CmdletBinding()]
param(
    [ValidateSet('SelfCheck', 'DryRun', 'Replay', 'Run')]
    [string]$Mode = 'SelfCheck',
    [string]$EvidenceDir = $null,
    [string]$InstalledRoot = $null,
    [string]$CandidateRoot = $null,
    [string]$BaselinePath = $null,
    [string]$Tag = 'v1.12.5',
    [switch]$NoNetwork,
    [switch]$VerifyReleaseZip,
    [switch]$SkipWfp,
    [string]$LifecyclePath = $null,
    [switch]$AllowStateChangingRows,
    [switch]$AuthorizeAppLaunch
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

$scriptRoot = $PSScriptRoot
. (Join-Path $scriptRoot 'P06Common.ps1')
. (Join-Path $scriptRoot 'P06Identity.ps1')
. (Join-Path $scriptRoot 'P06Preflight.ps1')
. (Join-Path $scriptRoot 'P06Lifecycle.ps1')
. (Join-Path $scriptRoot 'P06Rows.ps1')

if ([string]::IsNullOrWhiteSpace($EvidenceDir)) {
    $EvidenceDir = $script:P06DefaultEvidenceDir
}
if ([string]::IsNullOrWhiteSpace($InstalledRoot)) {
    $InstalledRoot = $script:P06DefaultInstalledRoot
}
$EvidenceDir = [System.IO.Path]::GetFullPath($EvidenceDir)
if (-not (Test-Path -LiteralPath $EvidenceDir)) {
    New-Item -ItemType Directory -Path $EvidenceDir -Force | Out-Null
}
$stamp = Get-P06UtcStamp
$transcriptPath = Join-Path $EvidenceDir ('transcript-' + $stamp + '.txt')
$stableTranscriptPath = Join-Path $EvidenceDir 'selfcheck-transcript.txt'
Set-P06Transcript -Path $transcriptPath

$modeLabel = $Mode
if ($Mode -eq 'DryRun') { $modeLabel = 'SelfCheck' }

$blocks = @()

function Add-P06Block {
    param(
        [string]$Reason,
        [int]$ExitCode
    )
    $script:blocks += [ordered]@{ reason = $Reason; exit_code = $ExitCode }
}

try {
    Write-P06Section ('P0-6 harness ' + $script:P06HarnessVersion + ' - mode ' + $modeLabel)
    Write-P06Log ('repo root: ' + $script:P06RepoRoot)
    Write-P06Log ('evidence dir: ' + $EvidenceDir)
    Write-P06Log ('installed root: ' + $InstalledRoot)
    Write-P06Log 'safety: read-only; no app launch, no WinDivert service/driver/WFP mutation'
    $git = Get-P06GitFacts
    Write-P06Log ('git head={0} branch={1} dirty={2}' -f (Get-P06Field -Object $git -Name 'head_commit' -Default '?'), (Get-P06Field -Object $git -Name 'branch' -Default '?'), (Get-P06Field -Object $git -Name 'worktree_dirty' -Default '?'))

    if ($Mode -eq 'Run') {
        Add-P06Block -Reason 'Run mode is not implemented in the P0-6a scaffolding round: the real-driver rows are deferred to the owner-authorised clean boot (OD-6). This harness will never launch the app to improvise them.' -ExitCode 5
    }
    if ($Mode -eq 'Replay' -and [string]::IsNullOrWhiteSpace($LifecyclePath)) {
        Add-P06Block -Reason 'Replay mode requires -LifecyclePath pointing at a stored lifecycle.jsonl' -ExitCode 5
    }

    # -- 1. Artifact identity gate (blocks on mismatch) ----------------------
    Write-P06Section 'Artifact identity gate'
    $identity = Invoke-P06IdentityGate `
        -InstalledRoot $InstalledRoot `
        -CandidateRoot $CandidateRoot `
        -BaselinePath $BaselinePath `
        -Tag $Tag `
        -NoNetwork:$NoNetwork `
        -VerifyReleaseZip:$VerifyReleaseZip
    Write-P06JsonFile -Path (Join-Path $EvidenceDir 'identity.json') -Object $identity
    Write-P06Log ('identity status: {0} (source: {1})' -f $identity.status, $identity.identity_source)
    foreach ($comparison in @($identity.comparisons)) {
        $level = 'BLOCK'
        if ($comparison.result -eq 'match') { $level = 'PASS' }
        Write-P06Log ('  {0}: {1}' -f $comparison.name, $comparison.result) $level
    }
    if ($null -ne $identity.release_zip_verification) {
        Write-P06Log ('release zip verification: ok={0} zip_sha256_matches={1} members={2}' -f $identity.release_zip_verification.ok, $identity.release_zip_verification.zip_sha256_matches, (@($identity.release_zip_verification.members.Keys) -join ','))
    }
    if ($identity.status -ne 'match') {
        Add-P06Block -Reason ('artifact identity ' + $identity.status + ': ' + $identity.block_reason) -ExitCode 3
    } else {
        Write-P06Log 'artifact identity matches the published v1.12.5 release asset' 'PASS'
    }

    # -- 2. Read-only clean-boot preflight (blocks on dirty kernel) ----------
    Write-P06Section 'Clean-boot preflight'
    $wfpWorkDir = Join-Path $EvidenceDir 'wfp'
    $preflight = Invoke-P06Preflight -InstallRoot $InstalledRoot -WorkDir $wfpWorkDir -SkipWfp:$SkipWfp
    Write-P06JsonFile -Path (Join-Path $EvidenceDir 'preflight.json') -Object $preflight
    if ($preflight.dirty) {
        Add-P06Block -Reason ('dirty kernel (' + (@($preflight.dirty_reasons) -join '; ') + ')') -ExitCode 2
    }
    if (-not $preflight.no_state_change) {
        Add-P06Block -Reason 'kernel state changed between two read-only preflight reads; a run cannot be trusted' -ExitCode 5
    }

    # -- 3. Lifecycle consumer self-test (proves the assertions can fail) ----
    Write-P06Section 'Lifecycle consumer self-test (synthetic landed-schema fixtures)'
    $selfTestDir = Join-Path $EvidenceDir 'lifecycle-selftest'
    $selfTest = Invoke-P06LifecycleSelfTest -WorkDir $selfTestDir
    Write-P06JsonFile -Path (Join-Path $EvidenceDir 'lifecycle-selftest.json') -Object $selfTest
    foreach ($check in @($selfTest.checks)) {
        $level = 'FAIL'
        if ($check.pass) { $level = 'PASS' }
        Write-P06Log ('  {0}: {1} (expected {2}, actual {3})' -f $check.name, $(if ($check.pass) { 'ok' } else { 'FAILED' }), $check.expected, $check.actual) $level
    }
    if (-not $selfTest.ok) {
        Add-P06Block -Reason ('lifecycle consumer self-test failed: {0} of {1} checks' -f $selfTest.failed_count, $selfTest.check_count) -ExitCode 4
    } else {
        Write-P06Log ('lifecycle consumer self-test: {0}/{1} checks passed' -f ($selfTest.check_count - $selfTest.failed_count), $selfTest.check_count) 'PASS'
    }

    # -- Optional replay of a stored real-run lifecycle trace ----------------
    $lifecycleAudit = $null
    if (-not [string]::IsNullOrWhiteSpace($LifecyclePath)) {
        Write-P06Section ('Lifecycle replay: ' + $LifecyclePath)
        $lifecycleAudit = Invoke-P06LifecycleAudit -Path $LifecyclePath -IdentityGate $identity
        $auditName = 'lifecycle-audit-' + (Get-P06UtcStamp) + '.json'
        Write-P06JsonFile -Path (Join-Path $EvidenceDir $auditName) -Object $lifecycleAudit
        Write-P06Log ('lifecycle records={0} a2={1} handle_ledger_ok={2} registration_ledger_ok={3}' -f $lifecycleAudit.record_count, $lifecycleAudit.a2_transition.status, $lifecycleAudit.handle_ledger.ok, $lifecycleAudit.registration_ledger.ok)
        Write-P06JsonFile -Path (Join-Path $EvidenceDir 'lifecycle-audit.json') -Object $lifecycleAudit
    }

    # -- 4. Dry-run row matrix (no row may PASS) -----------------------------
    Write-P06Section 'P0-6 dry-run row matrix (A1/A2/A4/A5/A6/A8/A11/A12)'
    $rowMatrix = Invoke-P06RowMatrix `
        -IdentityGate $identity `
        -Preflight $preflight `
        -LifecycleAudit $lifecycleAudit `
        -RowLogPath (Join-Path $EvidenceDir 'rows.jsonl') `
        -SummaryPath (Join-Path $EvidenceDir 'row-summary.json') `
        -AllowStateChangingRows:$AllowStateChangingRows
    Write-P06Log ('row statuses: PASS={0} FAIL={1} U={2} BLOCKED={3}' -f $rowMatrix.summary.counts.PASS, $rowMatrix.summary.counts.FAIL, $rowMatrix.summary.counts.U, $rowMatrix.summary.counts.BLOCKED)
    Write-P06Log ('real-driver gate: ' + $rowMatrix.summary.real_driver_gate) 'BLOCK'

    # -- Run record (identity block + environment; A12 consumes it) ---------
    $runRecord = [ordered]@{
        schema = 'p0-6-run/v1'
        harness_version = $script:P06HarnessVersion
        mode = $modeLabel
        requested_mode = $Mode
        started_utc = (Get-P06Field -Object $identity -Name 'checked_at_utc' -Default (Get-P06UtcNow))
        finished_utc = (Get-P06UtcNow)
        repo_root = $script:P06RepoRoot
        evidence_dir = $EvidenceDir
        installed_root = $InstalledRoot
        candidate_root = $CandidateRoot
        git = $git
        identity = [ordered]@{
            status = $identity.status
            gate = $identity.gate
            source = $identity.identity_source
            expected = $identity.expected
            comparisons = $identity.comparisons
            installed = $identity.installed
            candidate = $identity.candidate
            release = $identity.release
        }
        preflight = [ordered]@{
            kernel_state = $preflight.kernel_state
            clean = $preflight.clean
            no_state_change = $preflight.no_state_change
            dirty_reasons = $preflight.dirty_reasons
            boot = $preflight.boot
            wfp_counts = $preflight.wfp_counts
        }
        lifecycle_selftest = [ordered]@{
            ok = $selfTest.ok
            check_count = $selfTest.check_count
            failed_count = $selfTest.failed_count
        }
        rows = $rowMatrix.summary
        gate = [ordered]@{
            real_driver_gate = 'NOT PASSED'
            verdict = 'NOT_PASSED'
            statement = 'No real-driver row was executed on this host. The P0-6 gate is not passed and this evidence supports no stability claim.'
        }
        blocks = @()
    }
    $exitCode = 0
    if (@($blocks).Count -gt 0) {
        $runRecord.blocks = @($blocks)
        # Precedence, not numeric minimum: identity first (an unidentifiable
        # artifact invalidates the run), then dirty kernel, then a failed
        # consumer self-test, then an unsupported mode.
        $codes = @($blocks | ForEach-Object { [int]$_.exit_code })
        foreach ($candidate in @(3, 2, 4, 5)) {
            if ($codes -contains $candidate) {
                $exitCode = $candidate
                break
            }
        }
        if ($exitCode -eq 0) { $exitCode = $codes[0] }
    }
    Write-P06JsonFile -Path (Join-Path $EvidenceDir 'run.json') -Object $runRecord

    $selfCheckRecord = [ordered]@{
        schema = 'p0-6-selfcheck/v1'
        harness_version = $script:P06HarnessVersion
        mode = $modeLabel
        finished_utc = (Get-P06UtcNow)
        exit_code = $exitCode
        identity = [ordered]@{ status = $identity.status; gate = $identity.gate; source = $identity.identity_source; block_reason = $identity.block_reason }
        preflight = [ordered]@{ kernel_state = $preflight.kernel_state; clean = $preflight.clean; no_state_change = $preflight.no_state_change; dirty_reasons = $preflight.dirty_reasons }
        lifecycle_selftest = [ordered]@{ ok = $selfTest.ok; check_count = $selfTest.check_count; failed_count = $selfTest.failed_count }
        rows = [ordered]@{ counts = $rowMatrix.summary.counts; real_driver_gate = $rowMatrix.summary.real_driver_gate; u_list = $rowMatrix.summary.u_list; blocked_list = $rowMatrix.summary.blocked_list }
        blocks = @($blocks)
        verifiable_now = @(
            'exact-artifact identity block (installed GUI / WinDivert.dll / WinDivert64.sys vs published v1.12.5 release sha256.sum and zip members)',
            'read-only kernel/service/process/boot state and WFP counts',
            'lifecycle.jsonl field-based consumer + A2 zero-teardown assertion, proven able to fail on synthetic fixtures',
            'the WFP-observed truth that this host is dirty (provider 1 / sublayers 20 / 0 diverting filters at idle)'
        )
        needs_clean_boot = @(
            'A1 clean-boot first start',
            'A2 two-endpoint transition with zero capture teardown',
            'A4 Stop-while-degraded and A5 fast-restart matrix',
            'A6 long session (>=60 min, >=10 transitions)',
            'A8 stale-card recovery',
            'A11 dirty-kernel recovery from the product-created state',
            'A12 runtime app_start identity half'
        )
        gate = [ordered]@{ real_driver_gate = 'NOT PASSED'; verdict = 'NOT_PASSED'; statement = 'The real-driver gate is not passed. No stability claim is supported.' }
    }
    Write-P06JsonFile -Path (Join-Path $EvidenceDir 'selfcheck.json') -Object $selfCheckRecord

    Write-P06Section 'Result'
    Write-P06Log ('gate verdict: NOT PASSED (rows executed: 0 of ' + $rowMatrix.summary.row_count + ')') 'BLOCK'
    Write-P06Log ('U rows: {0}; BLOCKED rows: {1}' -f @($rowMatrix.summary.u_list).Count, @($rowMatrix.summary.blocked_list).Count)
    foreach ($block in @($blocks)) {
        if ($block.exit_code -eq 2) {
            Write-P06Log 'BLOCKED: dirty kernel' 'BLOCK'
        } else {
            Write-P06Log ('BLOCKED: ' + $block.reason) 'BLOCK'
        }
    }
    if (@($blocks).Count -eq 0) {
        Write-P06Log 'self-check complete on a clean environment; rows remain U until the authorised run' 'PASS'
    }
    Set-P06Transcript -Path $null
    if (Test-Path -LiteralPath $transcriptPath) {
        Copy-Item -LiteralPath $transcriptPath -Destination $stableTranscriptPath -Force
    }
    Write-Host ''
    Write-Host ('EXIT={0}' -f $exitCode)
    exit $exitCode
} catch {
    $message = $_.Exception.Message
    try {
        Write-P06Log ('HARNESS ERROR: ' + $message) 'FAIL'
        Write-P06Log ($_.ScriptStackTrace) 'FAIL'
        $errorRecord = [ordered]@{
            schema = 'p0-6-harness-error/v1'
            finished_utc = (Get-P06UtcNow)
            message = $message
            script_stack = $_.ScriptStackTrace
            position = $_.InvocationInfo.PositionMessage
        }
        Write-P06JsonFile -Path (Join-Path $EvidenceDir 'harness-error.json') -Object $errorRecord
    } catch {
        Write-Host ('HARNESS ERROR (and error record failed): ' + $message)
    }
    Set-P06Transcript -Path $null
    if (Test-Path -LiteralPath $transcriptPath) {
        Copy-Item -LiteralPath $transcriptPath -Destination $stableTranscriptPath -Force -ErrorAction SilentlyContinue
    }
    exit 1
}
