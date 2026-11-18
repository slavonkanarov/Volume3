#requires -Version 5.1
param([switch]$Offline, [switch]$SystemOnly)

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot\..").Path
if ($env:USERPROFILE -ne [Environment]::GetFolderPath('UserProfile')) {
    throw 'Run this script in a normal PowerShell session under your Windows account, not in a sandbox.'
}
if (Get-Process volume3-tray, volume3 -ErrorAction SilentlyContinue) {
    throw 'Exit Volume3 from the tray before installing the new version.'
}
$installDir = Join-Path $env:ProgramFiles 'Volume3'
$shortcutDir = [Environment]::GetFolderPath('CommonPrograms')
$tray = Join-Path $installDir 'volume3-tray.exe'
$cli = Join-Path $installDir 'volume3.exe'

function Write-Installation {
    Write-Host "Installed: $tray"
    Write-Host "CLI: $cli"
    Write-Host "Start menu shortcut: $(Join-Path $shortcutDir 'Volume3.lnk')"
    Write-Host 'Existing settings in %LOCALAPPDATA%\Volume3 are preserved.'
}

Push-Location $repo
try {
    if (-not $SystemOnly) {
        $metadata = & cargo +stable-x86_64-pc-windows-msvc metadata --no-deps --format-version 1 --locked --offline | ConvertFrom-Json
        if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed' }
        $base = [version]($metadata.packages | Where-Object name -eq 'volume3').version
        $patch = $base.Build
        $tags = git -c "safe.directory=$($repo.Replace('\', '/'))" tag --list "v$($base.Major).$($base.Minor).*"
        if ($LASTEXITCODE -ne 0) { throw 'git tag failed' }
        $pattern = '^v' + [regex]::Escape("$($base.Major).$($base.Minor)") + '\.(0|[1-9][0-9]*)$'
        foreach ($tag in $tags) {
            if ($tag -cmatch $pattern) { $patch = [Math]::Max($patch, [long]$Matches[1] + 1) }
        }
        $buildArgs = @('build', '--release', '--locked', '--target', 'x86_64-pc-windows-msvc', '--target-dir', "$repo\target")
        if ($Offline) { $buildArgs += '--offline' }
        $previousBuildVersion = $env:VOLUME3_BUILD_VERSION
        $buildVersion = "$($base.Major).$($base.Minor).$patch-dev"
        try {
            $env:VOLUME3_BUILD_VERSION = $buildVersion
            Write-Host "Building Volume3 $env:VOLUME3_BUILD_VERSION"
            & cargo +stable-x86_64-pc-windows-msvc @buildArgs
            if ($LASTEXITCODE -ne 0) {
                throw 'Build failed. Rust MSVC and Visual Studio C++ Build Tools with the Windows SDK are required.'
            }
        } finally {
            $env:VOLUME3_BUILD_VERSION = $previousBuildVersion
        }
    }
    $releaseDir = Join-Path $repo 'target\x86_64-pc-windows-msvc\release'
    foreach ($name in @('volume3-tray.exe', 'volume3.exe')) {
        if (-not (Test-Path -LiteralPath (Join-Path $releaseDir $name) -PathType Leaf)) {
            throw "Build did not produce $name"
        }
    }
    $reportedVersion = & (Join-Path $releaseDir 'volume3.exe') --version
    if ($LASTEXITCODE -ne 0 -or (-not $SystemOnly -and $reportedVersion -ne "Volume3 $buildVersion")) {
        throw "Built CLI reports an unexpected version: $reportedVersion (expected Volume3 $buildVersion)"
    }

    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    if (-not ([Security.Principal.WindowsPrincipal]$identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        if ($SystemOnly) { throw 'Administrator rights are required to install Volume3.' }
        $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"", '-SystemOnly')
        if ($Offline) { $arguments += '-Offline' }
        try {
            $elevated = Start-Process -FilePath (Get-Process -Id $PID).Path -ArgumentList $arguments `
                -Verb RunAs -WorkingDirectory $repo -WindowStyle Hidden -Wait -PassThru
        } catch {
            throw "Administrator permission was not granted: $($_.Exception.Message)"
        }
        if ($elevated.ExitCode -ne 0) {
            throw "Elevated installation failed with exit code $($elevated.ExitCode). Run PowerShell as Administrator to see the error."
        }
        Write-Installation
        return
    }

    $msiInstallations = @(Get-ItemProperty `
        'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*', `
        'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*' -ErrorAction SilentlyContinue |
        Where-Object { $_.DisplayName -eq 'Volume3' -and $_.WindowsInstaller -eq 1 })
    foreach ($msi in $msiInstallations) {
        if ($msi.PSChildName -notmatch '^\{[0-9A-Fa-f-]{36}\}$') {
            throw "Cannot identify the old Volume3 MSI: $($msi.PSChildName)"
        }
        $process = Start-Process msiexec.exe -ArgumentList @('/x', $msi.PSChildName, '/qn', '/norestart') `
            -WindowStyle Hidden -Wait -PassThru
        if ($process.ExitCode -eq 3010) {
            throw 'The old Volume3 MSI was removed. Restart Windows, then run this script again.'
        }
        if ($process.ExitCode -ne 0) {
            throw "Removing the old Volume3 MSI failed with exit code $($process.ExitCode)."
        }
    }

    New-Item -ItemType Directory -Force $installDir, $shortcutDir | Out-Null
    Copy-Item -LiteralPath (Join-Path $releaseDir 'volume3-tray.exe'), `
        (Join-Path $releaseDir 'volume3.exe'), `
        "$repo\assets\volume3.ico", "$repo\LICENSE", "$repo\README.md" -Destination $installDir -Force
    Copy-Item -LiteralPath "$repo\installer\installed.marker" `
        -Destination (Join-Path $installDir 'volume3.installed') -Force
    foreach ($name in @('volume3-tray.exe', 'volume3.exe')) {
        if ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $releaseDir $name)).Hash -ne
            (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $installDir $name)).Hash) {
            throw "Installed $name does not match the current build"
        }
    }

    $shell = New-Object -ComObject WScript.Shell
    $shortcutPaths = @((Join-Path $shortcutDir 'Volume3.lnk'))
    $userShortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Volume3.lnk'
    if (Test-Path -LiteralPath $userShortcut) { $shortcutPaths += $userShortcut }
    foreach ($path in $shortcutPaths) {
        $shortcut = $shell.CreateShortcut($path)
        $shortcut.TargetPath = $tray
        $shortcut.WorkingDirectory = $installDir
        $shortcut.IconLocation = "$(Join-Path $installDir 'volume3.ico'),0"
        $shortcut.Description = 'Volume3 audio controls'
        $shortcut.Save()
        if ($shell.CreateShortcut($path).TargetPath -ne $tray) {
            throw "Start menu shortcut points to the wrong executable: $path"
        }
    }

    Write-Installation
} finally {
    Pop-Location
}
