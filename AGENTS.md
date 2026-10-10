# Agent instructions

## Current task boundary

The user requested documentation only. Keep the existing Rust files unchanged. Do not resume implementation, fix compilation, or expand features until the user explicitly asks to return to coding. The product plan is in PLAN.md.

## Language

- Write repository documentation, code comments, commit messages, and future project text in English.
- Communicate with the user in Russian.

## Product requirements

- Target Windows 11.
- Implement in Rust.
- Cover volume control, mouse and keyboard control, audio devices and applications, on-screen indicators, and a CLI.
- Profiles store the chosen output device and volume levels for devices and applications. Hotkey, mouse, and indicator settings are global.
- Make profile switching available through the GUI, tray, hotkeys, and CLI.
- Common actions must be usable without editing the configuration file manually.
- The [Volume² repository](https://github.com/irzyxa/Volume2) can inform behavior and design. Check licenses before copying code or assets.

## Data and behavior

- Use portable storage by default: configuration next to the executable, with --config for an explicit alternate path.
- Do not create services, registry entries, telemetry, or AppData files unless the user requests a different design.
- Avoid unnecessary history, logs, and temporary files. Protect the configuration against partial writes.
- Apply a profile when selected. Apply application rules once to new matching audio sessions; do not repeatedly undo manual volume changes.
- Handle missing devices and applications clearly without damaging saved profiles.

## Repository work

- Existing changes to .gitignore and LICENSE predate this application work; do not overwrite them.
- The current Rust code is an unfinished draft. Do not claim it works without a Windows 11 runtime check.
- When implementation resumes, first resolve the cargo check errors in src/audio.rs and connect the UI, then follow PLAN.md.
- Run cargo fmt, cargo test, and cargo check for the Windows target. Verify global hooks, tray, OSD, and default-device switching on Windows 11.
- Default-device switching through IPolicyConfig uses an undocumented Windows interface. Keep that call isolated and check its result.

