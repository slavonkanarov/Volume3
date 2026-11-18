param([Parameter(Mandatory)][string]$PlanFile, [switch]$Elevated)
$ErrorActionPreference = 'Stop'
$folder = Split-Path -Parent $PlanFile
$plan = Get-Content -LiteralPath $PlanFile -Raw -Encoding UTF8 | ConvertFrom-Json

function Assert-Packages {
    foreach ($package in @($plan.packages) + @($plan.recovery | Where-Object { $_ })) {
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

function Invoke-Installer([string]$Arguments) {
    $installer = Start-Process -FilePath "$env:SystemRoot\System32\msiexec.exe" `
        -ArgumentList "$Arguments /qn /norestart" -WindowStyle Hidden -Wait -PassThru
    if ($installer.ExitCode -notin @(0, 3010)) {
        throw "Windows Installer failed with exit code $($installer.ExitCode)"
    }
}

function Invoke-ElevatedUpdate {
    $arguments = "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File `"$PSCommandPath`" -PlanFile `"$PlanFile`" -Elevated"
    $installer = Start-Process -FilePath "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" `
        -ArgumentList $arguments -Verb RunAs -WindowStyle Hidden -Wait -PassThru
    if ($installer.ExitCode -ne 0) {
        $errorFile = Join-Path $folder 'error.txt'
        if (Test-Path -LiteralPath $errorFile) { throw (Get-Content -LiteralPath $errorFile -Raw) }
        throw "Elevated installation failed with exit code $($installer.ExitCode)"
    }
}

function Install-Binaries {
    if ($plan.installed) {
        Invoke-ElevatedUpdate
        return
    }
    try {
        Install-Portable
    } catch {
        # PowerShell wraps .NET calls, and rethrowing an ErrorRecord keeps that wrapper.
        $exception = $_.Exception
        while ($exception -and $exception -isnot [System.UnauthorizedAccessException]) {
            $exception = $exception.InnerException
        }
        if (-not $exception) { throw }
        Invoke-ElevatedUpdate
    }
}

if ($Elevated) {
    try {
        Assert-Packages
        if ($plan.installed) {
            if ($plan.product -notmatch '^\{[0-9A-Fa-f-]{36}\}$') { throw 'Invalid MSI product code' }
            if ($plan.recovery) { Invoke-Installer "/x $($plan.product)" }
            try {
                Invoke-Installer "/i `"$(Join-Path $folder $plan.packages[0].name)`""
            } catch {
                if ($plan.recovery) { Invoke-Installer "/i `"$(Join-Path $folder $plan.recovery.name)`"" }
                throw
            }
        } else {
            Install-Portable
        }
        exit 0
    } catch {
        $_ | Out-String | Set-Content -LiteralPath (Join-Path $folder 'error.txt') -Encoding UTF8
        exit 1
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
    if (Test-Path -LiteralPath $plan.config -PathType Leaf) {
        Copy-Item -LiteralPath $plan.config -Destination "$($plan.config).before-update" -Force
    }
    Install-Binaries
} catch {
    $failure = $_
}

$restarted = $false
if ($parentClosed) {
    try {
        if ($failure) {
            $env:VOLUME3_UPDATE_FAILED = '1'
            $env:VOLUME3_UPDATE_ERROR = "The update could not be installed. Your previous version has been restarted.`n`n$failure"
            if ($plan.recovery) { $env:VOLUME3_UPDATE_ERROR += "`nRecovery packages: $folder" }
        } else {
            Remove-Item Env:VOLUME3_UPDATE_FAILED, Env:VOLUME3_UPDATE_ERROR -ErrorAction SilentlyContinue
        }
        # A Windows argument ending in a backslash must escape it before the closing quote.
        $configArg = $plan.config -replace '(\\+)$', '$1$1'
        Start-Process -FilePath $plan.executable -ArgumentList "--config `"$configArg`"" -WindowStyle Hidden
        $restarted = $true
    } catch {
        $failure = "$failure Could not restart Volume3: $_"
    }
}
if ($failure -and $plan.recovery) {
    $failure = "$failure Recovery MSI and installation details are saved in $folder"
} else {
    foreach ($name in @('ready', 'go', 'plan.json', 'install.ps1', 'error.txt') + @($plan.packages.name) + @($plan.recovery.name | Where-Object { $_ })) {
        Remove-Item -LiteralPath (Join-Path $folder $name) -ErrorAction SilentlyContinue
    }
    try { [IO.Directory]::Delete($folder) } catch [IO.IOException] { }
}
if ($failure) {
    if (-not $restarted) {
        $shell = New-Object -ComObject WScript.Shell
        $null = $shell.Popup("Volume3 update failed: $failure", 30, 'Volume3', 16)
    }
    exit 1
}
