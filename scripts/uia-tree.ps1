<#
.SYNOPSIS
    Dump the live Windows UI Automation tree of the LightSpeed GUI to JSON.

.DESCRIPTION
    The LightSpeed GUI is an egui/eframe app whose controls are exposed through
    AccessKit's Windows adapter -- that is, the standard UI Automation (UIA)
    tree, the same surface Narrator or any automation driver sees. This script
    is the live end-to-end half of the accessibility contract; the offline half
    is `lightspeed-gui --a11y-dump <path>`.

    It attaches to a running instance (`-ProcessId`) or launches one (`-ExePath`),
    waits for AccessKit's lazy tree activation, walks the whole tree, and writes
    nested JSON. `-ExpectName` / `-ExpectAutomationId` turn it into an assertion
    usable by CI and by agent drivers:

        powershell -ExecutionPolicy Bypass -File scripts/uia-tree.ps1 `
            -ProcessId 1234 `
            -ExpectAutomationId main.action.primary,main.state.word `
            -OutFile wat/evidence/2026-11-23/uia/live-tree.json

    Each node carries the UIA facts an assistive technology consumes: name,
    control type, enabled/offscreen/focus state, `live_setting`, the
    ExpandCollapsePattern state (`expand_collapse`: Collapsed / Expanded /
    PartiallyExpanded / LeafNode) and the TogglePattern state (`toggle_state`:
    Off / On / Indeterminate). A pattern the provider does not support is
    reported as null, never as an exception string.

    Exit codes:
        0   the tree was dumped and every expectation matched
        1   the tree was dumped but at least one expectation is missing
        2   setup failure: no process, no window, or UIA never activated

    Windows PowerShell 5.1 only, no external modules. ASCII, LF.
#>
[CmdletBinding()]
param(
    # Attach to an already-running lightspeed-gui.exe. 0 means "launch -ExePath".
    [int]$ProcessId = 0,

    # Executable to launch when -ProcessId is not given.
    [string]$ExePath = '',

    # Where the nested tree JSON is written.
    [string]$OutFile = (Join-Path $env:TEMP 'lightspeed-uia-tree.json'),

    # Accessible-name expectations; a missing one exits 1. Accepts a
    # comma-separated list in one argument (the `-File` invocation path).
    [string[]]$ExpectName = @(),

    # AutomationId expectations (`main.ledger.*` wildcards work); a missing one exits 1.
    # Accepts a comma-separated list in one argument (the `-File` invocation path).
    [string[]]$ExpectAutomationId = @(),

    # Seconds to wait for the window and for AccessKit to publish its tree.
    [int]$TimeoutSeconds = 20,

    # Stop the process at the end, but only when this script launched it.
    [switch]$Stop
)

# The script runs under `powershell -File` too; keep failure modes explicit
# rather than letting an unhandled exception exit with an unrelated code.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Write-Out {
    param([string]$Text = '')
    # Write-Output, not Write-Host: `powershell -File ... > transcript.txt`
    # only captures the output stream on Windows PowerShell 5.1.
    Write-Output $Text
}

# Exit code 2 is "could not get a tree at all"; 1 is "tree dumped, assertion
# failed". Reporting goes to the output stream so a transcript captures it.
function Stop-SetupError {
    param([string]$Text)
    Write-Out ('ERROR: ' + $Text)
    exit 2
}

# --- UI Automation client assemblies -----------------------------------------
try {
    Add-Type -AssemblyName UIAutomationClient
    Add-Type -AssemblyName UIAutomationTypes
} catch {
    Stop-SetupError ('the UI Automation client assemblies are unavailable: ' + $_.Exception.Message)
}

$script:NodeCount = 0
$script:MaxNodes = 5000
$script:MaxDepth = 64
$script:Truncated = $false
$script:Walker = [System.Windows.Automation.TreeWalker]::RawViewWalker
$script:TrueCondition = [System.Windows.Automation.Condition]::TrueCondition
$script:Descendants = [System.Windows.Automation.TreeScope]::Descendants
$script:Children = [System.Windows.Automation.TreeScope]::Children

# --- Small safe readers -------------------------------------------------------
# AccessKit publishes its tree lazily and rebuilds nodes on every frame, so any
# single property read can raise ElementNotAvailableException mid-walk. A dump
# that dies on the first stale element is useless: every read falls back.

function Get-CurrentField {
    param($Element, [scriptblock]$Block, $Fallback = $null)
    try {
        $value = & $Block
        if ($null -eq $value) { return $Fallback }
        return $value
    } catch {
        return $Fallback
    }
}

function Get-Bounds {
    param($Element)
    $rect = Get-CurrentField $Element { $Element.Current.BoundingRectangle } $null
    if ($null -eq $rect) { return $null }
    # UIA reports an empty rect (infinite coordinates) for elements without a
    # layout; Infinity is not valid JSON, so it is reported as null instead.
    foreach ($number in @($rect.X, $rect.Y, $rect.Width, $rect.Height)) {
        if ([double]::IsNaN([double]$number) -or [double]::IsInfinity([double]$number)) {
            return $null
        }
    }
    if ($rect.Width -le 0 -or $rect.Height -le 0) { return $null }
    return [pscustomobject][ordered]@{
        x      = [math]::Round([double]$rect.X, 1)
        y      = [math]::Round([double]$rect.Y, 1)
        width  = [math]::Round([double]$rect.Width, 1)
        height = [math]::Round([double]$rect.Height, 1)
    }
}

function Get-ElementValue {
    param($Element)
    try {
        $pattern = $Element.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
        if ($null -eq $pattern) { return $null }
        return [string]$pattern.Current.Value
    } catch {
        return $null
    }
}

# ExpandCollapsePattern: the disclosure/combobox state a screen reader reports as
# "expanded"/"collapsed". AccessKit's Windows adapter implements the pattern for
# any node that carries an expanded state and for combo boxes, so the value is
# reported next to live_setting. Unsupported nodes (the common case) fall back to
# null rather than an exception. LeafNode is the adapter's answer for a control
# that does not carry a disclosure state at all.
function Get-ExpandCollapseState {
    param($Element)
    try {
        $pattern = $Element.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
        if ($null -eq $pattern) { return $null }
        $state = $pattern.Current.ExpandCollapseState
        switch ([int]$state) {
            0 { return 'Collapsed' }
            1 { return 'Expanded' }
            2 { return 'PartiallyExpanded' }
            3 { return 'LeafNode' }
            default { return [string]$state }
        }
    } catch {
        return $null
    }
}

# TogglePattern: checkbox/switch state as Off/On/Indeterminate. Same adapter
# mapping as the offline dump's `toggled` field, so the two can be compared.
function Get-ToggleState {
    param($Element)
    try {
        $pattern = $Element.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
        if ($null -eq $pattern) { return $null }
        $state = $pattern.Current.ToggleState
        switch ([int]$state) {
            0 { return 'Off' }
            1 { return 'On' }
            2 { return 'Indeterminate' }
            default { return [string]$state }
        }
    } catch {
        return $null
    }
}

function Get-LiveSetting {
    param($Element)
    try {
        $value = $Element.GetCurrentPropertyValue([System.Windows.Automation.AutomationElementIdentifiers]::LiveSettingProperty)
    } catch {
        return $null
    }
    if ($null -eq $value) { return $null }
    # The sentinel is a singleton object, not a string, so compare by reference.
    if ([object]::ReferenceEquals($value, [System.Windows.Automation.AutomationElement]::NotSupported)) {
        return $null
    }
    # An unsupported UIA property does not always throw: some providers (the
    # AccessKit Windows adapter among them) make the client return the
    # ArgumentException as a *value*. Treat that as "not exposed" instead of
    # stringifying the exception into the tree JSON.
    if ($value -is [System.Exception]) { return $null }
    # UIA_LiveSettingProperty is VT_I4: 0 Off, 1 Polite, 2 Assertive. Report the
    # same names the offline AccessKit dump uses so the two can be compared.
    if ($value -is [int] -or $value -is [long]) {
        switch ([int]$value) {
            0 { return 'Off' }
            1 { return 'Polite' }
            2 { return 'Assertive' }
            default { return [string]$value }
        }
    }
    return [string]$value
}

function New-UiaNode {
    param($Element, [int]$Depth)

    $name = [string](Get-CurrentField $Element { $Element.Current.Name } '')
    $automationId = [string](Get-CurrentField $Element { $Element.Current.AutomationId } '')
    $controlType = [string](Get-CurrentField $Element { $Element.Current.ControlType.ProgrammaticName } '')
    $localizedType = [string](Get-CurrentField $Element { $Element.Current.LocalizedControlType } '')
    $isEnabled = [bool](Get-CurrentField $Element { $Element.Current.IsEnabled } $false)
    $isOffscreen = [bool](Get-CurrentField $Element { $Element.Current.IsOffscreen } $true)
    $focusable = [bool](Get-CurrentField $Element { $Element.Current.IsKeyboardFocusable } $false)
    $focused = [bool](Get-CurrentField $Element { $Element.Current.HasKeyboardFocus } $false)

    return [pscustomobject][ordered]@{
        name                   = $name
        automation_id          = $automationId
        control_type           = $controlType
        localized_control_type = $localizedType
        is_enabled             = $isEnabled
        is_offscreen           = $isOffscreen
        focusable              = $focusable
        has_keyboard_focus     = $focused
        live_setting           = (Get-LiveSetting $Element)
        expand_collapse        = (Get-ExpandCollapseState $Element)
        toggle_state           = (Get-ToggleState $Element)
        bounds                 = (Get-Bounds $Element)
        value                  = (Get-ElementValue $Element)
        depth                  = $Depth
        children               = @()
    }
}

function Get-ChildElements {
    param($Element)
    $children = New-Object System.Collections.ArrayList
    $child = $null
    try {
        $child = $script:Walker.GetFirstChild($Element)
    } catch {
        $child = $null
    }
    while ($null -ne $child) {
        [void]$children.Add($child)
        $next = $null
        try {
            $next = $script:Walker.GetNextSibling($child)
        } catch {
            $next = $null
        }
        $child = $next
    }
    return @($children)
}

function Get-UiaTree {
    param($Element, [int]$Depth)

    if ($script:NodeCount -ge $script:MaxNodes) {
        $script:Truncated = $true
        return $null
    }
    $script:NodeCount = $script:NodeCount + 1

    $node = New-UiaNode $Element $Depth
    if ($Depth -ge $script:MaxDepth) {
        if ((Get-ChildElements $Element).Count -gt 0) { $script:Truncated = $true }
        return $node
    }

    $kidNodes = New-Object System.Collections.ArrayList
    foreach ($child in (Get-ChildElements $Element)) {
        if ($script:NodeCount -ge $script:MaxNodes) {
            $script:Truncated = $true
            break
        }
        $kid = Get-UiaTree $child ($Depth + 1)
        if ($null -ne $kid) { [void]$kidNodes.Add($kid) }
    }
    $node.children = @($kidNodes)
    return $node
}

function Get-FlatTree {
    param($Node)
    $flat = New-Object System.Collections.ArrayList
    $pending = New-Object System.Collections.Stack
    $pending.Push($Node)
    while ($pending.Count -gt 0) {
        $current = $pending.Pop()
        [void]$flat.Add($current)
        $kids = $current.children
        for ($index = $kids.Count - 1; $index -ge 0; $index--) {
            $pending.Push($kids[$index])
        }
    }
    return @($flat)
}

function Test-NameMatch {
    param([string]$Candidate, [string]$Expected)
    if ([string]::IsNullOrEmpty($Candidate)) { return $false }
    if ($Candidate -ieq $Expected) { return $true }
    # Substring match keeps `-ExpectName Settings` usable when a localized
    # label carries decorative text around it.
    return ($Candidate -like ('*' + $Expected + '*'))
}

function Test-IdMatch {
    param([string]$Candidate, [string]$Expected)
    if ([string]::IsNullOrEmpty($Candidate)) { return $false }
    if ($Expected.Contains('*') -or $Expected.Contains('?')) {
        return ($Candidate -like $Expected)
    }
    # AutomationIds are a stable, case-sensitive contract.
    return ($Candidate -ceq $Expected)
}

# `powershell -File script.ps1 -ExpectAutomationId a,b,c` hands the whole
# "a,b,c" token to the parameter as one string (the comma is not an array
# separator on that path), so every expectation would be reported missing even
# when the id is in the tree. Split comma-separated lists ourselves; repeated
# -ExpectAutomationId arguments still work because each element is expanded.
function Expand-ExpectationList {
    param([string[]]$Values)
    $expanded = New-Object System.Collections.ArrayList
    foreach ($value in $Values) {
        if ($null -eq $value) { continue }
        foreach ($part in ([string]$value -split ',')) {
            $trimmed = $part.Trim()
            if (-not [string]::IsNullOrEmpty($trimmed)) { [void]$expanded.Add($trimmed) }
        }
    }
    return @($expanded)
}

# Normalize the assertion lists once: either comma-separated in one argument or
# repeated as separate arguments.
$ExpectAutomationId = @(Expand-ExpectationList $ExpectAutomationId)
$ExpectName = @(Expand-ExpectationList $ExpectName)

# --- Resolve or launch the process -------------------------------------------
if ($ProcessId -eq 0) {
    if ([string]::IsNullOrWhiteSpace($ExePath)) {
        Stop-SetupError 'pass -ProcessId <pid> to attach, or -ExePath <exe> to launch.'
    }
    if (-not (Test-Path -LiteralPath $ExePath -PathType Leaf)) {
        Stop-SetupError ('executable not found: ' + $ExePath)
    }
}

$script:OwnsProcess = $false
$startedProcess = $null
$targetProcess = $null
$windowHandle = [IntPtr]::Zero
$windowElement = $null
$tree = $null
$exitCode = 2

try {
    if ($ProcessId -ne 0) {
        try {
            $targetProcess = Get-Process -Id $ProcessId -ErrorAction Stop
        } catch {
            Stop-SetupError ('no process with id ' + $ProcessId + ': ' + $_.Exception.Message)
        }
        Write-Out ('Attaching to lightspeed-gui pid ' + $ProcessId)
    } else {
        try {
            # A GUI-subsystem app: Start-Process does not wait for it.
            $startedProcess = Start-Process -FilePath $ExePath -PassThru
        } catch {
            Stop-SetupError ('could not launch ' + $ExePath + ': ' + $_.Exception.Message)
        }
        $script:OwnsProcess = $true
        $targetProcess = $startedProcess
        $ProcessId = $targetProcess.Id
        Write-Out ('Launched ' + $ExePath + ' pid ' + $ProcessId)
    }

    # --- Wait for the status window and for AccessKit to publish its tree ------
    # Two traps on this host:
    #   1. `Process.MainWindowHandle` is unreliable for winit: a visible helper
    #      window ("Winit Thread Event Target") can be created before the status
    #      window and .NET may return that one, which has no accessibility
    #      subtree at all. So windows are addressed through UI Automation itself
    #      and the candidate with a populated subtree wins.
    #   2. AccessKit's Windows adapter builds its tree lazily: it answers the
    #      first WM_GETOBJECT with a placeholder and fills the tree only after
    #      the app rendered a frame, so the probes are retried.
    $activationThreshold = 10
    $activationAttempts = 0
    $descendantCount = 0
    $windowElement = $null
    $activationWatch = [System.Diagnostics.Stopwatch]::StartNew()
    $activationDeadline = (Get-Date).AddSeconds([Math]::Max(1, $TimeoutSeconds))
    $processIdCondition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $ProcessId)
    $desktop = [System.Windows.Automation.AutomationElement]::RootElement

    while ($true) {
        $activationAttempts = $activationAttempts + 1
        $targetProcess.Refresh()
        if ($targetProcess.HasExited) {
            if ($null -ne $startedProcess) {
                $code = 'unknown'
                try { $code = [string]$targetProcess.ExitCode } catch { $code = 'unknown' }
                if ($code -eq '2') {
                    Stop-SetupError 'the launched process exited with code 2: the single-instance guard reports another LightSpeed instance already running.'
                }
                Stop-SetupError ('the launched process exited (code ' + $code + ') before it opened a window.')
            }
            Stop-SetupError ('pid ' + $ProcessId + ' exited before it opened a window.')
        }

        $topLevel = @()
        try {
            $topLevel = @($desktop.FindAll($script:Children, $processIdCondition))
        } catch {
            $topLevel = @()
        }
        if ($topLevel.Count -eq 0) {
            # Fallback for providers that the desktop does not enumerate: the
            # process's own main-window handle. Only used when nothing else is
            # found, because that handle is the unreliable part above.
            $handle = [long]$targetProcess.MainWindowHandle
            if ($handle -ne 0) {
                try {
                    $fromHandle = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$handle)
                    if ($null -ne $fromHandle) { $topLevel = @($fromHandle) }
                } catch {
                    $topLevel = @()
                }
            }
        }

        foreach ($candidate in $topLevel) {
            $count = -1
            try {
                $count = $candidate.FindAll($script:Descendants, $script:TrueCondition).Count
            } catch {
                $count = -1
            }
            if ($count -gt $descendantCount) {
                $descendantCount = $count
                $windowElement = $candidate
            }
        }

        if ($descendantCount -ge $activationThreshold) { break }
        if ((Get-Date) -gt $activationDeadline) { break }
        Start-Sleep -Milliseconds 150
    }
    $activationWatch.Stop()

    Write-Out ('Activation: ' + $activationAttempts + ' probe(s), ' + $activationWatch.ElapsedMilliseconds + ' ms, ' + $descendantCount + ' descendants (threshold ' + $activationThreshold + ')')

    if ($null -eq $windowElement) {
        Stop-SetupError ('pid ' + $ProcessId + ' has no UI Automation top-level window; it may be hidden in the tray.')
    }

    # Keep the window handle for metadata; UIA's own NativeWindowHandle is the
    # authoritative one for the element that actually publishes the tree.
    $nativeHandle = [long](Get-CurrentField $windowElement { $windowElement.Current.NativeWindowHandle } 0)
    if ($nativeHandle -eq 0) { $nativeHandle = [long]$targetProcess.MainWindowHandle }
    $windowHandle = [IntPtr]$nativeHandle
    Write-Out ('Main window handle: 0x{0:X}' -f $nativeHandle)

    if ($descendantCount -lt $activationThreshold) {
        Stop-SetupError ('the UIA tree never activated (best window exposes ' + $descendantCount + ' descendants, threshold ' + $activationThreshold + '); AccessKit may be inert in this build.')
    }

    # The chosen element must still be alive for the walk; a rebuilt tree can
    # invalidate it, in which case a retry is cheaper than a half-empty dump.
    try {
        [void]$windowElement.Current.Name
    } catch {
        Stop-SetupError 'the selected window element went stale before the tree walk; re-run the script.'
    }

    # Walk the real tree, not the probe list, so nesting is preserved.
    $script:NodeCount = 0
    $script:Truncated = $false
    $tree = Get-UiaTree $windowElement 0

    # --- Flatten, assert, serialize -----------------------------------------
    $flat = Get-FlatTree $tree
    $windowTitle = [string](Get-CurrentField $windowElement { $windowElement.Current.Name } '')
    $exePath = ''
    try {
        $exePath = [string]$targetProcess.Path
    } catch {
        $exePath = $ExePath
    }
    if ([string]::IsNullOrEmpty($exePath) -and -not [string]::IsNullOrEmpty($ExePath)) {
        $exePath = $ExePath
    }

    $automationIds = @($flat | Where-Object { -not [string]::IsNullOrEmpty($_.automation_id) } | ForEach-Object { [string]$_.automation_id } | Sort-Object -Unique)
    $visibleTexts = @($flat | ForEach-Object {
        if (-not [string]::IsNullOrEmpty($_.name)) { [string]$_.name }
        if (-not [string]::IsNullOrEmpty($_.value)) { [string]$_.value }
    })

    $missingIds = New-Object System.Collections.ArrayList
    foreach ($expected in $ExpectAutomationId) {
        $matched = $false
        foreach ($candidate in $automationIds) {
            if (Test-IdMatch $candidate $expected) { $matched = $true; break }
        }
        # Ids can legitimately sit on nodes whose value was unreadable; fall back
        # to the flat list so the assertion does not depend on the unique sort.
        if (-not $matched) {
            foreach ($node in $flat) {
                if (Test-IdMatch ([string]$node.automation_id) $expected) { $matched = $true; break }
            }
        }
        if (-not $matched) { [void]$missingIds.Add($expected) }
    }

    $missingNames = New-Object System.Collections.ArrayList
    foreach ($expected in $ExpectName) {
        $matched = $false
        foreach ($candidate in $visibleTexts) {
            if (Test-NameMatch $candidate $expected) { $matched = $true; break }
        }
        if (-not $matched) { [void]$missingNames.Add($expected) }
    }

    $exitCode = 0
    if ($missingIds.Count -gt 0 -or $missingNames.Count -gt 0) {
        $exitCode = 1
    }

    $document = [pscustomobject][ordered]@{
        pid           = $ProcessId
        exe           = $exePath
        window_title  = $windowTitle
        window_handle = [long]$windowHandle.ToInt64()
        generated_at  = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
        node_count    = $flat.Count
        scheme        = 'raw'
        truncated     = $script:Truncated
        activation    = [pscustomobject][ordered]@{
            attempts    = $activationAttempts
            waited_ms   = $activationWatch.ElapsedMilliseconds
            descendants = $descendantCount
            threshold   = $activationThreshold
        }
        expectations  = [pscustomobject][ordered]@{
            automation_ids         = @($ExpectAutomationId)
            names                  = @($ExpectName)
            missing_automation_ids = @($missingIds)
            missing_names          = @($missingNames)
        }
        tree          = $tree
    }

    $json = $document | ConvertTo-Json -Depth 100
    $outDirectory = Split-Path -Path $OutFile -Parent
    if (-not [string]::IsNullOrEmpty($outDirectory) -and -not (Test-Path -LiteralPath $outDirectory -PathType Container)) {
        New-Item -ItemType Directory -Path $outDirectory -Force | Out-Null
    }
    # UTF-8 without a BOM: this file is machine input for the evidence pipeline.
    [System.IO.File]::WriteAllText($OutFile, $json, (New-Object System.Text.UTF8Encoding($false)))

    # --- Human summary -------------------------------------------------------
    Write-Out ''
    Write-Out '== LightSpeed live UIA tree =='
    Write-Out ('window title : ' + $windowTitle)
    Write-Out ('pid          : ' + $ProcessId)
    Write-Out ('hwnd         : 0x{0:X}' -f $windowHandle.ToInt64())
    Write-Out ('scheme       : raw')
    Write-Out ('nodes        : ' + $flat.Count)
    if ($script:Truncated) { Write-Out 'truncated    : yes (node/depth cap reached)' }

    $typeCounts = @($flat | Where-Object { -not [string]::IsNullOrEmpty($_.control_type) } | ForEach-Object { $_.control_type -replace '^ControlType\.', '' } | Group-Object | Sort-Object -Property Count -Descending)
    if ($typeCounts.Count -gt 0) {
        $typeText = ($typeCounts | ForEach-Object { $_.Name + ' ' + $_.Count }) -join ', '
        Write-Out ('by type      : ' + $typeText)
    }

    if ($automationIds.Count -gt 0) {
        Write-Out ('automation ids (' + $automationIds.Count + '): ' + ($automationIds -join ', '))
    } else {
        Write-Out 'automation ids (0): none exposed'
    }

    if ($missingIds.Count -gt 0) {
        Write-Out ('MISSING automation ids: ' + (@($missingIds) -join ', '))
    }
    if ($missingNames.Count -gt 0) {
        Write-Out ('MISSING names: ' + (@($missingNames) -join ', '))
    }
    if ($missingIds.Count -eq 0 -and $missingNames.Count -eq 0) {
        Write-Out ('expectations : ' + @($ExpectAutomationId).Count + ' automation id(s), ' + @($ExpectName).Count + ' name(s) -- all present')
    }

    Write-Out ('json         : ' + $OutFile)
}
catch {
    Stop-SetupError ($_.Exception.Message)
}
finally {
    # Contract: stop the process only when this script started it, and only
    # when -Stop was asked for or the run failed. An attached instance is
    # never touched.
    if ($script:OwnsProcess -and $null -ne $startedProcess) {
        $alive = $false
        try {
            $startedProcess.Refresh()
            $alive = -not $startedProcess.HasExited
        } catch {
            $alive = $false
        }
        if ($alive) {
            if ($Stop -or ($exitCode -ne 0)) {
                try {
                    Stop-Process -Id $ProcessId -Force -ErrorAction Stop
                    Write-Out ('Stopped pid ' + $ProcessId)
                } catch {
                    Write-Out ('WARNING: could not stop pid ' + $ProcessId + ': ' + $_.Exception.Message)
                }
            } else {
                Write-Out ('Left running: pid ' + $ProcessId + ' (pass -Stop to stop it)')
            }
        }
    }
}

exit $exitCode
