[CmdletBinding()]
param(
    [ValidateRange(8, 256)]
    [int]$SizeMiB = 256,

    [ValidateRange(1, 10)]
    [int]$Runs = 3,

    [string]$OutputPath = "benchmark-results/service.json"
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
try {
    cargo +1.91.1 build --locked --release --features service-benchmark --bin service_benchmark
    if ([System.OperatingSystem]::IsWindows()) {
        $binary = Join-Path $repoRoot "target/release/service_benchmark.exe"
    }
    else {
        $binary = Join-Path $repoRoot "target/release/service_benchmark"
    }
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
        throw "Release benchmark binary was not produced."
    }

    $results = [System.Collections.Generic.List[object]]::new()
    for ($run = 1; $run -le $Runs; $run++) {
        $tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("torrent-lite-benchmark-" + [guid]::NewGuid().ToString("N"))
        [System.IO.Directory]::CreateDirectory($tempRoot) | Out-Null
        $stdoutPath = Join-Path $tempRoot "stdout.json"
        $stderrPath = Join-Path $tempRoot "stderr.txt"
        $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
        $startInfo.FileName = $binary
        $startInfo.ArgumentList.Add("--size-mib")
        $startInfo.ArgumentList.Add([string]$SizeMiB)
        $startInfo.WorkingDirectory = $repoRoot
        $startInfo.UseShellExecute = $false
        $startInfo.CreateNoWindow = $true
        $startInfo.RedirectStandardOutput = $true
        $startInfo.RedirectStandardError = $true
        $startInfo.Environment["RUST_BACKTRACE"] = "0"
        $startInfo.StandardOutputEncoding = [System.Text.Encoding]::UTF8
        $startInfo.StandardErrorEncoding = [System.Text.Encoding]::UTF8

        $process = [System.Diagnostics.Process]::new()
        $process.StartInfo = $startInfo
        $samples = [System.Collections.Generic.List[object]]::new()
        $clock = [System.Diagnostics.Stopwatch]::StartNew()
        try {
            if (-not $process.Start()) {
                throw "Could not start the service benchmark process."
            }
            while (-not $process.HasExited) {
                $process.Refresh()
                $samples.Add([ordered]@{
                    elapsed_seconds = [math]::Round($clock.Elapsed.TotalSeconds, 3)
                    working_set_bytes = [long]$process.WorkingSet64
                    cpu_seconds = [math]::Round($process.TotalProcessorTime.TotalSeconds, 3)
                })
                if ($clock.Elapsed -gt [TimeSpan]::FromMinutes(10)) {
                    $process.Kill($true)
                    throw "Service benchmark exceeded the 10-minute limit."
                }
                Start-Sleep -Milliseconds 100
            }
            $process.WaitForExit()
            $clock.Stop()
            $stdout = $process.StandardOutput.ReadToEnd()
            $stderr = $process.StandardError.ReadToEnd()
            [System.IO.File]::WriteAllText($stdoutPath, $stdout, [System.Text.Encoding]::UTF8)
            [System.IO.File]::WriteAllText($stderrPath, $stderr, [System.Text.Encoding]::UTF8)
            if ($process.ExitCode -ne 0) {
                throw "Service benchmark run $run failed with exit code $($process.ExitCode): $stderr"
            }

            $report = $stdout | ConvertFrom-Json -AsHashtable
            if ($report.schema_version -ne 1 -or
                $report.transport -ne "tcp_loopback_through_policy_proxy" -or
                -not $report.download_verified -or
                $report.tracker_announces -lt 1 -or
                $report.proxy_tracker_connections -lt 1 -or
                $report.proxy_peer_connections -lt 1) {
                throw "Service benchmark run $run returned an invalid or unverified report."
            }
            $maxWorkingSet = ($samples | Measure-Object -Property working_set_bytes -Maximum).Maximum
            $results.Add([ordered]@{
                run = $run
                elapsed_seconds = [math]::Round($clock.Elapsed.TotalSeconds, 3)
                cpu_seconds = [math]::Round($process.TotalProcessorTime.TotalSeconds, 3)
                max_sampled_working_set_bytes = [long]$maxWorkingSet
                samples = @($samples)
                service_report = $report
            })
        }
        finally {
            if (-not $process.HasExited) {
                $process.Kill($true)
                $process.WaitForExit()
            }
            $process.Dispose()
            Remove-Item -LiteralPath $tempRoot -Recurse -Force
        }
    }

    $throughputs = @($results | ForEach-Object { [double]$_.service_report.transfer_mib_per_sec } | Sort-Object)
    $workingSets = @($results | ForEach-Object { [double]$_.max_sampled_working_set_bytes } | Sort-Object)
    $medianThroughput = if ($Runs % 2) {
        $throughputs[[int][math]::Floor($Runs / 2)]
    }
    else {
        ($throughputs[$Runs / 2 - 1] + $throughputs[$Runs / 2]) / 2
    }
    $medianWorkingSet = if ($Runs % 2) {
        $workingSets[[int][math]::Floor($Runs / 2)]
    }
    else {
        ($workingSets[$Runs / 2 - 1] + $workingSets[$Runs / 2]) / 2
    }
    $commit = (git rev-parse HEAD).Trim()
    $artifact = [ordered]@{
        schema_version = 1
        commit = $commit
        runner_os = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
        runner_image = $env:ImageOS
        generated_at_utc = [DateTimeOffset]::UtcNow.ToString("O")
        payload_bytes = $SizeMiB * 1024 * 1024
        runs = @($results)
        summary = [ordered]@{
            median_transfer_mib_per_sec = [math]::Round($medianThroughput, 3)
            median_max_sampled_working_set_bytes = [long]$medianWorkingSet
        }
    }

    $resolvedOutputPath = if ([System.IO.Path]::IsPathRooted($OutputPath)) {
        $OutputPath
    }
    else {
        Join-Path $repoRoot $OutputPath
    }
    $outputDirectory = Split-Path -Parent $resolvedOutputPath
    if ($outputDirectory) {
        [System.IO.Directory]::CreateDirectory($outputDirectory) | Out-Null
    }
    $json = $artifact | ConvertTo-Json -Depth 20
    [System.IO.File]::WriteAllText($resolvedOutputPath, $json + [Environment]::NewLine, [System.Text.Encoding]::UTF8)
    Write-Output "Wrote service benchmark results to $resolvedOutputPath"
}
finally {
    Pop-Location
}
