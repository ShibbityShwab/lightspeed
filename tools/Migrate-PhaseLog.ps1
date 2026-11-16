# One-off migration (2026-11-16): dedupe + split wat/state/current-phase.md.
# Run with -Apply to write; without it, prints a plan and verification counts.
param([switch]$Apply)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
if (-not (Test-Path (Join-Path $root 'wat\state\current-phase.md'))) {
    $root = 'C:\Users\ShibbityShwab\Documents\GitHub\lightspeed'
}
$logPath = Join-Path $root 'wat\state\current-phase.md'
$phasesDir = Join-Path $root 'wat\state\phases'
$lines = [System.IO.File]::ReadAllLines($logPath)
if (($lines | Select-String -SimpleMatch '## History' -Quiet)) {
    Write-Host 'This log is already migrated (it has a "## History" section).'
    Write-Host 'Routine rotation: move the old dated entry verbatim into'
    Write-Host 'wat/state/phases/<YYYY-MM>.md, demote its headings one level, and update'
    Write-Host 'the History table in wat/state/current-phase.md. See wat/state/README.md.'
    exit 2
}
Write-Host "read $($lines.Count) lines"

# ---- 1. Dedupe the repeated tail -------------------------------------------
# The file grew a second copy of the WF-052..WF-023 phase blocks. Detect the
# duplicate run by block hash (blocks start at a line beginning with '# ').
$blocks = @()
$cur = $null
for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match '^# ') {
        if ($cur) { $blocks += $cur }
        $cur = [pscustomobject]@{ Start = $i; Lines = [System.Collections.Generic.List[string]]::new() }
    }
    if ($cur) { $cur.Lines.Add($lines[$i]) } elseif (-not $cur) {
        # preamble before the first heading
    }
}
if ($cur) { $blocks += $cur }
if (-not $blocks) { throw 'no # blocks found' }

$seen = @{}
$keep = New-Object System.Collections.Generic.List[object]
$dropped = New-Object System.Collections.Generic.List[object]
foreach ($b in $blocks) {
    # A phase block is identified by its heading; a repeated heading is a
    # duplicated block even when the copy was truncated (the WF-025 tail was).
    $heading = $b.Lines[0]
    if ($seen.ContainsKey($heading)) {
        $dropped.Add($b)
    } else {
        $seen[$heading] = $b.Start
        $keep.Add($b)
    }
}
Write-Host "blocks=$($blocks.Count) kept=$($keep.Count) dropped=$($dropped.Count)"
if ($dropped.Count -gt 0) {
    Write-Host ("dropped line ranges: " + (($dropped | ForEach-Object { "$($_.Start + 1)" }) -join ', '))
    # every dropped block must be byte-identical to a kept one, which the hash above proves
}

# Rebuild the deduped line list: preamble + kept blocks in order.
$firstStart = $blocks[0].Start
$deduped = New-Object System.Collections.Generic.List[string]
for ($i = 0; $i -lt $firstStart; $i++) { $deduped.Add($lines[$i]) }
foreach ($b in $keep) { foreach ($l in $b.Lines) { $deduped.Add($l) } }
Write-Host "deduped lines=$($deduped.Count) (was $($lines.Count))"

# ---- 2. Split into H1/H2 blocks --------------------------------------------
$hb = @()
$cur2 = $null
foreach ($line in $deduped) {
    if ($line -match '^(#{1,2}) ') {
        if ($cur2) { $hb += $cur2 }
        $cur2 = [pscustomobject]@{
            Level = $Matches[1].Length
            Heading = $line
            Lines = [System.Collections.Generic.List[string]]::new()
        }
    }
    if ($cur2) { $cur2.Lines.Add($line) }
}
if ($cur2) { $hb += $cur2 }

# ---- 2b. Drop identical duplicate H2 entries -------------------------------
# The log had the same dated entry pasted twice inside different phase blocks.
# Two copies with the same body are one entry; copies whose bodies differ are
# kept and counted, because dropping them would lose text.
$seenH2 = @{}
$hbFiltered = New-Object System.Collections.Generic.List[object]
$h2Dropped = 0
foreach ($b in $hb) {
    if ($b.Level -eq 2) {
        $bodyText = ($b.Lines -join "`n").TrimEnd()
        $sha = (Get-FileHash -InputStream ([System.IO.MemoryStream]::new([System.Text.Encoding]::UTF8.GetBytes($bodyText))) -Algorithm SHA1).Hash
        if ($seenH2.ContainsKey($sha)) { $h2Dropped++; continue }
        $seenH2[$sha] = $b.Heading
    }
    $hbFiltered.Add($b)
}
$byHeading = $hbFiltered | Where-Object { $_.Level -eq 2 } | Group-Object Heading
$h2KeptDiff = ($byHeading | Where-Object { $_.Count -gt 1 }).Count
Write-Host "H2 identical duplicates dropped=$h2Dropped; same-heading different-body pairs kept=$h2KeptDiff"
$hb = $hbFiltered

$lastDate = $null
foreach ($b in $hb) {
    $m = [regex]::Match($b.Heading, '(20\d{2}-\d{2}-\d{2})')
    if ($m.Success) {
        $b | Add-Member -NotePropertyName Date -NotePropertyValue $m.Groups[1].Value -Force
        $lastDate = $m.Groups[1].Value
    } else {
        $b | Add-Member -NotePropertyName Date -NotePropertyValue $lastDate -Force
    }
}

$months = $hb | Where-Object { $_.Date } | ForEach-Object { $_.Date.Substring(0, 7) } | Sort-Object -Unique
Write-Host ("months: " + ($months -join ', '))
if (-not $months) { throw 'no dated blocks found' }
$newestMonth = $months[-1]
Write-Host "newest month: $newestMonth"

function Demote([string]$line) {
    if ($line -match '^(#{1,6}) ') { return '#' + $line }
    return $line
}

$byMonth = @{}
foreach ($b in $hb) {
    $month = if ($b.Date) { $b.Date.Substring(0, 7) } else { $newestMonth }
    if (-not $byMonth.ContainsKey($month)) { $byMonth[$month] = New-Object System.Collections.Generic.List[object] }
    $byMonth[$month].Add($b)
}

# ---- 3. Report what would be written ---------------------------------------
foreach ($month in ($byMonth.Keys | Sort-Object)) {
    $n = $byMonth[$month].Count
    $titles = ($byMonth[$month] | Where-Object { $_.Level -eq 2 } | ForEach-Object { $_.Heading.TrimStart('#').Trim() }) -join ' | '
    Write-Host ("{0}: {1} blocks :: {2}" -f $month, $n, $titles.Substring(0, [Math]::Min(160, $titles.Length)))
}

if (-not $Apply) {
    Write-Host 'DRY RUN - pass -Apply to write files'
    return
}

# ---- 4. Write month files ---------------------------------------------------
New-Item -ItemType Directory -Force -Path $phasesDir | Out-Null
$indexRows = @()
foreach ($month in ($byMonth.Keys | Sort-Object)) {
    if ($month -eq $newestMonth) { continue }
    $out = New-Object System.Collections.Generic.List[string]
    $out.Add("# LightSpeed phase log - $month")
    $out.Add('')
    $out.Add("> Archive of dated phase entries moved out of ``wat/state/current-phase.md``.")
    $out.Add("> Append-only: do not rewrite history; add a new entry in the current file instead.")
    $out.Add('')
    $out.Add('---')
    $out.Add('')
    foreach ($b in $byMonth[$month]) {
        $blk = New-Object System.Collections.Generic.List[string]
        foreach ($l in $b.Lines) { $blk.Add((Demote $l)) }
        while ($blk.Count -gt 0 -and $blk[$blk.Count - 1].Trim() -eq '') { $blk.RemoveAt($blk.Count - 1) }
        foreach ($l in $blk) { $out.Add($l) }
        $out.Add('')
    }
    $path = Join-Path $phasesDir "$month.md"
    [System.IO.File]::WriteAllText($path, (($out -join "`n").TrimEnd() + "`n"), [System.Text.UTF8Encoding]::new($false))
    $titles = $byMonth[$month] | Where-Object { $_.Level -eq 2 } | ForEach-Object { $_.Heading.TrimStart('#').Trim() }
    $indexRows += [pscustomobject]@{ Month = $month; File = "$month.md"; Entries = $titles.Count; Titles = $titles }
    Write-Host "wrote $path ($($out.Count) lines)"
}

# ---- 5. Rebuild current-phase.md -------------------------------------------
$date = Get-Date -Format 'yyyy-MM-dd'
$cur = New-Object System.Collections.Generic.List[string]
$cur.Add('# Current Phase')
$cur.Add('')
$cur.Add("**Updated:** $date")
$cur.Add('**History:** older entries live in `wat/state/phases/<YYYY-MM>.md`; this file keeps the live state and the newest entries only.')
$cur.Add('')
$statusBlock = $hb[0]
if ($statusBlock -and $statusBlock.Level -eq 1) {
    $cur.Add('## Current state')
    # the H1 heading is replaced by the new file title; its body is the live status
    $statusBody = New-Object System.Collections.Generic.List[string]
    foreach ($l in ($statusBlock.Lines | Select-Object -Skip 1)) { $statusBody.Add($l) }
    while ($statusBody.Count -gt 0 -and $statusBody[$statusBody.Count - 1].Trim() -eq '') { $statusBody.RemoveAt($statusBody.Count - 1) }
    foreach ($l in $statusBody) { $cur.Add($l) }
    $cur.Add('')
} else {
    $cur.Add('## Current state')
    $cur.Add('(no status block found during migration)')
    $cur.Add('')
}
$cur.Add('## Recent entries')
$cur.Add('')
foreach ($b in ($byMonth[$newestMonth] | Where-Object { $_.Level -eq 2 })) {
    $blk = New-Object System.Collections.Generic.List[string]
    foreach ($l in $b.Lines) { $blk.Add((Demote $l)) }
    while ($blk.Count -gt 0 -and $blk[$blk.Count - 1].Trim() -eq '') { $blk.RemoveAt($blk.Count - 1) }
    foreach ($l in $blk) { $cur.Add($l) }
    $cur.Add('')
}
$cur.Add('## History')
$cur.Add('')
$cur.Add('| Month | Entries | File |')
$cur.Add('|---|---|---|')
foreach ($row in $indexRows) { $cur.Add("| $($row.Month) | $($row.Entries) | [$($row.File)](phases/$($row.File)) |") }
$cur.Add('')
$cur.Add('---')
$cur.Add('')
$cur.Add('## How this file is maintained')
$cur.Add('')
$cur.Add('- One dated entry per completed round, appended at the top of **Recent entries**.')
$cur.Add('- When an entry no longer describes active work, move it verbatim to the month file in `wat/state/phases/` and demote its headings by one level.')
$cur.Add('- Decisions go to `wat/state/decisions/<YYYY-MM>.md`; evidence goes to `wat/evidence/<YYYY-MM-DD>/`.')
$cur.Add('- History is append-only. Nothing is deleted, only moved.')
$cur.Add('')
[System.IO.File]::WriteAllText($logPath, ($cur -join "`n"), [System.Text.UTF8Encoding]::new($false))
Write-Host "wrote $logPath"

# ---- 6. Index ---------------------------------------------------------------
$idx = New-Object System.Collections.Generic.List[string]
$idx.Add('# Phase history')
$idx.Add('')
$idx.Add('Monthly archives of `wat/state/current-phase.md`. Newest month last.')
$idx.Add('')
$idx.Add('| Month | Entries | File |')
$idx.Add('|---|---|---|')
foreach ($row in $indexRows) { $idx.Add("| $($row.Month) | $($row.Entries) | [$($row.File)]($($row.File)) |") }
$idx.Add('')
foreach ($row in $indexRows) {
    $idx.Add("## $($row.Month)")
    $idx.Add('')
    foreach ($t in $row.Titles) { $idx.Add("- $t") }
    $idx.Add('')
}
[System.IO.File]::WriteAllText((Join-Path $phasesDir 'README.md'), ($idx -join "`n"), [System.Text.UTF8Encoding]::new($false))
Write-Host "wrote $phasesDir\README.md"
