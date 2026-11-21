param(
    [Parameter(Mandatory = $true)][string]$Binary,
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$Root = "."
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $false

$helpOutput = & $Binary --help 2>&1 | Out-String
if ($LASTEXITCODE -ne 0 -or $helpOutput -notmatch "USAGE:") {
    throw "release smoke: --help failed"
}

$versionOutput = (& $Binary --version 2>&1 | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or $versionOutput -ne "neolint $Version") {
    throw "release smoke: unexpected --version output: $versionOutput"
}

$valid = Join-Path $Root "corpus/small/vector_math.lsl"
$invalid = Join-Path $Root "corpus/invalid/duplicate_event.lsl"

& $Binary $valid | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "release smoke: valid corpus fixture failed"
}

$diagnosticOutput = (& $Binary $invalid 2>&1 | Out-String)
if ($LASTEXITCODE -ne 1 -or $diagnosticOutput -notmatch "SEM020") {
    throw "release smoke: invalid fixture did not report SEM020 with exit 1"
}

$jsonOutput = (& $Binary --format json $invalid 2>&1 | Out-String)
if ($LASTEXITCODE -ne 1) {
    throw "release smoke: JSON invocation did not exit 1"
}
$payload = @($jsonOutput | ConvertFrom-Json)
if ($payload.Count -lt 1) {
    throw "release smoke: expected a non-empty JSON array"
}
$first = $payload[0]
foreach ($field in @("file", "code", "severity", "kind", "message", "line", "column", "end_line", "end_column", "start", "end")) {
    if ($null -eq $first.PSObject.Properties[$field]) {
        throw "release smoke: missing JSON field $field"
    }
}
if ($first.code -ne "SEM020") {
    throw "release smoke: unexpected JSON diagnostic code $($first.code)"
}

Write-Output "release smoke: ok"
exit 0
