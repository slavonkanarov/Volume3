# Volume3 plan

## Goal

Build a convenient portable Windows 11 application in Rust. Its feature set should cover Volume²'s volume control, keyboard and mouse control, audio devices and applications, and on-screen indicators. For external control, only a command-line interface is in scope for now. Named profiles are the main additional feature.

The [Volume² repository](https://github.com/irzyxa/Volume2) may be used as a reference for behavior and interface ideas. Check the license and provenance of any code or asset before copying it; the project should have its own implementation and visual identity.

## Current state

The first implementation now has a Windows tray icon with a left-click volume panel, a right-click playback-device menu, Windows sound shortcuts, and Exit. The menu saves a selected device in the active profile. A separate tray executable avoids a console window; the CLI remains available. The Windows target builds and Rust tests pass. The panel, menu, Windows Legacy submenu, Exit action, and reselecting the current output device were checked at runtime on Windows 11. The master slider was tested with a reversible 1% change; switching to another device and adjusting application sliders still need verification.

The full settings window, hooks, OSD, and complete profile behavior are still planned. Follow the stages below while keeping the first version small.

## Product decisions

- Target Windows 11 only.
- Ship a portable executable and an optional per-user MSI. No system service is required for normal use.
- Offer a tray interface, a compact control panel, a full settings window, and a CLI.
- Keep mouse controls, hotkeys, and indicator appearance global.
- Keep profiles focused on the selected output device and volume levels.

### Profile semantics

Each named profile stores:

- the selected output device;
- target volume levels for individual output devices;
- target volume levels for applications, keyed by executable name.

Examples: Default, Cinema, Gaming.

Selecting a profile applies its device choice and saved levels. Apply application rules to sessions that are already running and once when a matching new audio session appears. Do not continuously force the level back after the user adjusts it manually. An explicit "Save current state" action updates the profile.

If a saved device is unavailable, skip that device, report it clearly, and preserve the rest of the profile. A missing application is normal; its rule remains ready for the next matching session.

## User experience

### Tray and compact panel

- Left-click opens a compact panel with the output device, master volume, and application sliders.
- The right-click tray menu switches output devices and provides Windows sound shortcuts and Exit.
- The mouse wheel over the tray icon adjusts volume when enabled.

### Main window

- List playback and recording devices and choose the default device.
- Show a mixer for running applications with volume and mute controls.
- Create a profile from the current state; rename, delete, save, and apply profiles.
- Configure hotkeys, mouse behavior, and on-screen indicators in separate sections.

### On-screen display

- Provide bar, compact, and text styles.
- Configure screen position, display duration, color, size, and monitor.
- Show feedback for volume and mute changes and for device or profile switching.

## Implementation stages

### Stage 0: Make the draft build

- Fix the current Windows binding errors and connect the missing UI module.
- Keep configuration, audio, CLI, input, and UI code in separate modules.
- Keep Cargo.lock in the repository.
- Run cargo fmt, cargo test, and cargo check for the Windows target.

**Done when:** the project compiles for its target and all existing tests pass.

### Stage 1: Audio, devices, and CLI

- Master volume, mute/unmute, adjustment step, stereo balance, and current level.
- Enumerate active playback and recording devices; select a default device.
- Enumerate sessions on every active output device; adjust or mute an application.
- CLI commands for reading and changing these values.
- Clear errors for unknown devices, processes, profiles, and arguments.

**Done when:** CLI commands work without a running GUI and handle multiple devices and multiple sessions from one application.

### Stage 2: Profiles

- GUI and CLI actions to create, list, select, save, rename, and delete profiles.
- Create a profile from current device and application levels.
- Apply rules once to newly created audio sessions.
- Switch profiles through the tray, a hotkey, and the CLI.
- Handle disconnected devices and stopped applications without losing rules.

**Done when:** Cinema and Gaming profiles can be saved, switched, and restored after restarting the application.

### Stage 3: Keyboard and mouse

- Configurable global hotkeys for volume, mute, microphone mute, next/previous profile, and opening the window.
- Wheel control over the taskbar, tray icon, and selected screen edges.
- Configurable step, active areas, and modifier keys; detect conflicting bindings.
- Optionally suspend input handling in full-screen applications.

**Done when:** controls work with a hidden window, normal scrolling is unaffected outside configured areas, and hooks are released on exit.

### Stage 4: UI and indicators

- Finish the tray, compact panel, main window, mixer, and profile editor.
- Implement the three OSD styles and appearance settings.
- Show microphone and current device status where helpful.
- Support Windows 11 DPI scaling, multiple monitors, and light/dark themes.

**Done when:** common tasks require neither manual JSON editing nor an open console.

### Stage 5: Windows 11 verification and packaging

- Test on Windows 11 with USB and Bluetooth devices, multiple outputs, several applications, device disconnect/reconnect, and sleep/resume.
- Test a profile containing an unavailable device or a stopped application.
- Produce a portable build and document build and usage steps.
- Confirm that removing the application folder removes its own persistent data.

## Persistence and system footprint

- Store profiles and global settings in one JSON file next to the executable by default.
- Allow an explicit alternate path through --config.
- Save safely: write to a temporary file in the same directory, then replace the JSON; clean up after failures.
- Do not add telemetry, background services, registry entries, or automatic startup by default.
- If the executable directory is not writable, show an error and ask for an explicit --config path. Do not silently write to AppData.
- Store only necessary settings, not activity history or window titles.

## Technical notes

- Use Windows Core Audio for endpoint and audio-session volume.
- Windows default-device switching currently relies on the private IPolicyConfig COM interface. Isolate that code and verify it on current Windows 11 builds. A failed switch must be visible to the user.
- Use Win32 hotkeys and a low-level mouse hook for global input. Keep hook callbacks short.
- A Windows-target check in WSL validates Rust types and bindings but cannot prove that audio APIs, tray behavior, hooks, or OSD work at runtime. Test those on Windows 11.
- For native Windows builds and runtime testing, use the Windows MSVC Rust toolchain and its C++ build prerequisites. See the [Rust installation guide](https://www.rust-lang.org/tools/install) and [MSVC prerequisites](https://rust-lang.github.io/rustup/installation/windows-msvc.html).

