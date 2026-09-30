# Exercise failure accounting without compiling a benchmark or requiring Pester.
$ErrorActionPreference = 'Stop'
$converter = Join-Path $PSScriptRoot 'criterion-to-benchmark-json.ps1'
$root = Join-Path ([System.IO.Path]::GetTempPath()) ('hm-criterion-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $root | Out-Null
$output = Join-Path $root 'results.json'
$checks = 0

function New-Fixture([string]$case, $estimate, [string]$name = 'latency') {
    $directory = Join-Path $root $case
    $latest = Join-Path $directory 'measurement/new'
    New-Item -ItemType Directory -Force $latest | Out-Null
    @{ full_id = $name } | ConvertTo-Json | Set-Content (Join-Path $latest 'benchmark.json')
    if ($null -ne $estimate) {
        @{ mean = $estimate } | ConvertTo-Json | Set-Content (Join-Path $latest 'estimates.json')
    }
    return $directory
}

function Assert-Rejected([string]$directory) {
    $rejected = $false
    try { & $converter -CriterionDir $directory -OutFile $output }
    catch { $rejected = $true }
    if (-not $rejected) { throw "Invalid measurements were accepted: $directory" }
    $script:checks++
}

$valid = New-Fixture 'valid' @{ point_estimate = 12.3456; standard_error = 0.25 }
& $converter -CriterionDir $valid -OutFile $output
$results = @(Get-Content $output -Raw | ConvertFrom-Json)
if ($results.Count -ne 1 -or $results[0].name -ne 'latency' -or $results[0].value -ne 12.346 -or $results[0].unit -ne 'ns') {
    throw 'Valid measurement was not converted correctly'
}
$checks++

$multiple = New-Fixture 'multiple' @{ point_estimate = 2; standard_error = 0 } 'z'
$second = New-Fixture 'second' @{ point_estimate = 1; standard_error = 0 } 'a'
Copy-Item -LiteralPath (Join-Path $second 'measurement') -Destination (Join-Path $multiple 'other') -Recurse
& $converter -CriterionDir $multiple -OutFile $output
$results = @(Get-Content $output -Raw | ConvertFrom-Json)
if ($results.Count -ne 2 -or $results[0].name -ne 'a' -or $results[1].name -ne 'z') {
    throw 'Multiple measurements were not sorted or serialized as an array'
}
$checks++

Assert-Rejected (Join-Path $root 'missing')
$empty = Join-Path $root 'empty'
New-Item -ItemType Directory $empty | Out-Null
Assert-Rejected $empty
Assert-Rejected (New-Fixture 'no-estimates' $null)
Assert-Rejected (New-Fixture 'missing-mean' @{ standard_error = 1 })
Assert-Rejected (New-Fixture 'missing-error' @{ point_estimate = 1 })
Assert-Rejected (New-Fixture 'string' @{ point_estimate = '12'; standard_error = 1 })
Assert-Rejected (New-Fixture 'boolean' @{ point_estimate = $true; standard_error = 1 })
Assert-Rejected (New-Fixture 'negative' @{ point_estimate = -1; standard_error = 1 })
Assert-Rejected (New-Fixture 'zero' @{ point_estimate = 0; standard_error = 1 })
Assert-Rejected (New-Fixture 'negative-error' @{ point_estimate = 1; standard_error = -1 })
$nonfinite = New-Fixture 'nonfinite' @{ point_estimate = 1; standard_error = 0 }
'{"mean":{"point_estimate":1e999,"standard_error":0}}' | Set-Content (Join-Path $nonfinite 'measurement/new/estimates.json')
Assert-Rejected $nonfinite
$duplicate = New-Fixture 'duplicate' @{ point_estimate = 1; standard_error = 0 }
Copy-Item -LiteralPath (Join-Path $duplicate 'measurement') -Destination (Join-Path $duplicate 'other') -Recurse
Assert-Rejected $duplicate
$invalidName = New-Fixture 'invalid-name' @{ point_estimate = 1; standard_error = 0 }
'{"full_id":42}' | Set-Content (Join-Path $invalidName 'measurement/new/benchmark.json')
Assert-Rejected $invalidName
Write-Host "Passed $checks Criterion conversion checks."
