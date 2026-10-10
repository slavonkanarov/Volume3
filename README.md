# Volume3

Volume3 is an early portable Windows 11 audio controller written in Rust. Its first usable interface is a tray icon for choosing the playback device, changing master volume in 5% steps, and muting or unmuting audio. The icon is original artwork in [assets/volume3.png](assets/volume3.png).

![Volume3 tray icon](assets/volume3.png)

## Build and run

On Windows 11, install the Rust MSVC toolchain and its C++ build prerequisites, then run:

```powershell
cargo build --release
.\target\release\volume3-tray.exe
```

Left or right click the tray icon to open the menu. The checked device is the current default output. Selecting another device changes the Windows default output and saves its ID in the active profile. The menu also offers volume up/down, mute/unmute, and Exit.

`volume3-tray.exe` has no console window. `volume3.exe` remains the CLI. For example:

```powershell
.\target\release\volume3.exe device list
.\target\release\volume3.exe volume get
.\target\release\volume3.exe volume set 35
```

Settings are stored in `volume3.json` beside the executable when changed. Both programs accept `--config PATH` before the command, or before launching the tray, to use another location. Neither program creates a service, registry entry, or AppData file.

## Current limits

The Windows target passes `cargo check`, and the Rust tests pass on Linux. The tray, device switching, and audio behavior still need a Windows 11 runtime test. Default-device switching uses an isolated, undocumented `IPolicyConfig` call; errors appear in the tray interface. The full control panel, hotkeys, mouse controls, on-screen indicators, and richer profile editing remain planned in [PLAN.md](PLAN.md).

The [Volume² project](https://github.com/irzyxa/Volume2) is a reference for functionality and interaction ideas. Volume3 uses its own code and artwork.
