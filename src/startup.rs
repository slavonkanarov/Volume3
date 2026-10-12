use std::{
    env,
    ffi::{OsStr, OsString},
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR},
        System::Registry::{
            RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW, RegOpenKeyExW,
            RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE,
            REG_SZ, RRF_RT_REG_SZ,
        },
    },
};

use crate::config::Config;

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: PCWSTR = w!("Volume3");

struct OpenKey(HKEY);

impl Drop for OpenKey {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

pub fn is_enabled(config_path: &Path) -> Result<bool, String> {
    let expected = current_command(config_path)?;
    Ok(read_command(RUN_KEY, VALUE_NAME)?.is_some_and(|actual| actual == expected))
}

pub fn set_enabled(config_path: &Path, enabled: bool) -> Result<(), String> {
    if !enabled {
        return delete_command(RUN_KEY, VALUE_NAME);
    }
    let executable = tray_executable()?;
    if !executable.is_file() {
        return Err(format!(
            "Cannot enable startup: {} is missing",
            executable.display()
        ));
    }
    let command = current_command(config_path)?;
    write_command(RUN_KEY, VALUE_NAME, &command)
}

fn write_command(key_path: PCWSTR, value_name: PCWSTR, command: &OsStr) -> Result<(), String> {
    let mut data: Vec<u16> = command.encode_wide().collect();
    if data.len() >= 260 {
        return Err("Cannot enable startup: the command is longer than 259 characters".into());
    }
    data.push(0);
    let bytes = unsafe { std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), data.len() * 2) };
    let mut key = HKEY::default();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            key_path,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    };
    check(status, "Open Windows startup settings")?;
    let key = OpenKey(key);
    check(
        unsafe { RegSetValueExW(key.0, value_name, None, REG_SZ, Some(bytes)) },
        "Enable Volume3 startup",
    )
}

fn delete_command(key_path: PCWSTR, value_name: PCWSTR) -> Result<(), String> {
    let mut key = HKEY::default();
    let status =
        unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, key_path, None, KEY_SET_VALUE, &mut key) };
    if missing(status) {
        return Ok(());
    }
    check(status, "Open Windows startup settings")?;
    let key = OpenKey(key);
    let status = unsafe { RegDeleteValueW(key.0, value_name) };
    if missing(status) {
        Ok(())
    } else {
        check(status, "Disable Volume3 startup")
    }
}

fn read_command(key_path: PCWSTR, value_name: PCWSTR) -> Result<Option<OsString>, String> {
    let mut size = 0u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key_path,
            value_name,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    };
    if missing(status) {
        return Ok(None);
    }
    check(status, "Read Windows startup settings")?;
    if size % 2 != 0 || size > 32_768 {
        return Err("Invalid Volume3 startup entry".into());
    }
    let mut data = vec![0u16; size as usize / 2];
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key_path,
            value_name,
            RRF_RT_REG_SZ,
            None,
            Some(data.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    check(status, "Read Windows startup settings")?;
    let end = data
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(data.len());
    Ok(Some(OsString::from_wide(&data[..end])))
}

fn current_command(config_path: &Path) -> Result<OsString, String> {
    let executable = tray_executable()?;
    let default_path = Config::default_path()?;
    let absolute_config = if config_path.is_absolute() {
        config_path.to_path_buf()
    } else {
        env::current_dir()
            .map_err(|error| error.to_string())?
            .join(config_path)
    };
    Ok(command_line(&executable, &absolute_config, &default_path))
}

fn tray_executable() -> Result<PathBuf, String> {
    let current = env::current_exe().map_err(|error| error.to_string())?;
    let name = current.file_name().and_then(OsStr::to_str).unwrap_or("");
    if name.eq_ignore_ascii_case("volume3.exe") {
        Ok(current.with_file_name("volume3-tray.exe"))
    } else {
        Ok(current)
    }
}

fn command_line(executable: &Path, config_path: &Path, default_path: &Path) -> OsString {
    let mut command = OsString::from("\"");
    command.push(executable.as_os_str());
    command.push("\"");
    if config_path != default_path {
        command.push(" --config \"");
        command.push(config_path.as_os_str());
        command.push("\"");
    }
    command
}

fn missing(status: WIN32_ERROR) -> bool {
    status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND
}

fn check(status: WIN32_ERROR, action: &str) -> Result<(), String> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "{action}: {}",
            std::io::Error::from_raw_os_error(status.0 as i32)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_value_round_trip() {
        use windows::Win32::System::Registry::RegDeleteKeyW;

        let key_name: Vec<u16> = format!("Software\\Volume3\\AutostartTest-{}", std::process::id())
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let key = PCWSTR(key_name.as_ptr());
        let name = w!("Command");
        let command = OsString::from(r#""C:\Program Files\Volume3\volume3-tray.exe""#);
        let result = (|| {
            write_command(key, name, &command)?;
            assert_eq!(read_command(key, name)?, Some(command));
            delete_command(key, name)?;
            assert_eq!(read_command(key, name)?, None);
            Ok::<(), String>(())
        })();
        unsafe {
            let _ = RegDeleteKeyW(HKEY_CURRENT_USER, key);
        }
        result.unwrap();
    }

    #[test]
    fn startup_command_quotes_executable_and_custom_config() {
        let executable = Path::new(r"C:\Program Files\Volume3\volume3-tray.exe");
        let default = Path::new(r"C:\Users\Alice\AppData\Local\Volume3\volume3.json");
        let custom = Path::new(r"C:\My Settings\volume3.json");
        assert_eq!(
            command_line(executable, default, default),
            r#""C:\Program Files\Volume3\volume3-tray.exe""#
        );
        assert_eq!(
            command_line(executable, custom, default),
            r#""C:\Program Files\Volume3\volume3-tray.exe" --config "C:\My Settings\volume3.json""#
        );
    }
}
