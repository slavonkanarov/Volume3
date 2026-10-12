#[cfg(windows)]
fn main() {
    if let Err(error) = volume3::app::run() {
        eprintln!("Volume3: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Volume3 runs on Windows 11. Use a Windows build to control audio.");
}
