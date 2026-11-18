param([Parameter(Mandatory)][string]$PlanFile)
$ErrorActionPreference = 'Stop'
$folder = Split-Path -Parent $PlanFile
$plan = Get-Content -LiteralPath $PlanFile -Raw -Encoding UTF8 | ConvertFrom-Json

function Assert-Packages {
    foreach ($package in $plan.packages) {
        $source = Join-Path $folder $package.name
        $sha256 = [Security.Cryptography.SHA256]::Create()
        try {
            $stream = [IO.File]::OpenRead($source)
            try { $hash = [BitConverter]::ToString($sha256.ComputeHash($stream)).Replace('-', '') }
            finally { $stream.Dispose() }
        } finally { $sha256.Dispose() }
        if ($hash -ne $package.sha256) {
            throw "Update package SHA-256 mismatch: $($package.name)"
        }
    }
}

function Install-Portable {
    $directory = Split-Path -Parent $plan.executable
    $nonce = [guid]::NewGuid().ToString('N')
    $files = foreach ($package in $plan.packages) {
        $target = if ($package.name -eq 'volume3-tray.exe') { $plan.executable } else {
            Join-Path $directory $package.name
        }
        @{
            Source = Join-Path $folder $package.name
            Target = $target
            Staged = "$target.$nonce.new"
            Backup = "$target.$nonce.bak"
            Replaced = $false
        }
    }
    try {
        foreach ($file in $files) {
            [IO.File]::Copy($file.Source, $file.Staged)
        }
        foreach ($file in $files) {
            if ([IO.File]::Exists($file.Target)) {
                [IO.File]::Replace($file.Staged, $file.Target, $file.Backup)
            } else {
                [IO.File]::Move($file.Staged, $file.Target)
            }
            $file.Replaced = $true
        }
    } catch {
        $failure = $_
        foreach ($file in $files) {
            if ([IO.File]::Exists($file.Backup)) {
                try {
                    [IO.File]::Copy($file.Backup, $file.Target, $true)
                    [IO.File]::Delete($file.Backup)
                } catch {
                    $failure = "$failure Rollback failed; original file saved at $($file.Backup). $_"
                }
            } elseif ($file.Replaced) {
                try { [IO.File]::Delete($file.Target) } catch { $failure = "$failure Rollback failed: $_" }
            }
        }
        throw $failure
    } finally {
        foreach ($file in $files) {
            Remove-Item -LiteralPath $file.Staged -ErrorAction SilentlyContinue
        }
    }
    foreach ($file in $files) {
        Remove-Item -LiteralPath $file.Backup -ErrorAction SilentlyContinue
    }
}

$parent = [Diagnostics.Process]::GetProcessById($plan.process_id)
$null = $parent.Handle
[IO.File]::WriteAllText((Join-Path $folder 'ready'), '')
$deadline = [DateTime]::UtcNow.AddSeconds(15)
while (-not (Test-Path -LiteralPath (Join-Path $folder 'go'))) {
    # The parent may exit right after writing the go file, so re-check it before giving up.
    if (($parent.HasExited -or [DateTime]::UtcNow -gt $deadline) -and
        -not (Test-Path -LiteralPath (Join-Path $folder 'go'))) { exit 1 }
    Start-Sleep -Milliseconds 25
}
$failure = $null
$parentClosed = $false
try {
    if (-not $parent.WaitForExit(30000)) { throw 'Volume3 did not close in time' }
    $parentClosed = $true
    Assert-Packages
    if ($plan.installed) {
        $msi = Join-Path $folder $plan.packages[0].name
        $installer = Start-Process -FilePath "$env:SystemRoot\System32\msiexec.exe" `
            -ArgumentList "/i `"$msi`" /qn /norestart" -Verb RunAs -Wait -PassThru
        if ($installer.ExitCode -notin @(0, 3010)) {
            throw "Windows Installer failed with exit code $($installer.ExitCode)"
        }
    } else {
        Install-Portable
    }
} catch {
    $failure = $_
}

if ($parentClosed) {
    try {
        if ($failure) { $env:VOLUME3_UPDATE_FAILED = '1' }
        else { Remove-Item Env:VOLUME3_UPDATE_FAILED -ErrorAction SilentlyContinue }
        # A Windows argument ending in a backslash must escape it before the closing quote.
        $configArg = $plan.config -replace '(\\+)$', '$1$1'
        Start-Process -FilePath $plan.executable -ArgumentList "--config `"$configArg`""
    } catch {
        $failure = "$failure Could not restart Volume3: $_"
    }
}
foreach ($name in @('ready', 'go', 'plan.json', 'install.ps1') + @($plan.packages.name)) {
    Remove-Item -LiteralPath (Join-Path $folder $name) -ErrorAction SilentlyContinue
}
try { [IO.Directory]::Delete($folder) } catch [IO.IOException] { }
if ($failure) {
    $shell = New-Object -ComObject WScript.Shell
    $null = $shell.Popup("Volume3 update failed: $failure", 30, 'Volume3', 16)
    exit 1
}
