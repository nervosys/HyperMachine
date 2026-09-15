# The other half of the machine.
#
# Everything in this repository that only Windows can compile, which is more
# than it looks: `crates/hv2-core/src/backends/whpx.rs` is about five thousand
# lines that WSL cannot build at all, and its sixteen documented examples were
# fenced with ```ignore for exactly that reason -- so nothing checked them until
# somebody ran this. All sixteen were broken against the current API.
#
# Run it from the repository root, after tools/sweep.sh:
#
#   pwsh -File tools/sweep.ps1
#
# It deliberately uses the default `target/` rather than a scratch directory:
# the WSL sweep sets CARGO_TARGET_DIR elsewhere precisely so that these two do
# not invalidate each other's artifacts on every alternation.

$ErrorActionPreference = 'Continue'
$script:fail = 0

function Step($name) { Write-Host "`n=== $name" }
function Bad($why) { Write-Host "  FAILED: $why"; $script:fail = 1 }

# Sum every `test result` line rather than reading the tail. A run that is
# truncated shows no failures because it shows almost nothing, and that has
# been mistaken for a pass here more than once.
function Measure-Tests($output) {
    $lines = ($output -split "`n") | Where-Object { $_ -match '^test result' }
    $p = 0; $f = 0; $i = 0
    foreach ($l in $lines) {
        if ($l -match '(\d+) passed') { $p += [int]$Matches[1] }
        if ($l -match '(\d+) failed') { $f += [int]$Matches[1] }
        if ($l -match '(\d+) ignored') { $i += [int]$Matches[1] }
    }
    [pscustomobject]@{ Lines = $lines.Count; Passed = $p; Failed = $f; Ignored = $i }
}

if (-not (Test-Path 'crates/hv2-core/Cargo.toml')) {
    Write-Host 'Run this from the repository root.'
    exit 1
}

# hv2-api's build script needs protoc and there is no system one.
$protoc = (Get-Command protoc.exe -ErrorAction SilentlyContinue).Source
if ($protoc) { $env:PROTOC = $protoc }

# Counts measured on this tree. They are asserted rather than printed for the
# same reason the Linux sweep asserts 88: a figure that drifts quietly is how a
# run with whole crates missing once got reported as a pass.
$EXPECT_CORE_LINES = 21
$EXPECT_REST_LINES = 68

Step 'core (mirrors CI job "Test Core (Windows)")'
# This is the only place the WHPX backend is compiled, and `cargo test -p` runs
# doc-tests too -- which is what checks the sixteen whpx examples. Worth stating
# because the workspace's other doc-test step runs on the Linux job, where
# whpx.rs does not exist.
$out = & cargo test -p hv2-core 2>&1 | Out-String
$r = Measure-Tests $out
Write-Host "  $($r.Passed) passed, $($r.Failed) failed, $($r.Ignored) ignored, across $($r.Lines) result lines"
if ($out -notmatch 'Doc-tests hv2_core') { Bad 'no doc-tests ran, so the whpx examples went unchecked' }
if ($r.Failed -ne 0) { Bad "$($r.Failed) tests failed" }
if ($r.Lines -ne $EXPECT_CORE_LINES) { Bad "expected $EXPECT_CORE_LINES result lines, saw $($r.Lines)" }

Step 'the rest (mirrors CI job "Test (Windows)")'
$out = & cargo test --workspace --exclude hv2-core 2>&1 | Out-String
$r = Measure-Tests $out
Write-Host "  $($r.Passed) passed, $($r.Failed) failed, $($r.Ignored) ignored, across $($r.Lines) result lines"
if ($r.Failed -ne 0) { Bad "$($r.Failed) tests failed" }
if ($r.Lines -ne $EXPECT_REST_LINES) { Bad "expected $EXPECT_REST_LINES result lines, saw $($r.Lines)" }

Step 'clippy'
# CI does not lint this crate on Windows at all, so the whpx code path has no
# lint coverage anywhere else.
$out = & cargo clippy -p hv2-core --all-targets 2>&1 | Out-String
$n = (($out -split "`n") | Where-Object { $_ -match '^(warning|error)' }).Count
Write-Host "  hv2-core: $n"
if ($n -ne 0) { Bad "clippy reported $n" }

Step 'examples build'
# Including pic_timer_interrupts, which is Windows-gated and which CI never
# touches, because CI runs no examples on any platform.
$out = & cargo build -p hv2-core --examples 2>&1 | Out-String
if ($LASTEXITCODE -ne 0) { Bad 'examples did not build' } else { Write-Host '  ok' }

Step 'WHPX availability'
# Not a failure. The backend cannot run unless this optional feature is on, and
# a run that skips its integration test should say why rather than look clean.
$f = Get-CimInstance Win32_OptionalFeature -Filter "Name='HypervisorPlatform'" -ErrorAction SilentlyContinue
if ($f -and $f.InstallState -eq 1) {
    Write-Host '  HypervisorPlatform enabled -- `cargo test -p hv2-core -- --ignored` can run the WHPX test'
} else {
    Write-Host '  HypervisorPlatform DISABLED -- WhpxBackend::new() cannot succeed here.'
    Write-Host '  Everything above compiled the backend; nothing above ran it.'
}

if ($script:fail -eq 0) { Write-Host "`nWINDOWS SWEEP OK" } else { Write-Host "`nWINDOWS SWEEP FAILED" }
exit $script:fail
