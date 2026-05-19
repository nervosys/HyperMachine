# benchmarks/harness/adapters/hyper_v.ps1
# Adapter: Microsoft Hyper-V (Windows). PowerShell, invoked by run.ps1.
#
# Verb on $args[0], remaining args after. Honors the same ABI as the *.sh
# adapters (see ../lib/adapter.sh).

[CmdletBinding()] param([Parameter(Position = 0, Mandatory = $true)][string]$Verb)

$ErrorActionPreference = 'Stop'

$StateRoot = if ($env:BENCH_STATE_ROOT) { Join-Path $env:BENCH_STATE_ROOT 'hyper_v' } else { Join-Path (Get-Location) '.state\hyper_v' }
New-Item -ItemType Directory -Force -Path $StateRoot | Out-Null

function VmDir([string]$id) { Join-Path $StateRoot $id }

function Get-FreePort {
    $l = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $l.Start(); $p = $l.LocalEndpoint.Port; $l.Stop(); return $p
}

switch ($Verb) {
    'setup' {
        $image = $args[1]; $cpus = [int]$args[2]; $mem = [int64]$args[3]; $disk = $args[4]; $netif = $args[5]
        $vmId = "hv-$(Get-Date -UFormat %s)-$PID"
        $d = VmDir $vmId; New-Item -ItemType Directory -Force -Path $d | Out-Null
        # Hyper-V wants VHDX. Convert if needed.
        $vhdx = Join-Path $d 'disk.vhdx'
        if ($image -match '\.qcow2$') {
            if (Get-Command qemu-img -ErrorAction SilentlyContinue) {
                & qemu-img convert -O vhdx $image $vhdx | Out-Null
            }
            else { throw "qcow2 input requires qemu-img on PATH" }
        }
        elseif ($image -match '\.vhdx?$') {
            Copy-Item $image $vhdx
        }
        else { throw "unsupported image format: $image" }

        New-VM -Name $vmId -MemoryStartupBytes ($mem * 1MB) -Generation 2 -VHDPath $vhdx -SwitchName 'Default Switch' | Out-Null
        Set-VM  -Name $vmId -ProcessorCount $cpus -AutomaticStartAction Nothing -AutomaticStopAction TurnOff
        Set-VMFirmware -VMName $vmId -EnableSecureBoot Off
        # Hyper-V "Default Switch" gives the guest a routable IP via internal NAT;
        # SSH is reached via the guest's IP rather than a hostfwd. We discover it in wait_ssh.
        $sshPort = 22
        @"
cpus=$cpus
mem=$mem
netif=$netif
ssh_port=$sshPort
"@ | Set-Content -Path (Join-Path $d 'meta')
        Write-Output $vmId
    }
    'start' {
        $vmId = $args[1]
        Start-VM -Name $vmId | Out-Null
        # Pid of the worker process (vmwp.exe) for this VM:
        $guid = (Get-VM -Name $vmId).Id.Guid
        $proc = Get-WmiObject Win32_Process -Filter "Name='vmwp.exe'" | Where-Object { $_.CommandLine -match $guid } | Select-Object -First 1
        if ($proc) { Write-Output $proc.ProcessId } else { Write-Output 0 }
    }
    'wait_ssh' {
        $vmId = $args[1]; $port = [int]$args[2]; $timeout = [int]$args[3]
        $start = Get-Date
        while (((Get-Date) - $start).TotalSeconds -lt $timeout) {
            $ip = (Get-VMNetworkAdapter -VMName $vmId).IPAddresses | Where-Object { $_ -match '^\d+\.\d+\.\d+\.\d+$' } | Select-Object -First 1
            if ($ip) {
                # Try TCP connect to SSH.
                try {
                    $c = [System.Net.Sockets.TcpClient]::new()
                    $iar = $c.BeginConnect($ip, $port, $null, $null)
                    if ($iar.AsyncWaitHandle.WaitOne(500) -and $c.Connected) {
                        $c.EndConnect($iar); $c.Close()
                        $elapsed = ((Get-Date) - $start).TotalSeconds
                        Write-Output ('{0:F3}' -f $elapsed)
                        exit 0
                    }
                    $c.Close()
                }
                catch {}
            }
            Start-Sleep -Milliseconds 250
        }
        exit 124
    }
    'snapshot' {
        $vmId = $args[1]; $name = $args[2]
        Checkpoint-VM -Name $vmId -SnapshotName $name | Out-Null
    }
    'restore' {
        $vmId = $args[1]; $name = $args[2]
        $snap = Get-VMSnapshot -VMName $vmId -Name $name
        Restore-VMSnapshot -VMSnapshot $snap -Confirm:$false | Out-Null
        Start-VM -Name $vmId | Out-Null
    }
    'stop' {
        $vmId = $args[1]
        try { Stop-VM -Name $vmId -Force -TurnOff | Out-Null } catch {}
    }
    'destroy' {
        $vmId = $args[1]; $d = VmDir $vmId
        try { Remove-VM -Name $vmId -Force | Out-Null } catch {}
        if (Test-Path $d) { Remove-Item -Recurse -Force $d }
    }
    'metrics' {
        $vmId = $args[1]
        try {
            $vm = Get-VM -Name $vmId
            $rssKiB = [int64]($vm.MemoryAssigned / 1KB)
            $cpu = $vm.CPUUsage
            Write-Output ("rss_kib={0},cpu_pct={1}" -f $rssKiB, $cpu)
        }
        catch {
            Write-Output "rss_kib=0,cpu_pct=0"
        }
    }
    default { Write-Error "unknown verb $Verb"; exit 2 }
}
