# benchmarks/harness/run.ps1 — Windows orchestrator.
# Subset of run.sh — supports Hyper-V, HyperMachine (WHPX), VirtualBox.
# Workloads that require Linux host tools (iperf3/netperf as server, qemu-img
# for image conversion) need those tools on PATH.

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Config,
    [Parameter(Mandatory = $true)][string]$Out,
    [string[]]$OnlyHv = @(),
    [string[]]$OnlyWl = @()
)

$ErrorActionPreference = 'Stop'
$HarnessDir = Split-Path -Parent $PSCommandPath
$BenchDir = Split-Path -Parent $HarnessDir
$AdaptersDir = Join-Path $HarnessDir 'adapters'
$WorkloadsDir = Join-Path $HarnessDir 'workloads'

New-Item -ItemType Directory -Force -Path $Out | Out-Null
Copy-Item $Config (Join-Path $Out 'config.snapshot.toml') -Force

# Capture environment.
$env:BENCH_STATE_ROOT = Join-Path $Out '.state'
$envObj = @{
    os      = (Get-CimInstance Win32_OperatingSystem).Caption
    kernel  = (Get-CimInstance Win32_OperatingSystem).Version
    cpu     = (Get-CimInstance Win32_Processor).Name
    mem_kib = [int64]((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1KB)
    tools   = @{}
}
foreach ($t in @('VBoxManage', 'qemu-img', 'hm-cli', 'python', 'bash')) {
    $c = Get-Command $t -ErrorAction SilentlyContinue
    $envObj.tools[$t] = if ($c) { $c.Source } else { $null }
}
$envObj.hyperv = (Get-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V -ErrorAction SilentlyContinue).State
$envObj | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $Out 'env.json')

# Parse config via Python (consistent with Linux side).
function Toml-Get([string]$key) {
    & python -c @"
import sys, tomllib
with open(r'$Config','rb') as f: d = tomllib.load(f)
cur = d
for p in '$key'.split('.'):
    cur = cur.get(p) if isinstance(cur, dict) else None
    if cur is None: break
if isinstance(cur, list):
    print(' '.join(str(x) for x in cur))
elif isinstance(cur, bool):
    print('true' if cur else 'false')
elif cur is not None:
    print(cur)
"@
}

function Toml-EnabledKeys([string]$table) {
    & python -c @"
import sys, tomllib
with open(r'$Config','rb') as f: d = tomllib.load(f)
for k,v in d.get('$table',{}).items():
    if isinstance(v,dict) and v.get('enabled'):
        print(k)
"@
}

$Samples = [int](Toml-Get 'run.samples')
$Cooldown = [int](Toml-Get 'run.cooldown_sec')
$Image = Toml-Get 'guest.image'
if (-not [System.IO.Path]::IsPathRooted($Image)) { $Image = Join-Path $BenchDir $Image }
$SshUser = Toml-Get 'guest.ssh_user'
$SshKey = Toml-Get 'guest.ssh_key'
if (-not [System.IO.Path]::IsPathRooted($SshKey)) { $SshKey = Join-Path $BenchDir $SshKey }
$Cpus = [int](Toml-Get 'guest.cpus')
$Mem = [int](Toml-Get 'guest.mem_mib')
$Netif = Toml-Get 'guest.netif'

$Hvs = Toml-EnabledKeys 'hypervisor' | Where-Object { $_ }
$Wls = Toml-EnabledKeys 'workload'   | Where-Object { $_ }

if ($OnlyHv.Count) { $Hvs = $Hvs | Where-Object { $OnlyHv -contains $_ } }
if ($OnlyWl.Count) { $Wls = $Wls | Where-Object { $OnlyWl -contains $_ } }

function Invoke-Adapter([string]$hv, [string]$verb, [object[]]$args) {
    if ($hv -eq 'hyper_v') {
        & powershell -NoProfile -File (Join-Path $AdaptersDir 'hyper_v.ps1') $verb @args
    }
    else {
        # Use git-bash / wsl bash. Caller must ensure it's on PATH.
        $script = Join-Path $AdaptersDir "$hv.sh"
        $bash = (Get-Command bash -ErrorAction SilentlyContinue).Source
        if (-not $bash) { throw "bash required for adapter '$hv'" }
        & $bash $script $verb @args
    }
}

foreach ($hv in $Hvs) {
    Write-Host "==> hypervisor: $hv"
    $hvOut = Join-Path $Out $hv
    New-Item -ItemType Directory -Force -Path $hvOut | Out-Null

    foreach ($wl in $Wls) {
        Write-Host "  -- workload: $wl"
        $wlDir = Join-Path $hvOut $wl
        $rawDir = Join-Path $wlDir 'raw'
        New-Item -ItemType Directory -Force -Path $rawDir | Out-Null

        switch ($wl) {
            'boot_cold' {
                for ($i = 0; $i -le $Samples; $i++) {
                    $vm = Invoke-Adapter $hv 'setup' @($Image, $Cpus, $Mem, '', $Netif)
                    [void](Invoke-Adapter $hv 'start' @($vm))
                    $port = 22
                    $elapsed = Invoke-Adapter $hv 'wait_ssh' @($vm, $port, 120)
                    if ($LASTEXITCODE -eq 0 -and $i -gt 0) {
                        Add-Content (Join-Path $rawDir 'boot_cold_seconds.raw') $elapsed
                        's' | Set-Content (Join-Path $rawDir 'boot_cold_seconds.unit')
                    }
                    [void](Invoke-Adapter $hv 'stop'    @($vm))
                    [void](Invoke-Adapter $hv 'destroy' @($vm))
                    Start-Sleep -Seconds $Cooldown
                }
            }
            default {
                Write-Warning "  workload $wl: full execution requires Linux host bash; run on Linux side."
            }
        }

        # Summarize.
        Get-ChildItem -Path $rawDir -Filter *.raw -ErrorAction SilentlyContinue | ForEach-Object {
            $metric = $_.BaseName
            & python -c @"
import csv, statistics
xs = [float(l.strip()) for l in open(r'$($_.FullName)') if l.strip()]
if not xs: raise SystemExit(0)
xs.sort()
def pct(p): return xs[max(0,min(len(xs)-1,int(round(p/100*(len(xs)-1)))))]
import sys
with open(r'$(Join-Path $wlDir ($metric + ".summary.csv"))','w',newline='') as f:
    w = csv.writer(f)
    w.writerow(['n','min','p50','p95','p99','max','mean','stdev'])
    w.writerow([len(xs), xs[0], pct(50), pct(95), pct(99), xs[-1],
                statistics.fmean(xs), statistics.pstdev(xs) if len(xs)>1 else 0.0])
"@
        }
    }
}

Write-Host "run complete: $Out"
