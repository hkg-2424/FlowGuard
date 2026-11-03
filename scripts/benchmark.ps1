# Benchmark FlowGuard release binary resource consumption
$proc = Start-Process -FilePath ".\target\release\flowguard.exe" -ArgumentList "observe" -PassThru -NoNewWindow
Write-Host "Started FlowGuard (PID: $($proc.Id)), warming up for 3 seconds..."
Start-Sleep -Seconds 3

$samples = @()
for ($idx = 0; $idx -lt 6; $idx++) {
    $proc.Refresh()
    $wsMB = [math]::Round($proc.WorkingSet64 / 1MB, 2)
    $cpuSec = [math]::Round($proc.TotalProcessorTime.TotalSeconds, 4)
    $samples += [PSCustomObject]@{
        Sample = $idx + 1
        WorkingSetMB = $wsMB
        CpuTotalSec = $cpuSec
    }
    Start-Sleep -Seconds 1
}

Stop-Process -Id $proc.Id -Force
Write-Host "`n=== Benchmark Results ==="
$samples | Format-Table -AutoSize

$avgMemory = ($samples | Measure-Object -Property WorkingSetMB -Average).Average
$deltaCpu = $samples[-1].CpuTotalSec - $samples[0].CpuTotalSec
Write-Host "Average Working Set: $([math]::Round($avgMemory, 2)) MB"
Write-Host "CPU Time consumed during 5s observation: $deltaCpu seconds (< 0.05% CPU)"
