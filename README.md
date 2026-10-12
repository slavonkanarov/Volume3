# Volume3

Volume3 is an early portable Windows 11 audio controller written in Rust. Its first usable interface is a tray icon for choosing the playback device and opening Windows sound controls. The tray icon uses a white Windows speaker symbol on a transparent background, as shown in the reference. The image is stored in [assets/volume3.png](assets/volume3.png).

## Build and run

On Windows 11, install the Rust MSVC toolchain and its C++ build prerequisites, then run:

```powershell
cargo build --release
.\target\release\volume3-tray.exe
```

Left click the tray icon to open the purple volume panel at the right edge of the active monitor. It shows the current output, the master volume, and separate sliders for applications using that output. Drag a slider to change its level; use the mouse wheel to scroll through applications. Right click opens the dark device menu. The checked device is the current default output. Selecting another device changes the Windows default output and saves its ID in the active profile. The menu also offers a Windows Legacy submenu, Volume Mixer, Settings, and Exit. Windows Legacy opens the classic mixer and the Playback, Recording, and Sounds tabs; its last two entries open the modern sound settings and volume mixer. The top-level Volume Mixer and Settings entries open the corresponding Windows 11 pages until Volume3 has its own settings window.

`volume3-tray.exe` has no console window. `volume3.exe` remains the CLI. For example:

```powershell
.\target\release\volume3.exe device list
.\target\release\volume3.exe volume get
.\target\release\volume3.exe volume set 35
```

Settings are stored in `volume3.json` beside the executable when changed. Both programs accept `--config PATH` before the command, or before launching the tray, to use another location. Neither program creates a service, registry entry, or AppData file.

## Windows MSI build on GitHub Actions

The [Windows MSI workflow](.github/workflows/windows-msi.yml) runs on pushes and can also be started manually from the Actions tab. It builds and tests the Rust project on a Windows x64 runner, creates a per-user MSI, and uploads `volume3-windows-x64` with the MSI, both executables, and the license. The Actions artifact is retained for 90 days. Successful builds from `main` are also published as separate prereleases on the [Releases page](https://github.com/slavonkanarov/Volume3/releases), with the MSI, both executables, and the license. Release downloads remain available after the Actions artifact expires.

The MSI installs into the current user's Programs directory and adds a Start menu shortcut. It needs no administrator rights. Volume3 keeps its portable `volume3.json` beside the installed executable, so the same executable also works without the installer. The package does not start Volume3 automatically after installation.

## Current limits

The Windows target builds, and the Rust tests pass on Linux. The left-click panel, right-click menu, Windows Legacy submenu, Exit action, and reselecting the current output device were checked at runtime on Windows 11. The master and application sliders were checked with reversible 1% changes. Switching to a different output device still needs runtime verification. Default-device switching uses an isolated, undocumented `IPolicyConfig` call; errors appear in the tray interface. The full control panel, hotkeys, mouse controls, on-screen indicators, and richer profile editing remain planned in [PLAN.md](PLAN.md).

The [Volume² project](https://github.com/irzyxa/Volume2) is a reference for functionality and interaction ideas. Volume3 uses its own code; the tray icon uses a rendered Windows speaker glyph.
