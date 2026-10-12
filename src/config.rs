use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    /// Active playback device. None means keep the current default device.
    pub output_device: Option<String>,
    /// Device ID -> target volume. Disconnected devices are skipped.
    pub device_volumes: BTreeMap<String, u8>,
    /// Executable file name -> target volume. Applied to running sessions.
    pub application_volumes: BTreeMap<String, u8>,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            output_device: None,
            device_volumes: BTreeMap::new(),
            application_volumes: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct InputSettings {
    pub wheel_on_taskbar: bool,
    pub wheel_on_screen_edge: bool,
    pub edge_width_px: i32,
    pub wheel_step: u8,
    pub hotkeys: Vec<Hotkey>,
}

impl Default for InputSettings {
    fn default() -> Self {
        Self {
            wheel_on_taskbar: true,
            wheel_on_screen_edge: false,
            edge_width_px: 8,
            wheel_step: 2,
            hotkeys: vec![
                Hotkey {
                    modifiers: 3,
                    virtual_key: 0x26,
                    action: Action::VolumeUp,
                },
                Hotkey {
                    modifiers: 3,
                    virtual_key: 0x28,
                    action: Action::VolumeDown,
                },
                Hotkey {
                    modifiers: 3,
                    virtual_key: 0x4d,
                    action: Action::Mute,
                },
                Hotkey {
                    modifiers: 3,
                    virtual_key: 0x50,
                    action: Action::NextProfile,
                },
            ],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hotkey {
    /// Win32 MOD_* bitmask: Alt=1, Ctrl=2, Shift=4, Win=8.
    pub modifiers: u32,
    pub virtual_key: u32,
    pub action: Action,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    VolumeUp,
    VolumeDown,
    Mute,
    MuteMicrophone,
    NextProfile,
    PreviousProfile,
    ShowWindow,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplaySettings {
    pub osd_enabled: bool,
    pub osd_duration_ms: u32,
    pub osd_style: OsdStyle,
    pub accent_rgb: [u8; 3],
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            osd_enabled: true,
            osd_duration_ms: 1400,
            osd_style: OsdStyle::Bar,
            accent_rgb: [78, 161, 255],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OsdStyle {
    Bar,
    Text,
    Compact,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub active_profile: String,
    pub profiles: BTreeMap<String, Profile>,
    pub input: InputSettings,
    pub display: DisplaySettings,
}

impl Default for Config {
    fn default() -> Self {
        let mut profiles = BTreeMap::new();
        profiles.insert("Default".into(), Profile::default());
        Self {
            active_profile: "Default".into(),
            profiles,
            input: InputSettings::default(),
            display: DisplaySettings::default(),
        }
    }
}

impl Config {
    pub fn default_path() -> Result<PathBuf, String> {
        let executable = env::current_exe().map_err(|e| e.to_string())?;
        let directory = executable
            .parent()
            .ok_or("executable has no parent directory")?;
        let local_appdata = env::var_os("LOCALAPPDATA").map(PathBuf::from);
        Self::path_for_executable(directory, local_appdata.as_deref())
    }

    fn path_for_executable(
        directory: &Path,
        local_appdata: Option<&Path>,
    ) -> Result<PathBuf, String> {
        if !directory.join("volume3.installed").is_file() {
            return Ok(directory.join("volume3.json"));
        }
        let local_appdata = local_appdata.ok_or("LOCALAPPDATA is not set")?;
        let path = local_appdata.join("Volume3").join("volume3.json");
        let previous = local_appdata
            .join("Programs")
            .join("Volume3")
            .join("volume3.json");
        if !path.exists() && previous.is_file() {
            Self::load(&previous)?.save(&path)?;
        }
        Ok(path)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = fs::read(path).map_err(|e| e.to_string())?;
        let config: Self = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let data = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        let result = (|| {
            let mut file = fs::File::create(&tmp).map_err(|e| e.to_string())?;
            file.write_all(&data).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            drop(file);
            replace_file(&tmp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.profiles.contains_key(&self.active_profile) {
            return Err(format!("unknown active profile: {}", self.active_profile));
        }
        for (name, profile) in &self.profiles {
            if name.trim().is_empty() {
                return Err("profile name cannot be empty".into());
            }
            if profile
                .device_volumes
                .values()
                .chain(profile.application_volumes.values())
                .any(|v| *v > 100)
            {
                return Err(format!("volume in profile {name} must be 0..100"));
            }
        }
        if self.input.wheel_step == 0 || self.input.wheel_step > 50 {
            return Err("wheel step must be 1..50".into());
        }
        Ok(())
    }

    pub fn active(&self) -> &Profile {
        &self.profiles[&self.active_profile]
    }

    pub fn switch(&mut self, name: &str) -> Result<(), String> {
        if !self.profiles.contains_key(name) {
            return Err(format!("profile not found: {name}"));
        }
        self.active_profile = name.into();
        Ok(())
    }

    pub fn cycle(&mut self, forward: bool) {
        let names: Vec<_> = self.profiles.keys().cloned().collect();
        if names.is_empty() {
            return;
        }
        let current = names
            .iter()
            .position(|name| name == &self.active_profile)
            .unwrap_or(0);
        let next = if forward {
            (current + 1) % names.len()
        } else {
            (current + names.len() - 1) % names.len()
        };
        self.active_profile = names[next].clone();
    }
}

#[cfg(windows)]
fn replace_file(source: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        },
    };
    let from: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
        .map_err(|e| e.to_string())
    }
}

#[cfg(not(windows))]
fn replace_file(source: &Path, target: &Path) -> Result<(), String> {
    fs::rename(source, target).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_replaces_existing_config() {
        let folder = std::env::temp_dir().join(format!("volume3-config-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("volume3.json");
        let mut config = Config::default();
        let result = (|| {
            config.save(&path)?;
            config.active_profile = "Cinema".into();
            config.profiles.insert("Cinema".into(), Profile::default());
            config.save(&path)?;
            let loaded = Config::load(&path)?;
            assert_eq!(loaded.active_profile, "Cinema");
            Ok::<(), String>(())
        })();
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("json.tmp"));
        let _ = fs::remove_dir(&folder);
        result.unwrap();
    }

    #[test]
    fn installed_config_migrates_existing_user_settings() {
        let root =
            std::env::temp_dir().join(format!("volume3-installed-config-{}", std::process::id()));
        let result = (|| {
            let installed = root.join("Program Files").join("Volume3");
            fs::create_dir_all(&installed).map_err(|e| e.to_string())?;
            fs::write(installed.join("volume3.installed"), "").map_err(|e| e.to_string())?;
            let local_appdata = root.join("AppData").join("Local");
            let previous = local_appdata
                .join("Programs")
                .join("Volume3")
                .join("volume3.json");
            let mut config = Config::default();
            config.active_profile = "Cinema".into();
            config.profiles.insert("Cinema".into(), Profile::default());
            config.save(&previous)?;
            let path = Config::path_for_executable(&installed, Some(&local_appdata))?;
            assert_eq!(path, local_appdata.join("Volume3").join("volume3.json"));
            assert_eq!(Config::load(&path)?.active_profile, "Cinema");
            Ok::<(), String>(())
        })();
        let _ = fs::remove_dir_all(&root);
        result.unwrap();
    }

    #[test]
    fn profiles_round_trip_and_cycle() {
        let mut config = Config::default();
        let mut cinema = Profile::default();
        cinema.device_volumes.insert("device-1".into(), 35);
        config.profiles.insert("Cinema".into(), cinema);
        config.switch("Cinema").unwrap();
        let data = serde_json::to_string(&config).unwrap();
        let mut parsed: Config = serde_json::from_str(&data).unwrap();
        parsed.validate().unwrap();
        assert_eq!(parsed.active().device_volumes["device-1"], 35);
        parsed.cycle(true);
        assert_eq!(parsed.active_profile, "Default");
    }
}
