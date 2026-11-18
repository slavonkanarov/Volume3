$ErrorActionPreference = 'Stop'
$root = Join-Path $env:RUNNER_TEMP ("Volume3 update test " + [guid]::NewGuid().ToString('N'))
$stage = Join-Path $root 'stage'
$app = Join-Path $root 'app'
New-Item -Path $stage, $app -ItemType Directory -Force | Out-Null
$parent = $null
$helper = $null
try {
    $tray = Join-Path $app 'volume3-tray.exe'
    $cli = Join-Path $app 'volume3.exe'
    $config = Join-Path $app 'custom config.json'
    $restarted = "$config.restarted"
    $source = Join-Path $root 'fixture.rs'
    @'
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 || args[1] != "--config" {
        std::thread::sleep(std::time::Duration::from_secs(60));
        return;
    }
    let mut marker = args[2].clone();
    marker.push(if cfg!(old_tray) { ".old" } else { ".restarted" });
    std::fs::write(marker, b"restarted").unwrap();
}
'@ | Set-Content -LiteralPath $source -Encoding ascii
    & rustc $source -o (Join-Path $stage 'volume3-tray.exe')
    if ($LASTEXITCODE -ne 0) { throw 'Failed to build update fixture' }
    & rustc $source --cfg old_tray -o $tray
    if ($LASTEXITCODE -ne 0) { throw 'Failed to build old tray fixture' }
    Set-Content -LiteralPath $cli -Value 'old CLI'
    Set-Content -LiteralPath $config -Value '{"keep":true}'
    Set-Content -LiteralPath (Join-Path $stage 'volume3.exe') -Value 'new CLI'
    $packages = foreach ($name in @('volume3-tray.exe', 'volume3.exe')) {
        @{ name = $name; sha256 = (Get-FileHash -LiteralPath (Join-Path $stage $name)).Hash }
    }
    $parent = Start-Process -FilePath $tray -WindowStyle Hidden -PassThru
    $plan = Join-Path $stage 'plan.json'
    @{ executable = $tray; config = $config; process_id = $parent.Id; installed = $false; packages = @($packages) } |
        ConvertTo-Json -Depth 3 | Set-Content -LiteralPath $plan -Encoding utf8
    $script = Join-Path $stage 'install.ps1'
    Copy-Item -LiteralPath "$PSScriptRoot/../src/updates/install.ps1" -Destination $script
    $helper = Start-Process -FilePath "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" `
        -ArgumentList "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File `"$script`" `"$plan`"" `
        -WindowStyle Hidden -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while (-not (Test-Path -LiteralPath (Join-Path $stage 'ready'))) {
        if ($helper.HasExited -or [DateTime]::UtcNow -gt $deadline) { throw 'Update helper not ready' }
        Start-Sleep -Milliseconds 50
    }
    [IO.File]::WriteAllText((Join-Path $stage 'go'), '')
    $parent.Kill()
    if (-not $helper.WaitForExit(45000) -or $helper.ExitCode -ne 0) { throw 'Update helper failed' }
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    while (-not (Test-Path -LiteralPath $restarted)) {
        if ([DateTime]::UtcNow -gt $deadline) { throw 'Tray did not restart with the custom config path' }
        Start-Sleep -Milliseconds 50
    }
    if ((Get-Content -LiteralPath $cli -Raw).Trim() -ne 'new CLI') { throw 'CLI not updated' }
    if ((Get-Content -LiteralPath $config -Raw).Trim() -ne '{"keep":true}') { throw 'Configuration was changed' }
} finally {
    foreach ($process in @($parent, $helper)) {
        if ($process -and -not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
    }
    Remove-Item -LiteralPath $root -Recurse -Force
}
