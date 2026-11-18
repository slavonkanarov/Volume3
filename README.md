# Volume3

Volume3 is an early portable Windows 11 audio controller written in Rust. Its first usable interface is a tray icon for choosing the playback device and opening Windows sound controls. The tray icon uses a white Windows speaker symbol on a transparent background, as shown in the reference. The image is stored in [assets/volume3.png](assets/volume3.png).

## Build and run

On Windows 11, install the Rust MSVC toolchain and its C++ build prerequisites, then run:

```powershell
cargo build --release
.\target\release\volume3-tray.exe
```

Left click the tray icon to open the purple volume panel at the right edge of the active monitor. It shows the current output, the master volume, and separate sliders for applications using that output. Drag a slider to change its level; use the mouse wheel to scroll through applications. Right click opens the dark device menu. The checked device is the current default output. Selecting another device changes the Windows default output and saves its ID in the active profile. The menu also offers a Windows Legacy submenu, Volume Mixer, Settings, and Exit. Windows Legacy opens the classic mixer and the Playback, Recording, and Sounds tabs; its last two entries open the modern sound settings and volume mixer. The top-level Volume Mixer entry opens the Windows 11 volume page. Settings opens a dark Volume3 window with General and Updates pages. General contains a Start with Windows switch. The switch manages the current user’s Windows sign-in startup entry and preserves a custom `--config` path.

`volume3-tray.exe` has no console window. `volume3.exe` remains the CLI. For example:

```powershell
.\target\release\volume3.exe device list
.\target\release\volume3.exe volume get
.\target\release\volume3.exe volume set 35
```

Portable builds store settings in `volume3.json` beside the executable. System installations, whether made by MSI or the local script, store them in `%LocalAppData%\Volume3\volume3.json` so the installed application can run without administrator rights. Both programs accept `--config PATH` before the command, or before launching the tray, to use another location. The programs do not create a service or telemetry.

## Install on this PC without MSI

Exit Volume3 from the tray, then run this from a normal PowerShell session under your Windows account in the repository root on Windows 11. The script requests administrator permission through Windows UAC:

```powershell
.\scripts\install-local.ps1
```

The script builds the current source into both release executables and installs them with the icon, license, and README in `C:\Program Files\Volume3`. It removes any existing Volume3 MSI installation first, then creates an all-users Start menu shortcut and updates an existing current-user shortcut to point to the new tray executable. MSI is used only to remove an older MSI installation; WiX is not needed. The script needs the Rust MSVC toolchain, Visual Studio C++ Build Tools, and the Windows SDK. It does not start Volume3 or enable sign-in startup. Existing settings in `%LOCALAPPDATA%\Volume3\volume3.json` are preserved.

Like RustClip, the script assigns a development version from local stable Git tags. It uses the next patch in the `Cargo.toml` major/minor series, never below the Cargo version, followed by `-dev`. For example, tag `v0.1.5` produces `0.1.6-dev`. It prints the build version, verifies it with `volume3.exe --version`, and compares the installed executables with the build using SHA-256. It does not fetch source changes or tags; update your checkout before installing if needed. Reinstalling the same version series without new tags keeps the same development version while rebuilding the current source.

Use `-Offline` when Cargo dependencies are already cached. To remove this script-based installation, exit Volume3, delete `C:\Program Files\Volume3` and the `Volume3.lnk` shortcuts from the all-users and current-user Start menus. Settings remain in `%LOCALAPPDATA%\Volume3` unless you remove them separately. The previous per-user copy in `%LOCALAPPDATA%\Programs\Volume3` is left untouched, but its Start menu shortcut is redirected to the system installation.

## Windows MSI build on GitHub Actions

The [Windows MSI workflow](.github/workflows/windows-msi.yml) builds and tests on Windows x64 and uploads `volume3-windows-x64` with `Volume3-<version>-x64.msi`, both executables, and the license. Actions artifacts are kept for 90 days. Published downloads remain available on the [Releases page](https://github.com/slavonkanarov/Volume3/releases).

`Cargo.toml` sets the minimum `major.minor.patch` version. The next patch is one higher than the highest stable tag in the current `major.minor` series, but never lower than the Cargo version. The workflow does not modify `Cargo.toml`.

| Trigger | Version | Distribution |
| --- | --- | --- |
| Push to `main` | `<next version>` | Stable release with an automatic `v<version>` tag |
| Push to `dev`, pull request, or manual run outside `release` | `<next version>-dev.<run number>` | Actions artifact only |
| Push to `release` (optional) | `<next version>-rc.<run number>` | Actions artifact only |
| Push a `v<version>` or `v<version>-rc.<number>` tag | Version from the tag | Stable release from `main` or prerelease from `release`, respectively |

Pull requests target `dev`, `release`, or `main`. Manual runs on the `release` branch use the RC suffix and do not publish. The exact build version is displayed at the bottom of the Settings sidebar, on the Updates page, and by `volume3.exe --version`. Plain Cargo builds use the version from `Cargo.toml` unless `VOLUME3_BUILD_VERSION` is set; `install-local.ps1` sets it automatically as described above. Windows Installer uses only the numeric version, so an RC and its corresponding stable release have the same MSI upgrade version.

## Updating Volume3

Open **Settings → Updates** to manage updates:

- **Check for updates automatically** checks at startup and every six hours; on by default.
- **Download, install, and restart automatically** installs newer compatible releases after Settings is closed; off by default.
- **Include prereleases** adds prereleases to automatic checks and the version list; off by default.
- **Check for updates** refreshes the version list even when automatic checking is off.
- **Cancel** cancels a check or download and discards a pending download.
- Select an available version to read its release notes, then use **Install selected version** to install it and restart. The status shows download progress and errors.

The tray **Updates** submenu offers the same checking and installation preferences and a shortcut to install the newest available update. Only releases with complete packages and valid SHA-256 digests are offered. Automatic checks only propose newer versions. Installing an older or equal version turns off automatic installation to keep the chosen version.

Downloads are verified against the SHA-256 digests published by GitHub Releases. The updater replaces both executables in portable and script-based installations or installs the new MSI, then restarts the tray with the same configuration path. Protected folders such as Program Files and MSI installations request administrator approval; the helper restarts the application from the original process without elevation. Cancelling approval restarts the existing version. Before installation, the configuration is copied to `<config path>.before-update`, replacing the previous backup. Older versions may not understand newer configuration formats; backups are not restored automatically.

Settings buttons, switches, version selection, and application messages share the dark appearance. After a failed update, the restarted application shows the error in its own window. A system dialog is used only if the application cannot restart or create its message window.

MSI downgrades require the current version's complete MSI to be available in Releases. The updater downloads and verifies it before uninstalling, and attempts to reinstall it if the selected version fails. After a failed downgrade, recovery packages remain in the temporary update directory reported in the error. Script-based installations can also be updated by rerunning `scripts/install-local.ps1`. To get these controls when running an older release, install this source with the local script or install a newer release manually once.

When opened normally, the MSI shows a branded setup wizard with the license, installation progress, and a completion message. Silent installs with `/qn` remain available. The MSI requests administrator approval through Windows UAC, installs into `C:\Program Files\Volume3`, and adds a Start menu shortcut for all users. Later MSI builds replace the existing machine-wide installation. The previous 0.1.1 MSI used a per-user installation context, so Windows Installer cannot upgrade it in place: uninstall 0.1.1 once before installing 0.1.2 or newer. Volume3 copies existing settings from `%LocalAppData%\Programs\Volume3\volume3.json` to its new per-user settings path on first launch. The package does not start Volume3 automatically after installation or enable sign-in startup until the user turns it on in Settings.

The MSI replaces its packaged application files even when unversioned local builds have newer modification dates. User configuration is stored separately and is not part of the MSI payload.

## Current limits

The Windows MSVC target builds, and the Rust tests pass on Windows. The left-click panel, right-click menu, Windows Legacy submenu, Exit action, and reselecting the current output device were checked at runtime on Windows 11. The master and application sliders were checked with reversible 1% changes. Switching to a different output device still needs runtime verification. Default-device switching uses an isolated, undocumented `IPolicyConfig` call; errors appear in the tray interface. The full control panel, hotkeys, mouse controls, on-screen indicators, and richer profile editing remain planned in [PLAN.md](PLAN.md).

The Updates page, preference persistence, manual GitHub checks with automatic checking disabled, version selection, cancellation, and the styled update-error window were checked in the running application. The portable helper was tested for replacing both executables, restarting with a custom configuration path, configuration backup, checksum rejection, and rollback with a locked executable. A regression check reproduces PowerShell's wrapped .NET access-denied error and verifies that it requests elevation while file-lock failures do not.

Local Windows 11 checks exercised real protected-folder replacement with elevation, an upgrade from the published 0.1.5 MSI payload to a 0.1.6 candidate MSI, and installation of the candidate over modified local EXEs. Both installed executables matched the candidate payload; the original configuration and startup entry stayed unchanged. Start menu launch, the installed Updates page, CLI device enumeration, and restart without administrator privileges passed. These MSI checks used `/qn`; setup wizard interactions were not exercised. MSI downgrade recovery order has been tested with installer calls mocked, and still needs a real installation check.

The [Volume² project](https://github.com/irzyxa/Volume2) is a reference for functionality and interaction ideas. Volume3 uses its own code; the tray icon uses a rendered Windows speaker glyph.
