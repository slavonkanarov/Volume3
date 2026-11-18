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
    if ((Get-Content -LiteralPath "$config.before-update" -Raw).Trim() -ne '{"keep":true}') { throw 'Configuration backup is missing' }

    # Exercise the same helper functions without launching UAC or changing installed files.
    $tokens = $null
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseFile(
        (Join-Path $PSScriptRoot '../src/updates/install.ps1'), [ref]$tokens, [ref]$errors)
    if ($errors) { throw ($errors | Out-String) }
    foreach ($function in $ast.FindAll({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] }, $false)) {
        . ([scriptblock]::Create($function.Extent.Text))
    }
    $folder = Join-Path $root 'packages'
    $atomic = Join-Path $root 'atomic'
    New-Item -ItemType Directory -Path $folder, $atomic | Out-Null
    $packages = foreach ($name in @('volume3-tray.exe', 'volume3.exe')) {
        Set-Content -LiteralPath (Join-Path $folder $name) -Value "new $name"
        Set-Content -LiteralPath (Join-Path $atomic $name) -Value "old $name"
        @{ name = $name; sha256 = (Get-FileHash -LiteralPath (Join-Path $folder $name)).Hash }
    }
    $plan = @{ executable = Join-Path $atomic 'volume3-tray.exe'; packages = @($packages); recovery = $null }
    Assert-Packages
    $plan.packages[0].sha256 = 'bad checksum'
    $rejected = $false
    try { Assert-Packages } catch { $rejected = $true }
    if (-not $rejected) { throw 'A corrupt package was accepted' }

    $locked = [IO.File]::Open((Join-Path $atomic 'volume3.exe'), 'Open', 'ReadWrite', 'None')
    $rolledBack = $false
    try { Install-Portable } catch { $rolledBack = $true } finally { $locked.Dispose() }
    if (-not $rolledBack) { throw 'Replacing a locked CLI should fail' }
    foreach ($name in @('volume3-tray.exe', 'volume3.exe')) {
        if ((Get-Content -LiteralPath (Join-Path $atomic $name) -Raw).Trim() -ne "old $name") {
            throw "Rollback did not preserve $name"
        }
    }
    if (Get-ChildItem -LiteralPath $atomic -Filter '*.new') { throw 'Staged files were not cleaned up' }

    # Reproduce the wrapped access-denied error from .NET, including the helper's rethrow.
    $readonly = Join-Path $atomic 'volume3-tray.exe'
    [IO.File]::SetAttributes($readonly, [IO.FileAttributes]::ReadOnly)
    function Install-Portable {
        try { [IO.File]::Copy((Join-Path $folder 'volume3-tray.exe'), $readonly, $true) }
        catch { $failure = $_; throw $failure }
    }
    $script:elevations = 0
    function Invoke-ElevatedUpdate { $script:elevations++ }
    try { Install-Binaries } finally { [IO.File]::SetAttributes($readonly, [IO.FileAttributes]::Normal) }
    if ($script:elevations -ne 1) { throw 'Wrapped access-denied error did not request elevation exactly once' }
    function Install-Portable { throw [IO.IOException]::new('File is locked') }
    $rejected = $false
    try { Install-Binaries } catch { $rejected = $true }
    if (-not $rejected -or $script:elevations -ne 1) { throw 'A file-lock failure incorrectly requested elevation' }
    Write-Host 'Portable replacement, restart, backup, checksum rejection, rollback, and access-denied elevation passed.'
} finally {
    foreach ($process in @($parent, $helper)) {
        if ($process -and -not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
    }
    $testParent = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\') + '\'
    if (-not [IO.Path]::GetFullPath($root).StartsWith($testParent, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to remove a test directory outside $testParent"
    }
    Remove-Item -LiteralPath $root -Recurse -Force
}
