#requires -Version 7.0

[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string] $Executable,

    [Parameter(Mandatory)]
    [string] $DataRoot,

    [Parameter(Mandatory)]
    [string] $ExpectedVersion,

    [Parameter(Mandatory)]
    [string] $EvidenceFile,

    [ValidateRange(1000, 120000)]
    [int] $TimeoutMs = 30000
)

$ErrorActionPreference = 'Stop'

$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
if ([IO.Path]::GetExtension($resolvedExecutable) -ne '.exe') {
    throw 'The installed protocol smoke requires a Windows executable.'
}

New-Item -ItemType Directory -Force -Path $DataRoot | Out-Null
$resolvedDataRoot = (Resolve-Path -LiteralPath $DataRoot).Path
$dataRootItem = Get-Item -LiteralPath $resolvedDataRoot -Force
if (($dataRootItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw 'The protocol smoke data root cannot be a reparse point.'
}

$capabilityPath = Join-Path $resolvedDataRoot '.deltamod-capability-evidence.json'
$queuePath = Join-Path $resolvedDataRoot '.deltamod-protocol-queue-evidence.json'
$protocolPath = Join-Path $resolvedDataRoot '.deltamod-protocol-evidence.json'
if ((Test-Path -LiteralPath $capabilityPath) -or (Test-Path -LiteralPath $queuePath) -or (Test-Path -LiteralPath $protocolPath)) {
    throw 'The protocol smoke requires a fresh disposable data root.'
}

$evidencePath = [IO.Path]::GetFullPath($EvidenceFile)
$evidenceParent = Split-Path -Parent $evidencePath
if ($evidenceParent) {
    New-Item -ItemType Directory -Force -Path $evidenceParent | Out-Null
}

function Wait-ForJsonFile {
    param(
        [Parameter(Mandatory)]
        [string] $Path,

        [Parameter(Mandatory)]
        [Diagnostics.Process] $Application,

        [Parameter(Mandatory)]
        [Diagnostics.Stopwatch] $Clock,

        [Parameter(Mandatory)]
        [int] $LimitMs
    )

    while ($Clock.ElapsedMilliseconds -lt $LimitMs) {
        if ($Application.HasExited) {
            throw "The installed application exited early with code $($Application.ExitCode)."
        }
        if (Test-Path -LiteralPath $Path -PathType Leaf) {
            return Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
        }
        Start-Sleep -Milliseconds 100
    }
    throw "Timed out waiting for $(Split-Path -Leaf $Path)."
}

$startInfo = [Diagnostics.ProcessStartInfo]::new()
$startInfo.FileName = $resolvedExecutable
$startInfo.UseShellExecute = $false
$startInfo.ArgumentList.Add('--data-root')
$startInfo.ArgumentList.Add($resolvedDataRoot)
$startInfo.Environment['DELTAMOD_SMOKE_DATA_ROOT'] = $resolvedDataRoot
$startInfo.Environment['DELTAMOD_SMOKE_CAPABILITY_FILE'] = $capabilityPath
$startInfo.Environment['DELTAMOD_SMOKE_PROTOCOL_FILE'] = $protocolPath

$application = $null
try {
    $application = [Diagnostics.Process]::Start($startInfo)
    if (-not $application) {
        throw 'The installed application could not be started.'
    }

    $clock = [Diagnostics.Stopwatch]::StartNew()
    $capability = Wait-ForJsonFile -Path $capabilityPath -Application $application -Clock $clock -LimitMs $TimeoutMs
    if (-not $capability.ok -or $capability.packageVersion -ne $ExpectedVersion) {
        throw 'The installed capability probe did not pass with the expected version.'
    }
    # Page-load evidence is written before the renderer finishes registering
    # its four protocol listeners. Keep the process alive for the same bounded
    # readiness window used by the packaged capability smoke.
    Start-Sleep -Milliseconds 1500

    $registryPath = 'Registry::HKEY_CURRENT_USER\Software\Classes\deltamod-community\shell\open\command'
    if (-not (Test-Path -LiteralPath $registryPath)) {
        throw 'The NSIS package did not register the deltamod-community protocol.'
    }
    $registeredCommand = (Get-Item -LiteralPath $registryPath).GetValue('')
    if ($registeredCommand -notmatch '^"(?<executable>[^"]+)"') {
        throw 'The protocol registration command is malformed.'
    }
    $registeredExecutable = (Resolve-Path -LiteralPath $Matches.executable).Path
    if (-not [string]::Equals($registeredExecutable, $resolvedExecutable, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'The protocol registration does not target the installed Tauri executable.'
    }

    $protocolUri = 'deltamod-community://gb/launch?item=12'
    $handoffInfo = [Diagnostics.ProcessStartInfo]::new()
    $handoffInfo.FileName = $registeredExecutable
    $handoffInfo.UseShellExecute = $false
    $handoffInfo.ArgumentList.Add($protocolUri)
    $handoff = [Diagnostics.Process]::Start($handoffInfo)
    if (-not $handoff) {
        throw 'The registered protocol handoff process could not be started.'
    }
    $queueClock = [Diagnostics.Stopwatch]::StartNew()
    $queued = Wait-ForJsonFile -Path $queuePath -Application $application -Clock $queueClock -LimitMs $TimeoutMs
    $protocolClock = [Diagnostics.Stopwatch]::StartNew()
    $protocol = Wait-ForJsonFile -Path $protocolPath -Application $application -Clock $protocolClock -LimitMs $TimeoutMs

    $checks = [ordered]@{
        registeredToInstalledExecutable = $true
        queuedInFirstProcess = ([uint32]$queued.processId -eq [uint32]$application.Id)
        forwardedToFirstProcess = ([uint32]$protocol.processId -eq [uint32]$application.Id)
        rendererReady = ($protocol.checks.rendererReady -eq $true)
        strictProtocolAction = ($protocol.checks.strictProtocolAction -eq $true)
        expectedAction = ($protocol.action -eq 'launch' -and [uint64]$protocol.itemId -eq 12)
        firstProcessStillRunning = (-not $application.HasExited)
    }
    if ($checks.Values -contains $false) {
        throw 'The installed protocol smoke returned a failed check.'
    }

    [ordered]@{
        schemaVersion = 1
        status = 'passed'
        ok = $true
        packageVersion = $protocol.packageVersion
        platform = 'windows'
        checks = $checks
        operationId = $protocol.operationId
        rendererGeneration = $protocol.rendererGeneration
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $evidencePath -Encoding utf8
}
finally {
    if ($application -and -not $application.HasExited) {
        Stop-Process -Id $application.Id -Force
        $application.WaitForExit(10000) | Out-Null
    }
}
