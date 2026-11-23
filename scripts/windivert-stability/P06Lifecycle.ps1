# P06Lifecycle.ps1 - field-based consumer for the P0-1 `lifecycle.jsonl`.
#
# This file parses the machine-readable lifecycle sink by FIELDS, never by prose,
# and carries the behavioural assertions the P0-6 rows need:
#   * base record schema (`ts`, `ts_ms`, `app_session`, `build`, `seq`, `event`)
#   * `app_start` build/driver identity
#   * `interceptor_state` transitions
#   * `windivert_open`/`windivert_close` handle-id pairing (one close per open)
#   * `control_registration` ledger (at most one active session; superseded rows
#     resolvable)
#   * `presence`, `watchdog`, `ui_error`/`ui.word`, `gui.action`, `shutdown`
#   * A2: ZERO capture teardowns during a server/relay transition
#
# The closed vocabularies mirror client/src/app/lifecycle.rs exactly. A new
# spelling is a schema change and fails the consumer, which is the point: the
# harness must not silently accept an unversioned trace.
#
# The self-test at the bottom proves the assertions can FAIL (mutation-style):
# a clean A2 fixture must pass the A2 assertion and a fixture with an injected
# Stop must fail it. That is a harness self-test, NOT a row PASS - no
# real-driver row can pass in this round.

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

$script:P06KnownEvents = @(
    'app_start',
    'interceptor_state',
    'windivert_open',
    'windivert_close',
    'control_registration',
    'presence',
    'watchdog',
    'ui_error',
    'ui.word',
    'ui_state',
    'gui.action',
    'shutdown'
)

$script:P06BaseFields = @('ts', 'ts_ms', 'app_session', 'build', 'seq', 'event')

# Required field names per event, taken from the typed structs in
# client/src/app/lifecycle.rs. Optional fields are still keys (serde writes
# `null`); the consumer records them but only requires the key to exist.
$script:P06RequiredFields = @{
    'app_start' = @('version', 'commit', 'windivert_dll_sha256', 'windivert_sys_sha256', 'windivert_version', 'elevated', 'host_os', 'boot_id', 'prev_shutdown')
    'interceptor_state' = @('from', 'to', 'reason', 'actor', 'command_id', 'pin_id', 'pin_active', 'handle_ids', 'locked_server', 'relay', 'duration_ms', 'attempt', 'max_attempts', 'prev_event_id')
    'windivert_open' = @('id', 'kind', 'attempt', 'layer', 'priority', 'flags', 'filter', 'reason', 'actor', 'command_id', 'pin_active', 'pin_id', 'elapsed_us', 'ms_since_last_close', 'fwp_code', 'owned_handles', 'result', 'snapshot_id')
    'windivert_close' = @('id', 'kind', 'close_age_ms', 'result', 'pin_active', 'owned_handles')
    'control_registration' = @('attempt', 'outcome', 'session', 'relay', 'node', 'region', 'reason', 'prev_session', 'superseded', 'close_reason', 'generation')
    'presence' = @('from', 'to', 'cause', 'scan_ms', 'latch')
    'watchdog' = @('verdict', 'streak', 'fault', 'action', 'relay', 'server', 'intercepted', 'injected', 'from_relay')
    'ui_error' = @('op', 'source', 'kind', 'detail_key', 'source_command', 'visible_primary_state', 'reason_code', 'cleared_by', 'age_ms')
    'ui.word' = @('from', 'to', 'reason', 'boosting')
    'ui_state' = @('action_slot', 'relay', 'registration', 'error', 'boosting', 'locked_server', 'relay_rtt_ms')
    'gui.action' = @('action', 'source', 'from', 'to', 'command_id')
    'shutdown' = @('reason', 'clean', 'handle_ids_closed', 'pin_released')
}

$script:P06Enums = @{
    'interceptor_state.to' = @('idle', 'boosting', 'start_failed')
    'interceptor_state.from' = @('idle', 'boosting', 'start_failed')
    'interceptor_state.reason' = @('user_start', 'user_stop', 'presence_start', 'presence_stop', 'relay_switch', 'server_transition', 'stale_server', 'watchdog_demote', 'error_retry', 'auto_recover', 'quit', 'update', 'elevation_handoff', 'force_kill_recovery', 'unknown')
    'interceptor_state.actor' = @('user', 'auto', 'retry', 'internal', 'system')
    'windivert_open.kind' = @('pin', 'intercept', 'inject', 'shadow', 'legacy')
    'windivert_open.result' = @('ok', 'failed')
    'windivert_open.reason' = @('user_start', 'user_stop', 'presence_start', 'presence_stop', 'relay_switch', 'server_transition', 'stale_server', 'watchdog_demote', 'error_retry', 'auto_recover', 'quit', 'update', 'elevation_handoff', 'force_kill_recovery', 'unknown')
    'windivert_open.actor' = @('user', 'auto', 'retry', 'internal', 'system')
    'windivert_close.kind' = @('pin', 'intercept', 'inject', 'shadow', 'legacy')
    'windivert_close.result' = @('ok', 'failed')
    'control_registration.outcome' = @('ok', 'fail', 'timeout', 'superseded')
    'control_registration.close_reason' = @('session_replaced', 'client_quit', 'timeout', 'unknown')
    'presence.cause' = @('process_started', 'process_exited', 'name_match', 'scan_error', 'disabled', 'manual_pick')
    'watchdog.verdict' = @('healthy', 'no_reply_from_relay', 'injection_failed', 'too_few_replies')
    'watchdog.action' = @('keep', 'demote', 'switch_direct')
    'ui_error.op' = @('show', 'clear', 'replace')
    'ui_error.source' = @('capture', 'route', 'registration', 'config', 'game')
    'shutdown.reason' = @('quit', 'update', 'panic', 'elevation_handoff', 'kill', 'unknown')
    'app_start.prev_shutdown' = @('clean', 'unclean', 'unknown')
    'gui.action.action' = @('optimize', 'stop', 'restart', 'quit', 'update_install', 'elevation_handoff')
    'gui.action.source' = @('button', 'tray', 'shortcut', 'auto', 'system')
    'gui.action.from' = @('capturing', 'idle', 'fault_latched', 'shutdown')
    'gui.action.to' = @('capturing', 'idle', 'fault_latched', 'shutdown')
}

# Kinds that own a WFP filter/callout: closing one of these is a capture
# teardown. A `pin` handle keeps the driver referenced but owns no WFP object
# (P0-5 truth rule), so a pin close is not a capture teardown.
$script:P06CaptureHandleKinds = @('intercept', 'inject', 'shadow', 'legacy')

function ConvertTo-P06FlatRecord {
    param($Object)
    $flat = [ordered]@{}
    if ($null -eq $Object) { return $flat }
    foreach ($property in $Object.PSObject.Properties) {
        $flat[$property.Name] = $property.Value
    }
    return $flat
}

function Read-P06Lifecycle {
    param([string]$Path)
    $result = [ordered]@{
        path = $Path
        exists = $false
        line_count = 0
        empty_lines = 0
        parse_errors = @()
        records = @()
        event_counts = [ordered]@{}
    }
    if ([string]::IsNullOrWhiteSpace($Path) -or -not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return $result
    }
    $result.exists = $true
    try {
        $lines = [System.IO.File]::ReadAllLines($Path)
    } catch {
        $result.parse_errors += [ordered]@{ line = 0; error = ('could not read file: ' + $_.Exception.Message) }
        return $result
    }
    $lineNumber = 0
    foreach ($line in $lines) {
        $lineNumber++
        if ([string]::IsNullOrWhiteSpace($line)) {
            $result.empty_lines++
            continue
        }
        $result.line_count++
        $parsed = $null
        try {
            $parsed = $line | ConvertFrom-Json
        } catch {
            $result.parse_errors += [ordered]@{ line = $lineNumber; error = $_.Exception.Message; raw = $line }
            continue
        }
        $fields = ConvertTo-P06FlatRecord -Object $parsed
        $event = Get-P06Field -Object $fields -Name 'event' -Default $null
        $record = [ordered]@{
            line = $lineNumber
            seq = (Get-P06Field -Object $fields -Name 'seq' -Default $null)
            ts = (Get-P06Field -Object $fields -Name 'ts' -Default $null)
            ts_ms = (Get-P06Field -Object $fields -Name 'ts_ms' -Default $null)
            app_session = (Get-P06Field -Object $fields -Name 'app_session' -Default $null)
            build = (Get-P06Field -Object $fields -Name 'build' -Default $null)
            event = $event
            fields = $fields
            raw = $line
        }
        $result.records += $record
        $key = [string]$event
        if (-not $result.event_counts.Contains($key)) { $result.event_counts[$key] = 0 }
        $result.event_counts[$key] = [int]$result.event_counts[$key] + 1
    }
    return $result
}

function Test-P06LifecycleSchema {
    param($Parsed)
    $errors = @()
    $warnings = @()
    if (-not (Get-P06Field -Object $Parsed -Name 'exists' -Default $false)) {
        return [ordered]@{
            ok = $false
            errors = @('lifecycle.jsonl not found at ' + (Get-P06Field -Object $Parsed -Name 'path' -Default '<none>'))
            warnings = @()
            record_count = 0
            checked_events = @()
        }
    }
    foreach ($parseError in @(Get-P06Field -Object $Parsed -Name 'parse_errors' -Default @())) {
        $errors += ('line {0}: not valid JSON: {1}' -f (Get-P06Field -Object $parseError -Name 'line' -Default '?'), (Get-P06Field -Object $parseError -Name 'error' -Default '?'))
    }
    $records = @(Get-P06Field -Object $Parsed -Name 'records' -Default @())
    if (@($records).Count -eq 0) {
        $errors += 'lifecycle.jsonl contains zero records'
    }
    $checkedEvents = [ordered]@{}
    foreach ($record in $records) {
        $line = Get-P06Field -Object $record -Name 'line' -Default '?'
        $event = [string](Get-P06Field -Object $record -Name 'event' -Default '')
        $fields = Get-P06Field -Object $record -Name 'fields' -Default $null
        foreach ($baseField in $script:P06BaseFields) {
            $value = Get-P06Field -Object $fields -Name $baseField -Default $null
            if ($null -eq $value) {
                $errors += ('line {0}: base field `{1}` missing or null' -f $line, $baseField)
            }
        }
        if ($event.Length -eq 0) {
            $errors += ('line {0}: event name missing' -f $line)
            continue
        }
        if ($script:P06KnownEvents -notcontains $event) {
            $errors += ('line {0}: unknown event `{1}` (schema change requires a harness update)' -f $line, $event)
            continue
        }
        if (-not $checkedEvents.Contains($event)) { $checkedEvents[$event] = 0 }
        $checkedEvents[$event] = [int]$checkedEvents[$event] + 1
        foreach ($required in $script:P06RequiredFields[$event]) {
            if (-not $fields.Contains($required)) {
                $errors += ('line {0}: event `{1}` missing schema field `{2}`' -f $line, $event, $required)
            }
        }
        foreach ($enumName in $script:P06Enums.Keys) {
            # Event names contain dots ('ui.word', 'gui.action'), so split at the
            # LAST dot: everything before it is the event, after it the field.
            $splitAt = $enumName.LastIndexOf('.')
            if ($splitAt -lt 1) { continue }
            $enumEvent = $enumName.Substring(0, $splitAt)
            $enumField = $enumName.Substring($splitAt + 1)
            if ($enumEvent -ne $event) { continue }
            $value = Get-P06Field -Object $fields -Name $enumField -Default $null
            if ($null -eq $value) { continue }
            if ($script:P06Enums[$enumName] -notcontains ([string]$value)) {
                $errors += ('line {0}: event `{1}` field `{2}` value `{3}` is outside the closed vocabulary' -f $line, $event, $enumField, $value)
            }
        }
        if ($event -eq 'app_start') {
            $version = Get-P06Field -Object $fields -Name 'version' -Default $null
            $commit = Get-P06Field -Object $fields -Name 'commit' -Default $null
            if ([string]::IsNullOrWhiteSpace([string]$version)) { $errors += ('line {0}: app_start version is empty' -f $line) }
            if ([string]::IsNullOrWhiteSpace([string]$commit)) { $errors += ('line {0}: app_start commit is empty (A12 cannot identify the build)' -f $line) }
            if ([string]$commit -eq 'unknown') { $warnings += ('line {0}: app_start commit is `unknown` (build had no git stamp)' -f $line) }
        }
        if ($event -eq 'gui.action') {
            $commandId = Get-P06Field -Object $fields -Name 'command_id' -Default $null
            if ([string]::IsNullOrWhiteSpace([string]$commandId)) {
                $errors += ('line {0}: gui.action command_id is empty (action cannot be attributed)' -f $line)
            }
        }
    }
    return [ordered]@{
        ok = (@($errors).Count -eq 0)
        errors = @($errors)
        warnings = @($warnings)
        record_count = @($records).Count
        checked_events = $checkedEvents
    }
}

function Test-P06HandleLedger {
    param($Parsed)
    $opens = [ordered]@{}
    $closes = [ordered]@{}
    $errors = @()
    $warnings = @()
    $records = @(Get-P06Field -Object $Parsed -Name 'records' -Default @())
    foreach ($record in $records) {
        $event = [string](Get-P06Field -Object $record -Name 'event' -Default '')
        if ($event -ne 'windivert_open' -and $event -ne 'windivert_close') { continue }
        $fields = Get-P06Field -Object $record -Name 'fields' -Default $null
        $id = [string](Get-P06Field -Object $fields -Name 'id' -Default $null)
        $line = Get-P06Field -Object $record -Name 'line' -Default '?'
        $seq = Get-P06Field -Object $record -Name 'seq' -Default $null
        if ([string]::IsNullOrWhiteSpace($id)) {
            $errors += ('line {0}: {1} has no handle id' -f $line, $event)
            continue
        }
        if ($event -eq 'windivert_open') {
            if (-not $opens.Contains($id)) { $opens[$id] = @() }
            $opens[$id] += [ordered]@{
                line = $line
                seq = $seq
                kind = [string](Get-P06Field -Object $fields -Name 'kind' -Default '')
                result = [string](Get-P06Field -Object $fields -Name 'result' -Default '')
                pin_active = (Get-P06Field -Object $fields -Name 'pin_active' -Default $null)
                pin_id = (Get-P06Field -Object $fields -Name 'pin_id' -Default $null)
                reason = [string](Get-P06Field -Object $fields -Name 'reason' -Default '')
            }
        } else {
            if (-not $closes.Contains($id)) { $closes[$id] = @() }
            $closes[$id] += [ordered]@{
                line = $line
                seq = $seq
                kind = [string](Get-P06Field -Object $fields -Name 'kind' -Default '')
                result = [string](Get-P06Field -Object $fields -Name 'result' -Default '')
                pin_active = (Get-P06Field -Object $fields -Name 'pin_active' -Default $null)
            }
        }
    }
    $openIds = @($opens.Keys)
    $closeIds = @($closes.Keys)
    $paired = 0
    $active = @()
    foreach ($id in $openIds) {
        $attempts = @($opens[$id])
        $successes = @($attempts | Where-Object { $_.result -eq 'ok' })
        $closeList = @()
        if ($closes.Contains($id)) { $closeList = @($closes[$id]) }
        if (@($successes).Count -eq 0) {
            # Every attempt failed: the driver never returned a handle, so no
            # close is expected. A close without a successful open is an error.
            if (@($closeList).Count -gt 0) {
                $errors += ('handle id {0}: closed but never opened successfully' -f $id)
            }
            continue
        }
        if (@($successes).Count -gt 1) {
            $errors += ('handle id {0}: opened successfully {1} times (an id must identify one handle lifetime)' -f $id, @($successes).Count)
        }
        if (@($closeList).Count -eq 0) {
            $active += $id
            continue
        }
        if (@($closeList).Count -gt 1) {
            $errors += ('handle id {0}: {1} close records for one open (one close per open)' -f $id, @($closeList).Count)
        }
        foreach ($close in $closeList) {
            $openSeq = @($successes)[0].seq
            if ($null -ne $openSeq -and $null -ne $close.seq -and [int64]$close.seq -le [int64]$openSeq) {
                $errors += ('handle id {0}: close seq {1} precedes open seq {2}' -f $id, $close.seq, $openSeq)
            }
        }
        $paired++
    }
    foreach ($id in $closeIds) {
        if (-not $opens.Contains($id)) {
            $errors += ('handle id {0}: close record without any open record' -f $id)
        }
    }
    $pinOpenIds = @()
    $pinCloseIds = @()
    foreach ($id in $openIds) {
        $attempts = @($opens[$id])
        $ok = @($attempts | Where-Object { $_.result -eq 'ok' })
        if (@($ok).Count -gt 0 -and @($ok)[0].kind -eq 'pin') { $pinOpenIds += $id }
    }
    foreach ($id in $closeIds) {
        $closeList = @($closes[$id])
        if (@($closeList).Count -gt 0 -and @($closeList)[0].kind -eq 'pin') { $pinCloseIds += $id }
    }
    return [ordered]@{
        ok = (@($errors).Count -eq 0)
        errors = @($errors)
        warnings = @($warnings)
        open_ids = @($openIds)
        close_ids = @($closeIds)
        paired_handle_count = $paired
        active_open_ids = @($active)
        pin = [ordered]@{
            open_ids = @($pinOpenIds)
            close_ids = @($pinCloseIds)
            active = (@($pinOpenIds).Count - @($pinCloseIds).Count)
            one_pin_at_a_time = ((@($pinOpenIds).Count - @($pinCloseIds).Count) -le 1)
        }
        open_count = @($records | Where-Object { $_.event -eq 'windivert_open' }).Count
        close_count = @($records | Where-Object { $_.event -eq 'windivert_close' }).Count
        failed_open_count = @($records | Where-Object { $_.event -eq 'windivert_open' -and (Get-P06Field -Object $_.fields -Name 'result' -Default '') -eq 'failed' }).Count
    }
}

function Test-P06RegistrationLedger {
    param($Parsed)
    $sessions = [ordered]@{}
    $errors = @()
    $warnings = @()
    $activeSessions = @()
    $maxActive = 0
    $registrationCount = 0
    $records = @(Get-P06Field -Object $Parsed -Name 'records' -Default @())
    foreach ($record in $records) {
        if ($record.event -ne 'control_registration') { continue }
        $registrationCount++
        $fields = $record.fields
        $line = $record.line
        $session = Get-P06Field -Object $fields -Name 'session' -Default $null
        $outcome = [string](Get-P06Field -Object $fields -Name 'outcome' -Default '')
        $reason = [string](Get-P06Field -Object $fields -Name 'reason' -Default '')
        $closeReason = Get-P06Field -Object $fields -Name 'close_reason' -Default $null
        $superseded = [bool](Get-P06Field -Object $fields -Name 'superseded' -Default $false)
        $prevSession = Get-P06Field -Object $fields -Name 'prev_session' -Default $null
        $generation = Get-P06Field -Object $fields -Name 'generation' -Default $null
        if ($null -ne $prevSession) {
            if (-not $sessions.Contains([string]$prevSession)) {
                $errors += ('line {0}: prev_session {1} has no earlier ledger row' -f $line, $prevSession)
            }
        }
        if ($superseded) {
            if ($null -eq $session -and $null -eq $prevSession) {
                $errors += ('line {0}: superseded row names no session and no prev_session; it cannot be resolved' -f $line)
            } elseif ($null -ne $session -and -not $sessions.Contains([string]$session)) {
                # A discarded candidate generation can name a session token that
                # was never published, so it has no earlier active row. It is
                # resolved by the row itself (retired before first publish).
                $warnings += ('line {0}: superseded candidate session {1} was never active (discarded generation)' -f $line, $session)
            }
        }
        if ($reason -eq 'close') {
            if ($null -eq $session) {
                $errors += ('line {0}: registration close record has no session' -f $line)
                continue
            }
            $key = [string]$session
            if (-not $sessions.Contains($key)) {
                $errors += ('line {0}: close for registration session {1} never opened' -f $line, $session)
                continue
            }
            $entry = $sessions[$key]
            if ($entry.closed) {
                $errors += ('line {0}: registration session {1} closed twice' -f $line, $session)
            }
            $entry.closed = $true
            if ($null -ne $closeReason) { $entry.close_reason = $closeReason }
            if ($null -ne $generation) { $entry.generation = $generation }
            continue
        }
        if ($outcome -eq 'ok' -and $null -ne $session) {
            $key = [string]$session
            if ($sessions.Contains($key)) {
                $entry = $sessions[$key]
                if (-not $entry.closed -and -not $entry.superseded) {
                    $errors += ('line {0}: registration session {1} re-published while already active (duplicate active row)' -f $line, $session)
                }
                $entry.closed = $false
                $entry.superseded = $false
                $entry.superseded_by = $null
                if ($null -ne $generation) { $entry.generation = $generation }
            } else {
                # A newer successful registration replaces the live supervisor
                # without an explicit close row (engine.rs `reconcile_registration`
                # / `publish_registration`): the newer session supersedes the old
                # one. Resolve that here so the ledger keeps exactly one active
                # session, and keep the superseded row resolvable.
                foreach ($otherKey in @($sessions.Keys)) {
                    $other = $sessions[$otherKey]
                    if ($other.closed -or $other.superseded) { continue }
                    if ([string]$other.session -eq $key) { continue }
                    $other.superseded = $true
                    $other.superseded_by = $session
                    $warnings += ('line {0}: session {1} superseded by newer session {2} (no explicit close row)' -f $line, $other.session, $session)
                }
                $sessions[$key] = [ordered]@{
                    session = $session
                    first_line = $line
                    last_line = $line
                    closed = $false
                    superseded = $false
                    superseded_by = $null
                    close_reason = $null
                    generation = $generation
                }
            }
            $sessions[$key].last_line = $line
        }
        if ($superseded -and $null -ne $session -and $sessions.Contains([string]$session)) {
            $sessions[[string]$session].superseded = $true
            if ($null -ne $prevSession) { $sessions[[string]$session].superseded_by = $prevSession }
        }
        # Recompute active count after every row.
        $active = @()
        foreach ($key in $sessions.Keys) {
            $entry = $sessions[$key]
            if (-not $entry.closed -and -not $entry.superseded) { $active += $entry.session }
        }
        $activeSessions = $active
        if (@($active).Count -gt $maxActive) { $maxActive = @($active).Count }
        if (@($active).Count -gt 1) {
            $errors += ('line {0}: {1} registration sessions active at once' -f $line, @($active).Count)
        }
    }
    return [ordered]@{
        ok = (@($errors).Count -eq 0)
        errors = @($errors)
        warnings = @($warnings)
        registration_count = $registrationCount
        session_count = @($sessions.Keys).Count
        sessions = $sessions
        active_sessions = @($activeSessions)
        max_active = $maxActive
        superseded_rows = @($records | Where-Object { $_.event -eq 'control_registration' -and (Get-P06Field -Object $_.fields -Name 'superseded' -Default $false) }).Count
    }
}

function Get-P06TransitionMarkers {
    param($Parsed)
    $markers = @()
    $records = @(Get-P06Field -Object $Parsed -Name 'records' -Default @())
    foreach ($record in $records) {
        if ($record.event -ne 'control_registration') { continue }
        $fields = $record.fields
        $outcome = [string](Get-P06Field -Object $fields -Name 'outcome' -Default '')
        $reason = [string](Get-P06Field -Object $fields -Name 'reason' -Default '')
        $session = Get-P06Field -Object $fields -Name 'session' -Default $null
        $closeReason = Get-P06Field -Object $fields -Name 'close_reason' -Default $null
        if ($outcome -ne 'ok' -or $null -eq $session -or $null -ne $closeReason) { continue }
        if (@('destination_report', 'relay_switch', 'server_transition', 'stale_server') -contains $reason) {
            $markers += [ordered]@{
                line = $record.line
                seq = $record.seq
                reason = $reason
                session = $session
                source = 'control_registration'
            }
        }
    }
    return @($markers)
}

function Test-P06NoCaptureTeardown {
    param(
        $Parsed,
        [int]$FromLine = 1,
        [int]$ToLine = 0
    )
    $records = @(Get-P06Field -Object $Parsed -Name 'records' -Default @())
    $violations = @()
    foreach ($record in $records) {
        $line = [int]$record.line
        if ($line -lt $FromLine) { continue }
        if ($ToLine -gt 0 -and $line -gt $ToLine) { continue }
        $fields = $record.fields
        if ($record.event -eq 'windivert_close') {
            $kind = [string](Get-P06Field -Object $fields -Name 'kind' -Default '')
            if ($script:P06CaptureHandleKinds -contains $kind) {
                $violations += [ordered]@{
                    line = $line
                    event = 'windivert_close'
                    kind = $kind
                    id = (Get-P06Field -Object $fields -Name 'id' -Default $null)
                    detail = 'capture handle closed during a transition window'
                }
            }
        }
        if ($record.event -eq 'interceptor_state') {
            $to = [string](Get-P06Field -Object $fields -Name 'to' -Default '')
            if (@('idle', 'start_failed') -contains $to) {
                $violations += [ordered]@{
                    line = $line
                    event = 'interceptor_state'
                    to = $to
                    reason = [string](Get-P06Field -Object $fields -Name 'reason' -Default '')
                    actor = [string](Get-P06Field -Object $fields -Name 'actor' -Default '')
                    command_id = [string](Get-P06Field -Object $fields -Name 'command_id' -Default '')
                    detail = 'capture plane left boosting during a transition window'
                }
            }
        }
        if ($record.event -eq 'windivert_open') {
            $reason = [string](Get-P06Field -Object $fields -Name 'reason' -Default '')
            $result = [string](Get-P06Field -Object $fields -Name 'result' -Default '')
            if (@('server_transition', 'stale_server', 'relay_switch') -contains $reason) {
                $violations += [ordered]@{
                    line = $line
                    event = 'windivert_open'
                    reason = $reason
                    id = (Get-P06Field -Object $fields -Name 'id' -Default $null)
                    result = $result
                    detail = 'capture reopened for a transition (a transition must not restart capture)'
                }
            }
        }
    }
    return [ordered]@{
        ok = (@($violations).Count -eq 0)
        from_line = $FromLine
        to_line = $ToLine
        teardown_count = @($violations).Count
        violations = @($violations)
    }
}

function Test-P06A2Transition {
    param(
        $Parsed,
        [int]$WindowBefore = 2,
        [int]$WindowAfter = 4,
        [int[]]$ExplicitLines = @()
    )
    $markers = @()
    if (@($ExplicitLines).Count -gt 0) {
        foreach ($line in $ExplicitLines) {
            $markers += [ordered]@{ line = $line; reason = 'explicit'; session = $null; source = 'caller' }
        }
    } else {
        $markers = @(Get-P06TransitionMarkers -Parsed $Parsed)
    }
    if (@($markers).Count -eq 0) {
        return [ordered]@{
            status = 'U'
            ok = $false
            reason = 'no server/relay transition marker in the lifecycle trace; A2 was not exercised'
            markers = @()
            windows = @()
            teardown_count = $null
            violations = @()
        }
    }
    $windows = @()
    $allViolations = @()
    $teardown = 0
    foreach ($marker in $markers) {
        $from = [Math]::Max(1, [int]$marker.line - $WindowBefore)
        $to = [int]$marker.line + $WindowAfter
        $check = Test-P06NoCaptureTeardown -Parsed $Parsed -FromLine $from -ToLine $to
        $teardown += [int]$check.teardown_count
        foreach ($violation in @($check.violations)) { $allViolations += $violation }
        $windows += [ordered]@{
            marker_line = $marker.line
            reason = $marker.reason
            session = $marker.session
            from_line = $from
            to_line = $to
            teardown_count = $check.teardown_count
            ok = $check.ok
        }
    }
    return [ordered]@{
        status = $(if ($teardown -eq 0) { 'PASS_ASSERTION' } else { 'FAIL_ASSERTION' })
        ok = ($teardown -eq 0)
        reason = $(if ($teardown -eq 0) { 'zero capture teardowns in every transition window' } else { 'capture teardown(s) occurred inside a transition window' })
        window_before = $WindowBefore
        window_after = $WindowAfter
        marker_count = @($markers).Count
        markers = @($markers)
        windows = @($windows)
        teardown_count = $teardown
        violations = @($allViolations)
    }
}

function Test-P06UiAndActions {
    param($Parsed)
    $errors = @()
    $warnings = @()
    $records = @(Get-P06Field -Object $Parsed -Name 'records' -Default @())
    $uiErrors = @()
    $uiWords = @()
    $uiStates = @()
    $guiActions = @()
    foreach ($record in $records) {
        switch ([string]$record.event) {
            'ui_error' { $uiErrors += $record }
            'ui.word' { $uiWords += $record }
            'ui_state' { $uiStates += $record }
            'gui.action' { $guiActions += $record }
        }
    }
    # T15: exactly one gui.action per capture-crossing invocation.
    $byCommand = [ordered]@{}
    foreach ($action in $guiActions) {
        $commandId = [string](Get-P06Field -Object $action.fields -Name 'command_id' -Default '')
        if (-not $byCommand.Contains($commandId)) { $byCommand[$commandId] = @() }
        $byCommand[$commandId] += $action
    }
    foreach ($commandId in $byCommand.Keys) {
        $count = @($byCommand[$commandId]).Count
        if ($count -gt 1) {
            $errors += ('gui.action command_id `{0}` recorded {1} times (exactly one action per invocation)' -f $commandId, $count)
        }
    }
    # A ui_state semantic dump is emitted only when an error card appears.
    foreach ($state in $uiStates) {
        $precededByShow = $false
        foreach ($uiError in $uiErrors) {
            if ([int]$uiError.seq -lt [int]$state.seq -and [string](Get-P06Field -Object $uiError.fields -Name 'op' -Default '') -eq 'show') {
                $precededByShow = $true
            }
        }
        if (-not $precededByShow) {
            $warnings += ('line {0}: ui_state without a preceding ui_error show' -f $state.line)
        }
    }
    # A clear must name what cleared it.
    foreach ($uiError in $uiErrors) {
        $op = [string](Get-P06Field -Object $uiError.fields -Name 'op' -Default '')
        $clearedBy = Get-P06Field -Object $uiError.fields -Name 'cleared_by' -Default $null
        if ($op -eq 'clear' -and $null -eq $clearedBy) {
            $warnings += ('line {0}: ui_error clear has no cleared_by (error clearing cannot be attributed)' -f $uiError.line)
        }
    }
    return [ordered]@{
        ok = (@($errors).Count -eq 0)
        errors = @($errors)
        warnings = @($warnings)
        ui_error_count = @($uiErrors).Count
        ui_word_count = @($uiWords).Count
        ui_state_count = @($uiStates).Count
        gui_action_count = @($guiActions).Count
        gui_action_command_ids = @($byCommand.Keys)
        ui_error_ops = @($uiErrors | ForEach-Object { Get-P06Field -Object $_.fields -Name 'op' -Default $null })
    }
}

function Test-P06PresenceAndWatchdog {
    param($Parsed)
    $records = @(Get-P06Field -Object $Parsed -Name 'records' -Default @())
    $presence = @($records | Where-Object { $_.event -eq 'presence' })
    $watchdog = @($records | Where-Object { $_.event -eq 'watchdog' })
    $scanErrors = @($presence | Where-Object { [string](Get-P06Field -Object $_.fields -Name 'cause' -Default '') -eq 'scan_error' })
    $healthyWindows = @($watchdog | Where-Object { [string](Get-P06Field -Object $_.fields -Name 'verdict' -Default '') -eq 'healthy' })
    $twoWayWindows = @($watchdog | Where-Object {
        $injected = Get-P06Field -Object $_.fields -Name 'injected' -Default 0
        $fromRelay = Get-P06Field -Object $_.fields -Name 'from_relay' -Default 0
        ($null -ne $injected -and [uint64]$injected -gt 0) -and ($null -ne $fromRelay -and [uint64]$fromRelay -gt 0)
    })
    return [ordered]@{
        presence_count = @($presence).Count
        presence_scan_error_count = @($scanErrors).Count
        watchdog_count = @($watchdog).Count
        healthy_window_count = @($healthyWindows).Count
        two_way_window_count = @($twoWayWindows).Count
        two_way_share_available = (@($watchdog).Count -gt 0)
    }
}

function Test-P06Shutdown {
    param($Parsed, $HandleLedger)
    $records = @(Get-P06Field -Object $Parsed -Name 'records' -Default @())
    $shutdowns = @($records | Where-Object { $_.event -eq 'shutdown' })
    $errors = @()
    $warnings = @()
    if (@($shutdowns).Count -eq 0) {
        $warnings += 'no shutdown record in the lifecycle trace (trace truncated or unclean exit)'
    }
    if (@($shutdowns).Count -gt 1) {
        $errors += ('{0} shutdown records for one app_session (exactly one expected)' -f @($shutdowns).Count)
    }
    foreach ($shutdown in $shutdowns) {
        $clean = Get-P06Field -Object $shutdown.fields -Name 'clean' -Default $null
        $pinReleased = Get-P06Field -Object $shutdown.fields -Name 'pin_released' -Default $null
        $activeOpenIds = @(Get-P06Field -Object $HandleLedger -Name 'active_open_ids' -Default @())
        if ($clean -eq $true -and @($activeOpenIds).Count -gt 0) {
            $warnings += ('clean shutdown with {0} handle id(s) still open in the ledger' -f @($activeOpenIds).Count)
        }
        if ($pinReleased -eq $true) {
            $pin = Get-P06Field -Object $HandleLedger -Name 'pin' -Default $null
            $pinActive = Get-P06Field -Object $pin -Name 'active' -Default $null
            if ($null -ne $pinActive -and [int]$pinActive -ne 0) {
                $errors += ('shutdown pin_released=true but {0} pin handle(s) remain open' -f $pinActive)
            }
        }
    }
    return [ordered]@{
        ok = (@($errors).Count -eq 0)
        errors = @($errors)
        warnings = @($warnings)
        shutdown_count = @($shutdowns).Count
        reasons = @($shutdowns | ForEach-Object { Get-P06Field -Object $_.fields -Name 'reason' -Default $null })
        clean_flags = @($shutdowns | ForEach-Object { Get-P06Field -Object $_.fields -Name 'clean' -Default $null })
    }
}

function Get-P06AppStartIdentity {
    param($Parsed)
    $records = @(Get-P06Field -Object $Parsed -Name 'records' -Default @())
    $appStart = @($records | Where-Object { $_.event -eq 'app_start' })
    if (@($appStart).Count -eq 0) {
        return [ordered]@{ present = $false; count = 0; identity = $null }
    }
    $first = @($appStart)[0]
    return [ordered]@{
        present = $true
        count = @($appStart).Count
        line = $first.line
        identity = [ordered]@{
            version = (Get-P06Field -Object $first.fields -Name 'version' -Default $null)
            commit = (Get-P06Field -Object $first.fields -Name 'commit' -Default $null)
            windivert_dll_sha256 = (Get-P06Field -Object $first.fields -Name 'windivert_dll_sha256' -Default $null)
            windivert_sys_sha256 = (Get-P06Field -Object $first.fields -Name 'windivert_sys_sha256' -Default $null)
            windivert_version = (Get-P06Field -Object $first.fields -Name 'windivert_version' -Default $null)
            elevated = (Get-P06Field -Object $first.fields -Name 'elevated' -Default $null)
            host_os = (Get-P06Field -Object $first.fields -Name 'host_os' -Default $null)
            boot_id = (Get-P06Field -Object $first.fields -Name 'boot_id' -Default $null)
            prev_shutdown = (Get-P06Field -Object $first.fields -Name 'prev_shutdown' -Default $null)
            build = $first.build
        }
    }
}

function Test-P06IdentityAgainstRun {
    param($Parsed, $IdentityGate)
    $appStart = Get-P06AppStartIdentity -Parsed $Parsed
    if (-not $appStart.present) {
        return [ordered]@{
            status = 'U'
            ok = $false
            reason = 'no app_start record; runtime identity cannot be checked against the artifact identity block'
            app_start = $appStart
        }
    }
    $expected = Get-P06Field -Object $IdentityGate -Name 'expected' -Default $null
    $appIdentity = Get-P06Field -Object $appStart -Name 'identity' -Default $null
    $errors = @()
    $expectedVersion = [string](Get-P06Field -Object $expected -Name 'version' -Default '')
    $expectedCommit12 = [string](Get-P06Field -Object $expected -Name 'short_commit' -Default '')
    $expectedDll = [string](Get-P06Field -Object $expected -Name 'windivert_dll_sha256' -Default '')
    $expectedSys = [string](Get-P06Field -Object $expected -Name 'windivert_sys_sha256' -Default '')
    $runVersion = [string](Get-P06Field -Object $appIdentity -Name 'version' -Default '')
    $runCommit = [string](Get-P06Field -Object $appIdentity -Name 'commit' -Default '')
    $runDll = [string](Get-P06Field -Object $appIdentity -Name 'windivert_dll_sha256' -Default '')
    $runSys = [string](Get-P06Field -Object $appIdentity -Name 'windivert_sys_sha256' -Default '')
    if ($expectedVersion.Length -gt 0 -and $runVersion -ne $expectedVersion) {
        $errors += ('app_start version {0} != expected {1}' -f $runVersion, $expectedVersion)
    }
    if ([string]::IsNullOrWhiteSpace($runCommit) -or $runCommit -eq 'unknown') {
        $errors += 'app_start commit is absent or `unknown`; the trace is not attributable'
    } elseif ($expectedCommit12.Length -gt 0 -and -not $runCommit.StartsWith($expectedCommit12)) {
        $errors += ('app_start commit {0} does not match expected release commit {1}' -f $runCommit, $expectedCommit12)
    }
    if ($expectedDll.Length -gt 0 -and $runDll -ne $expectedDll) {
        $errors += 'app_start windivert_dll_sha256 != expected release dll hash'
    }
    if ($expectedSys.Length -gt 0 -and $runSys -ne $expectedSys) {
        $errors += 'app_start windivert_sys_sha256 != expected release sys hash'
    }
    return [ordered]@{
        status = $(if (@($errors).Count -eq 0) { 'MATCH' } else { 'MISMATCH' })
        ok = (@($errors).Count -eq 0)
        errors = @($errors)
        app_start = $appStart.identity
        expected = $expected
    }
}

function Invoke-P06LifecycleAudit {
    param(
        [string]$Path,
        $IdentityGate = $null
    )
    $parsed = Read-P06Lifecycle -Path $Path
    $schema = Test-P06LifecycleSchema -Parsed $parsed
    $handleLedger = Test-P06HandleLedger -Parsed $parsed
    $registrationLedger = Test-P06RegistrationLedger -Parsed $parsed
    $a2 = Test-P06A2Transition -Parsed $parsed
    $ui = Test-P06UiAndActions -Parsed $parsed
    $presence = Test-P06PresenceAndWatchdog -Parsed $parsed
    $shutdown = Test-P06Shutdown -Parsed $parsed -HandleLedger $handleLedger
    $appStart = Get-P06AppStartIdentity -Parsed $parsed
    $identityCheck = $null
    if ($null -ne $IdentityGate) {
        $identityCheck = Test-P06IdentityAgainstRun -Parsed $parsed -IdentityGate $IdentityGate
    }
    $errors = @()
    $sections = @(
        @{ name = 'schema'; value = $schema },
        @{ name = 'handle_ledger'; value = $handleLedger },
        @{ name = 'registration_ledger'; value = $registrationLedger },
        @{ name = 'ui_actions'; value = $ui },
        @{ name = 'shutdown'; value = $shutdown }
    )
    foreach ($section in $sections) {
        foreach ($error in @($section.value.errors)) {
            $errors += ('{0}: {1}' -f $section.name, $error)
        }
    }
    return [ordered]@{
        schema = 'p0-6-lifecycle-audit/v1'
        path = $Path
        exists = $parsed.exists
        line_count = $parsed.line_count
        record_count = @($parsed.records).Count
        event_counts = $parsed.event_counts
        parse_errors = @($parsed.parse_errors)
        schema_validation = $schema
        handle_ledger = $handleLedger
        registration_ledger = $registrationLedger
        a2_transition = $a2
        ui_actions = $ui
        presence_watchdog = $presence
        shutdown = $shutdown
        app_start = $appStart
        identity_check = $identityCheck
        ok = (@($errors).Count -eq 0)
        errors = @($errors)
    }
}

# -----------------------------------------------------------------------------
#  Harness self-test: prove the assertions can FAIL, on synthetic landed-schema
#  fixtures. This is a self-test of the consumer, never a row PASS.
# -----------------------------------------------------------------------------

function New-P06FixtureRecord {
    param(
        [string]$Event,
        [long]$Seq,
        [long]$TsMs,
        [hashtable]$Fields = @{},
        [long]$AppSession = 1001,
        [string]$Build = '1.12.5@6e30a65fe668'
    )
    $base = [ordered]@{
        ts = ([DateTimeOffset]::FromUnixTimeMilliseconds($TsMs).ToString('o'))
        ts_ms = $TsMs
        app_session = $AppSession
        build = $Build
        seq = $Seq
        event = $Event
    }
    foreach ($key in $Fields.Keys) { $base[$key] = $Fields[$key] }
    return $base
}

function New-P06A2Fixture {
    param(
        [string]$Path,
        [switch]$InjectTeardown
    )
    if (Test-Path -LiteralPath $Path) { Remove-Item -LiteralPath $Path -Force }
    $seed = [long]1763913600000
    $base = @{
        app_start = @{ version = '1.12.5'; commit = '6e30a65fe668'; windivert_dll_sha256 = 'C1E060EE19444A259B2162F8AF0F3FE8C4428A1C6F694DCE20DE194AC8D7D9A2'; windivert_sys_sha256 = '8DA085332782708D8767BCACE5327A6EC7283C17CFB85E40B03CD2323A90DDC2'; windivert_version = $null; elevated = $true; host_os = 'Windows test fixture'; boot_id = 'fixtureboot0001'; prev_shutdown = 'clean' }
        stateBoost = @{ from = 'idle'; to = 'boosting'; reason = 'user_start'; actor = 'user'; command_id = 'StartBoost'; pin_id = $null; pin_active = $false; handle_ids = ''; locked_server = $null; relay = $null; duration_ms = 0; attempt = $null; max_attempts = $null; prev_event_id = $null }
        pinOpen = @{ id = 1; kind = 'pin'; attempt = 1; layer = 'test'; priority = 0; flags = '0x0000'; filter = 'true'; reason = 'user_start'; actor = 'user'; command_id = 'StartBoost'; pin_active = $false; pin_id = $null; elapsed_us = 900; ms_since_last_close = $null; fwp_code = $null; owned_handles = 'pin:1'; result = 'ok'; snapshot_id = $null }
        interceptOpen = @{ id = 2; kind = 'intercept'; attempt = 1; layer = 'test'; priority = 0; flags = '0x0000'; filter = 'udp'; reason = 'user_start'; actor = 'user'; command_id = 'StartBoost'; pin_active = $true; pin_id = 1; elapsed_us = 1200; ms_since_last_close = $null; fwp_code = $null; owned_handles = 'pin:1,intercept:2'; result = 'ok'; snapshot_id = $null }
        regConnect = @{ attempt = 1; outcome = 'ok'; session = 11; relay = 'relay-a:443'; node = $null; region = $null; reason = 'connect'; prev_session = $null; superseded = $false; close_reason = $null; generation = 1 }
        regTransition = @{ attempt = 1; outcome = 'ok'; session = 12; relay = 'relay-b:443'; node = $null; region = $null; reason = 'destination_report'; prev_session = $null; superseded = $false; close_reason = $null; generation = 2 }
        regDiscarded = @{ attempt = 1; outcome = 'superseded'; session = 99; relay = 'relay-c:443'; node = $null; region = $null; reason = 'connect'; prev_session = $null; superseded = $true; close_reason = $null; generation = 2 }
        watchdogHealthy = @{ verdict = 'healthy'; streak = 0; fault = $null; action = 'keep'; relay = 'relay-b:443'; server = '203.0.113.10:27015'; intercepted = 120; injected = 118; from_relay = 44 }
        watchdogStarved = @{ verdict = 'no_reply_from_relay'; streak = 1; fault = 'NoReplyFromRelay'; action = 'keep'; relay = 'relay-b:443'; server = '203.0.113.10:27015'; intercepted = 120; injected = 118; from_relay = 0 }
        teardownState = @{ from = 'boosting'; to = 'idle'; reason = 'server_transition'; actor = 'auto'; command_id = 'TransitionStop'; pin_id = 1; pin_active = $true; handle_ids = 'intercept:2'; locked_server = $null; relay = $null; duration_ms = 4000; attempt = $null; max_attempts = $null; prev_event_id = $null }
        closeIntercept = @{ id = 2; kind = 'intercept'; close_age_ms = 5000; result = 'ok'; pin_active = $true; owned_handles = 'pin:1' }
        reopenIntercept = @{ id = 3; kind = 'intercept'; attempt = 1; layer = 'test'; priority = 0; flags = '0x0000'; filter = 'udp'; reason = 'server_transition'; actor = 'auto'; command_id = 'TransitionRestart'; pin_active = $true; pin_id = 1; elapsed_us = 1500; ms_since_last_close = 3000; fwp_code = $null; owned_handles = 'pin:1,intercept:3'; result = 'ok'; snapshot_id = $null }
    }
    $records = @(
        @{ e = 'app_start'; f = $base.app_start },
        @{ e = 'interceptor_state'; f = $base.stateBoost },
        @{ e = 'windivert_open'; f = $base.pinOpen },
        @{ e = 'windivert_open'; f = $base.interceptOpen },
        @{ e = 'control_registration'; f = $base.regConnect },
        @{ e = 'watchdog'; f = $base.watchdogHealthy },
        @{ e = 'control_registration'; f = $base.regTransition }
    )
    if ($InjectTeardown) {
        $records += @(
            @{ e = 'interceptor_state'; f = $base.teardownState },
            @{ e = 'windivert_close'; f = $base.closeIntercept },
            @{ e = 'windivert_open'; f = $base.reopenIntercept },
            @{ e = 'watchdog'; f = $base.watchdogStarved }
        )
    } else {
        $records += @(
            @{ e = 'control_registration'; f = $base.regDiscarded },
            @{ e = 'watchdog'; f = $base.watchdogHealthy },
            @{ e = 'watchdog'; f = $base.watchdogHealthy }
        )
    }
    $seq = 0
    foreach ($entry in $records) {
        $seq++
        $record = New-P06FixtureRecord -Event $entry.e -Seq $seq -TsMs ($seed + ($seq * 1000)) -Fields $entry.f
        Write-P06Jsonl -Path $Path -Record $record
    }
    return $Path
}

function New-P06LedgerBadFixture {
    param([string]$Path)
    if (Test-Path -LiteralPath $Path) { Remove-Item -LiteralPath $Path -Force }
    $seed = [long]1763913700000
    $records = @(
        @{ e = 'app_start'; f = @{ version = '1.12.5'; commit = '6e30a65fe668'; windivert_dll_sha256 = 'C1E060EE19444A259B2162F8AF0F3FE8C4428A1C6F694DCE20DE194AC8D7D9A2'; windivert_sys_sha256 = '8DA085332782708D8767BCACE5327A6EC7283C17CFB85E40B03CD2323A90DDC2'; windivert_version = $null; elevated = $true; host_os = 'fixture'; boot_id = 'fixturebad001'; prev_shutdown = 'clean' } },
        @{ e = 'windivert_close'; f = @{ id = 9; kind = 'intercept'; close_age_ms = 100; result = 'ok'; pin_active = $false; owned_handles = '' } },
        @{ e = 'windivert_open'; f = @{ id = 4; kind = 'intercept'; attempt = 1; layer = 'test'; priority = 0; flags = '0'; filter = 'udp'; reason = 'user_start'; actor = 'user'; command_id = 'StartBoost'; pin_active = $false; pin_id = $null; elapsed_us = 10; ms_since_last_close = $null; fwp_code = $null; owned_handles = 'intercept:4'; result = 'ok'; snapshot_id = $null } },
        @{ e = 'windivert_close'; f = @{ id = 4; kind = 'intercept'; close_age_ms = 100; result = 'ok'; pin_active = $false; owned_handles = '' } },
        @{ e = 'windivert_close'; f = @{ id = 4; kind = 'intercept'; close_age_ms = 200; result = 'ok'; pin_active = $false; owned_handles = '' } },
        @{ e = 'control_registration'; f = @{ attempt = 1; outcome = 'ok'; session = 21; relay = 'relay-a:443'; node = $null; region = $null; reason = 'connect'; prev_session = $null; superseded = $false; close_reason = $null; generation = 1 } },
        @{ e = 'control_registration'; f = @{ attempt = 1; outcome = 'ok'; session = 22; relay = 'relay-b:443'; node = $null; region = $null; reason = 'relay_switch'; prev_session = $null; superseded = $false; close_reason = $null; generation = 2 } },
        @{ e = 'control_registration'; f = @{ attempt = 1; outcome = 'ok'; session = 22; relay = 'relay-b:443'; node = $null; region = $null; reason = 'destination_report'; prev_session = $null; superseded = $false; close_reason = $null; generation = 3 } },
        @{ e = 'control_registration'; f = @{ attempt = 1; outcome = 'superseded'; session = $null; relay = $null; node = $null; region = $null; reason = 'connect'; prev_session = $null; superseded = $true; close_reason = $null; generation = 4 } },
        @{ e = 'gui.action'; f = @{ action = 'optimize'; source = 'button'; from = 'idle'; to = 'idle'; command_id = 'DupCommand' } },
        @{ e = 'gui.action'; f = @{ action = 'stop'; source = 'tray'; from = 'idle'; to = 'idle'; command_id = 'DupCommand' } }
    )
    $seq = 0
    foreach ($entry in $records) {
        $seq++
        Write-P06Jsonl -Path $Path -Record (New-P06FixtureRecord -Event $entry.e -Seq $seq -TsMs ($seed + ($seq * 1000)) -Fields $entry.f)
    }
    return $Path
}

function New-P06SchemaBadFixture {
    param([string]$Path)
    if (Test-Path -LiteralPath $Path) { Remove-Item -LiteralPath $Path -Force }
    $seed = [long]1763913800000
    $records = @(
        @{ e = 'app_start'; f = @{ version = '1.12.5'; commit = ''; windivert_dll_sha256 = $null; windivert_sys_sha256 = $null; windivert_version = $null; elevated = $true; host_os = 'fixture'; boot_id = 'fixturebadschema'; prev_shutdown = 'clean' } },
        @{ e = 'interceptor_state'; f = @{ from = 'idle'; to = 'boosting'; reason = 'nonsense_reason'; actor = 'user'; command_id = 'StartBoost'; pin_id = $null; pin_active = $false; handle_ids = ''; locked_server = $null; relay = $null; duration_ms = 0; attempt = $null; max_attempts = $null; prev_event_id = $null } },
        @{ e = 'unknown_event_xyz'; f = @{ anything = 1 } }
    )
    $seq = 0
    foreach ($entry in $records) {
        $seq++
        Write-P06Jsonl -Path $Path -Record (New-P06FixtureRecord -Event $entry.e -Seq $seq -TsMs ($seed + ($seq * 1000)) -Fields $entry.f)
    }
    return $Path
}

function Invoke-P06LifecycleSelfTest {
    param([string]$WorkDir)
    if (-not (Test-Path -LiteralPath $WorkDir)) {
        New-Item -ItemType Directory -Path $WorkDir -Force | Out-Null
    }
    $a2Clean = New-P06A2Fixture -Path (Join-Path $WorkDir 'fixture-a2-clean.jsonl')
    $a2Teardown = New-P06A2Fixture -Path (Join-Path $WorkDir 'fixture-a2-teardown.jsonl') -InjectTeardown
    $ledgerBad = New-P06LedgerBadFixture -Path (Join-Path $WorkDir 'fixture-ledger-bad.jsonl')
    $schemaBad = New-P06SchemaBadFixture -Path (Join-Path $WorkDir 'fixture-schema-bad.jsonl')

    $cleanAudit = Invoke-P06LifecycleAudit -Path $a2Clean
    $teardownAudit = Invoke-P06LifecycleAudit -Path $a2Teardown
    $ledgerAudit = Invoke-P06LifecycleAudit -Path $ledgerBad
    $schemaAudit = Invoke-P06LifecycleAudit -Path $schemaBad

    $checks = @()
    $checks += [ordered]@{
        name = 'landed-schema-accepted'
        fixture = 'fixture-a2-clean.jsonl'
        expected = 'schema ok'
        actual = $(if ($cleanAudit.schema_validation.ok) { 'schema ok' } else { 'schema errors: ' + (@($cleanAudit.schema_validation.errors) -join '; ') })
        pass = [bool]$cleanAudit.schema_validation.ok
    }
    $checks += [ordered]@{
        name = 'handle-ledger-clean-passes'
        fixture = 'fixture-a2-clean.jsonl'
        expected = 'ledger ok'
        actual = $(if ($cleanAudit.handle_ledger.ok) { 'ledger ok' } else { 'ledger errors: ' + (@($cleanAudit.handle_ledger.errors) -join '; ') })
        pass = [bool]$cleanAudit.handle_ledger.ok
    }
    $checks += [ordered]@{
        name = 'registration-ledger-clean-passes'
        fixture = 'fixture-a2-clean.jsonl'
        expected = 'ledger ok, one active session, discarded candidate resolved'
        actual = $(if ($cleanAudit.registration_ledger.ok) { 'ledger ok' } else { 'ledger errors: ' + (@($cleanAudit.registration_ledger.errors) -join '; ') })
        pass = ([bool]$cleanAudit.registration_ledger.ok -and [int]$cleanAudit.registration_ledger.max_active -le 1)
    }
    $checks += [ordered]@{
        name = 'a2-clean-zero-teardown'
        fixture = 'fixture-a2-clean.jsonl'
        expected = 'PASS_ASSERTION'
        actual = [string]$cleanAudit.a2_transition.status
        pass = ($cleanAudit.a2_transition.status -eq 'PASS_ASSERTION')
    }
    $checks += [ordered]@{
        name = 'a2-detects-injected-teardown'
        fixture = 'fixture-a2-teardown.jsonl'
        expected = 'FAIL_ASSERTION'
        actual = [string]$teardownAudit.a2_transition.status
        pass = ($teardownAudit.a2_transition.status -eq 'FAIL_ASSERTION' -and [int]$teardownAudit.a2_transition.teardown_count -gt 0)
    }
    $closeWithoutOpenDetected = $false
    foreach ($error in @($ledgerAudit.handle_ledger.errors)) {
        if ($error -match 'without any open|never opened') { $closeWithoutOpenDetected = $true }
    }
    $checks += [ordered]@{
        name = 'handle-ledger-detects-close-without-open-and-duplicate-close'
        fixture = 'fixture-ledger-bad.jsonl'
        expected = 'ledger errors'
        actual = ('errors={0}' -f @($ledgerAudit.handle_ledger.errors).Count)
        pass = ($closeWithoutOpenDetected -and -not $ledgerAudit.handle_ledger.ok)
    }
    $duplicateActiveDetected = $false
    foreach ($error in @($ledgerAudit.registration_ledger.errors)) {
        if ($error -match 're-published while already active|active at once') { $duplicateActiveDetected = $true }
    }
    $checks += [ordered]@{
        name = 'registration-ledger-detects-duplicate-active-session'
        fixture = 'fixture-ledger-bad.jsonl'
        expected = 'ledger errors'
        actual = ('errors={0}' -f @($ledgerAudit.registration_ledger.errors).Count)
        pass = $duplicateActiveDetected
    }
    $unresolvableDetected = $false
    foreach ($error in @($ledgerAudit.registration_ledger.errors)) {
        if ($error -match 'cannot be resolved') { $unresolvableDetected = $true }
    }
    $checks += [ordered]@{
        name = 'registration-ledger-detects-unresolvable-superseded-row'
        fixture = 'fixture-ledger-bad.jsonl'
        expected = 'ledger errors'
        actual = $(if ($unresolvableDetected) { 'detected' } else { 'missed' })
        pass = $unresolvableDetected
    }
    $checks += [ordered]@{
        name = 'gui-action-exactly-once-detected'
        fixture = 'fixture-ledger-bad.jsonl'
        expected = 'ui action errors'
        actual = ('errors={0}' -f @($ledgerAudit.ui_actions.errors).Count)
        pass = (-not $ledgerAudit.ui_actions.ok)
    }
    $checks += [ordered]@{
        name = 'schema-rejects-unknown-event-and-bad-enum'
        fixture = 'fixture-schema-bad.jsonl'
        expected = 'schema errors'
        actual = ('errors={0}' -f @($schemaAudit.schema_validation.errors).Count)
        pass = (-not $schemaAudit.schema_validation.ok)
    }
    $failures = @($checks | Where-Object { -not $_.pass })
    return [ordered]@{
        schema = 'p0-6-lifecycle-selftest/v1'
        ran_at_utc = (Get-P06UtcNow)
        work_dir = $WorkDir
        fixture_files = @($a2Clean, $a2Teardown, $ledgerBad, $schemaBad)
        check_count = @($checks).Count
        failed_count = @($failures).Count
        ok = (@($failures).Count -eq 0)
        checks = @($checks)
        audits = [ordered]@{
            a2_clean = $cleanAudit
            a2_teardown = $teardownAudit
            ledger_bad = $ledgerAudit
            schema_bad = $schemaAudit
        }
    }
}
