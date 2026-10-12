#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    if let Err(error) = run() {
        let message: Vec<u16> = error.encode_utf16().chain(Some(0)).collect();
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::MessageBoxW(
                None,
                windows::core::PCWSTR(message.as_ptr()),
                windows::core::w!("Volume3"),
                windows::Win32::UI::WindowsAndMessaging::MB_OK
                    | windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
            );
        }
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = match args.as_slice() {
        [] => volume3::config::Config::default_path()?,
        [flag, path] if flag == "--config" => path.into(),
        _ => return Err("usage: volume3-tray [--config PATH]".into()),
    };
    volume3::ui::run_gui(path)
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Volume3 tray runs on Windows 11.");
}
