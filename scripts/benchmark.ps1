param(
    [ValidateRange(8, 256)] [int] $SizeMiB = 256,
    [ValidateSet(1, 3, 5)] [int] $Runs = 3,
    [string] $OutputPath = 'benchmark-results/local.json'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
try {
    & cargo +1.91.1 build --locked --release --features durable-engine --example benchmark_local
    if ($LASTEXITCODE -ne 0) { throw 'Benchmark build failed.' }
    $binaryName = if ($IsWindows) { 'benchmark_local.exe' } else { 'benchmark_local' }
    $binaryPath = Join-Path $repoRoot "target/release/examples/$binaryName"
    if (-not (Test-Path -LiteralPath $binaryPath)) { throw 'Benchmark binary missing.' }
    $samples = [System.Collections.Generic.List[object]]::new()
    for ($run = 1; $run -le $Runs; $run++) {
        $stdoutPath = [System.IO.Path]::GetTempFileName()
        $stderrPath = [System.IO.Path]::GetTempFileName()
        try {
            $processArgs = @{
                FilePath = $binaryPath
                ArgumentList = @('--size-mib', [string]$SizeMiB)
                PassThru = $true
                RedirectStandardOutput = $stdoutPath
                RedirectStandardError = $stderrPath
            }
            if ($IsWindows) { $processArgs.WindowStyle = 'Hidden' }
            $benchmark = Start-Process @processArgs
            $peakWorkingSet = [long]0
            $cpuSeconds = [double]0
            while ($true) {
                $sample = Get-Process -Id $benchmark.Id -ErrorAction SilentlyContinue
                if ($null -ne $sample) {
                    $peakWorkingSet = [Math]::Max($peakWorkingSet, [long]$sample.WorkingSet64)
                    $cpuSeconds = [Math]::Max($cpuSeconds, [double]$sample.TotalProcessorTime.TotalSeconds)
                }
                if ($benchmark.HasExited) { break }
                Start-Sleep -Milliseconds 100
            }
            $benchmark.WaitForExit()
            if ($benchmark.ExitCode -ne 0) {
                throw "Benchmark run $run failed with exit code $($benchmark.ExitCode): $(Get-Content -LiteralPath $stderrPath -Raw)"
            }
            $measurement = Get-Content -LiteralPath $stdoutPath -Raw | ConvertFrom-Json
            if ($measurement.schema_version -ne 1 -or $measurement.payload_bytes -ne ($SizeMiB * 1048576)) {
                throw "Benchmark run $run returned an invalid result."
            }
            $samples.Add([pscustomobject]@{
                run = $run
                payload_bytes = [long]$measurement.payload_bytes
                transfer_seconds = [double]$measurement.transfer_seconds
                transfer_mib_per_sec = [double]$measurement.transfer_mib_per_sec
                process_cpu_seconds = $cpuSeconds
                sampled_peak_working_set_bytes = $peakWorkingSet
            })
        }
        finally {
            Remove-Item -LiteralPath $stdoutPath, $stderrPath -ErrorAction SilentlyContinue
        }
    }
    $bySpeed = @($samples | Sort-Object transfer_mib_per_sec)
    $byCpu = @($samples | Sort-Object process_cpu_seconds)
    $middle = [int][Math]::Floor($Runs / 2)
    $peak = [long]0
    foreach ($sample in $samples) {
        $peak = [Math]::Max($peak, [long]$sample.sampled_peak_working_set_bytes)
    }
    $revision = if ($env:GITHUB_SHA) { $env:GITHUB_SHA } else { (& git rev-parse HEAD).Trim() }
    $result = [ordered]@{
        schema_version = 1
        revision = $revision
        runner_os = $env:RUNNER_OS
        runner_image = $env:ImageOS
        powershell_os = $PSVersionTable.OS
        size_mib = $SizeMiB
        runs = $Runs
        transport = 'tcp_loopback_direct'
        storage = 'durable_sqlite_full_piece_recheck'
        sample_period_ms = 100
        samples = $samples.ToArray()
        median_transfer_mib_per_sec = [double]$bySpeed[$middle].transfer_mib_per_sec
        median_process_cpu_seconds = [double]$byCpu[$middle].process_cpu_seconds
        max_sampled_working_set_bytes = $peak
    }
    $json = $result | ConvertTo-Json -Depth 5
    $parent = Split-Path -Parent $OutputPath
    if ($parent) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
    Set-Content -LiteralPath $OutputPath -Value $json -Encoding utf8
    Write-Output $json
}
finally {
    Pop-Location
}
