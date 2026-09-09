$ErrorActionPreference = 'Continue'
Remove-Item Env:\CARGO_REGISTRY_TOKEN -ErrorAction SilentlyContinue
$m   = "C:\Users\adamm\dev\nervosys\os\AetherVM\Cargo.toml"
$log = "C:\Users\adamm\dev\nervosys\os\AetherVM\media\publish_remaining.log"

function Log($msg) {
  $ts = (Get-Date).ToUniversalTime().ToString("HH:mm:ss")
  Add-Content -Path $log -Value "[$ts] $msg"
}

# Dependency-ordered remaining NEW crates (1 per 10 min rate limit).
$order = @("hv2-gpu","hv2-net","hv2-agent","hv2-runtime","hv2-api","hv2-cli","hm-cli","hv1-arm","hm-gui","hypermachine")

Set-Content -Path $log -Value ("=== publish run started " + (Get-Date).ToUniversalTime().ToString("yyyy-MM-dd HH:mm:ss") + " UTC ===")

foreach ($c in $order) {
  $done = $false
  $attempt = 0
  while (-not $done) {
    $attempt++
    Log "publishing $c (attempt $attempt)"
    $out = cargo publish -p $c --manifest-path $m --no-verify 2>&1 | Out-String
    if ($out -match "Published $c") {
      Log "OK: $c published"
      $done = $true
    }
    elseif ($out -match "already (exists|uploaded)" -or $out -match "crate version .* is already uploaded") {
      Log "SKIP: $c already on registry"
      $done = $true
    }
    elseif ($out -match "429 Too Many Requests") {
      # Parse the 'try again after <RFC1123>' timestamp and sleep until then (+15s slack).
      $waitSec = 615
      if ($out -match "try again after ([^\.]+GMT)") {
        try {
          $reset = [datetime]::Parse($matches[1]).ToUniversalTime()
          $delta = ($reset - (Get-Date).ToUniversalTime()).TotalSeconds
          if ($delta -gt 0) { $waitSec = [int]$delta + 15 }
        } catch { }
      }
      Log "429 rate-limited on $c; sleeping $waitSec s"
      Start-Sleep -Seconds $waitSec
    }
    else {
      $tail = ($out -split "`n" | Select-Object -Last 6) -join " | "
      Log "ERROR on $c (attempt $attempt): $tail"
      if ($attempt -ge 4) { Log "GIVING UP on $c after $attempt attempts"; $done = $true }
      else { Start-Sleep -Seconds 60 }
    }
  }
  # Pace between successful new-crate publishes so we don't re-trip the burst limit.
  if ($c -ne $order[-1]) {
    Log "spacing 615s before next crate"
    Start-Sleep -Seconds 615
  }
}

Log "=== publish run complete ==="
