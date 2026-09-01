<#
.SYNOPSIS
  Convert Criterion's per-benchmark JSON output into the flat JSON array
  format that benchmark-action/github-action-benchmark understands natively
  via tool: 'customSmallerIsBetter' (see its README, "Other" tool section).

.WHY
  criterion's harness (harness = false benches) never emits libtest's
  `test <name> ... bench: <n> ns/iter (+/- <e>)` line, not even with
  `--output-format bencher` (that format omits the leading `test <name>`
  token entirely, e.g. `bench: 363 ns/iter (+/- 7)`). tool: 'cargo' in this
  action greps for the libtest line and therefore always finds zero
  benchmarks. Criterion instead writes structured JSON per benchmark under
  target/criterion/<bench>/new/{benchmark,estimates}.json on every run, and
  that JSON is what this script reads. No harness change and no reliance on
  any tool's stdout formatting.

.PARAMETER CriterionDir
  Root of criterion's output tree (default: target/criterion).

.PARAMETER OutFile
  Where to write the resulting JSON array.
#>
param(
    [string]$CriterionDir = "target/criterion",
    [Parameter(Mandatory = $true)]
    [string]$OutFile
)

$ErrorActionPreference = "Stop"

# Write without a BOM, and without relying on -Encoding utf8NoBOM (only
# available on PowerShell 7+; this script may also be run locally under
# Windows PowerShell 5.1).
function Write-Utf8NoBom([string]$Path, [string]$Content) {
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($Path, $Content, $utf8NoBom)
}

if (-not (Test-Path $CriterionDir)) {
    Write-Warning "Criterion output directory '$CriterionDir' does not exist; writing empty results."
    Write-Utf8NoBom -Path $OutFile -Content "[]"
    exit 0
}

# Every completed benchmark leaves its latest run's data under a `new/`
# directory (criterion's default, used whenever --baseline isn't passed,
# which is how this workflow invokes cargo bench).
$benchmarkFiles = Get-ChildItem -Path $CriterionDir -Recurse -Filter "benchmark.json" |
    Where-Object { $_.Directory.Name -eq "new" }

$results = @()

foreach ($bf in $benchmarkFiles) {
    $estimatesPath = Join-Path $bf.Directory.FullName "estimates.json"
    if (-not (Test-Path $estimatesPath)) {
        Write-Warning "No estimates.json next to $($bf.FullName); skipping."
        continue
    }

    $benchmark = Get-Content $bf.FullName -Raw | ConvertFrom-Json
    $estimates = Get-Content $estimatesPath -Raw | ConvertFrom-Json

    $name = $benchmark.full_id
    if (-not $name) { $name = $benchmark.title }
    if (-not $name) { $name = $bf.Directory.Parent.Name }

    $meanNs = $estimates.mean.point_estimate
    $stdErrNs = $estimates.mean.standard_error

    if ($null -eq $meanNs) {
        Write-Warning "No mean.point_estimate in $estimatesPath; skipping."
        continue
    }

    $results += [ordered]@{
        name  = $name
        unit  = "ns"
        value = [math]::Round([double]$meanNs, 3)
        range = "+/- $([math]::Round([double]$stdErrNs, 3))"
    }
}

$results = $results | Sort-Object { $_.name }

$json = ConvertTo-Json -InputObject @($results) -Depth 5
Write-Utf8NoBom -Path $OutFile -Content $json

Write-Host "Wrote $($results.Count) benchmark result(s) to $OutFile"
