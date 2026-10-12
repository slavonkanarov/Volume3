use crate::config::Profile;
use std::{collections::HashMap, ffi::c_void, path::Path};
use windows::{
    core::{Interface, GUID, PCWSTR, PWSTR},
    Win32::{
        Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
        Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation},
        Media::Audio::{
            eCapture, eCommunications, eConsole, eMultimedia, eRender, EDataFlow,
            IAudioSessionControl2, IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator,
            ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
        },
        System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc},
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
            COINIT_APARTMENTTHREADED, STGM_READ,
        },
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};

#[derive(Clone, Debug)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Clone, Debug)]
pub struct AppSession {
    pub process: String,
    pub pid: u32,
    pub device_id: String,
    pub volume: u8,
    pub muted: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct VolumeState {
    pub percent: u8,
    pub muted: bool,
    pub left: u8,
    pub right: u8,
    pub peak: f32,
}

pub struct MasterControl(IAudioEndpointVolume);

impl MasterControl {
    pub fn set_volume(&self, value: u8) -> Result<(), String> {
        if value > 100 {
            return Err("volume must be 0..100".into());
        }
        unsafe {
            self.0
                .SetMasterVolumeLevelScalar(value as f32 / 100.0, &GUID::zeroed())
                .map_err(err)
        }
    }
}

pub struct MixerApp {
    pub process: String,
    pub pid: u32,
    pub volume: u8,
    controls: Vec<ISimpleAudioVolume>,
}

impl MixerApp {
    pub fn set_volume(&self, value: u8) -> Result<(), String> {
        if value > 100 {
            return Err("volume must be 0..100".into());
        }
        for control in &self.controls {
            unsafe {
                control
                    .SetMasterVolume(value as f32 / 100.0, &GUID::zeroed())
                    .map_err(err)?;
            }
        }
        Ok(())
    }
}

pub struct Audio {
    enumerator: IMMDeviceEnumerator,
}

impl Audio {
    pub fn new() -> Result<Self, String> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .map_err(err)?;
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(err)?;
            Ok(Self { enumerator })
        }
    }

    pub fn devices(&self, flow: EDataFlow) -> Result<Vec<Device>, String> {
        unsafe {
            let collection = self
                .enumerator
                .EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE)
                .map_err(err)?;
            let default_id = self
                .default_device(flow)
                .ok()
                .map(|device| device_id(&device))
                .transpose()?
                .unwrap_or_default();
            let mut result = Vec::new();
            for i in 0..collection.GetCount().map_err(err)? {
                let device = collection.Item(i).map_err(err)?;
                let id = device_id(&device)?;
                let name = device_name(&device).unwrap_or_else(|_| id.clone());
                result.push(Device {
                    is_default: id == default_id,
                    id,
                    name,
                });
            }
            Ok(result)
        }
    }

    pub fn default_device(&self, flow: EDataFlow) -> Result<IMMDevice, String> {
        unsafe {
            self.enumerator
                .GetDefaultAudioEndpoint(flow, eMultimedia)
                .map_err(err)
        }
    }

    pub fn current_device_id(&self, flow: EDataFlow) -> Result<String, String> {
        device_id(&self.default_device(flow)?)
    }

    fn endpoint(&self, id: Option<&str>, flow: EDataFlow) -> Result<IAudioEndpointVolume, String> {
        let device = match id {
            Some(id) => unsafe {
                self.enumerator
                    .GetDevice(PCWSTR(wide(id).as_ptr()))
                    .map_err(err)?
            },
            None => self.default_device(flow)?,
        };
        unsafe { device.Activate(CLSCTX_ALL, None).map_err(err) }
    }

    pub fn state(&self, id: Option<&str>, flow: EDataFlow) -> Result<VolumeState, String> {
        let endpoint = self.endpoint(id, flow)?;
        unsafe {
            let pct = percent(endpoint.GetMasterVolumeLevelScalar().map_err(err)?);
            let muted = endpoint.GetMute().map_err(err)?.as_bool();
            let count = endpoint.GetChannelCount().map_err(err)?;
            let left = if count > 0 {
                percent(endpoint.GetChannelVolumeLevelScalar(0).map_err(err)?)
            } else {
                pct
            };
            let right = if count > 1 {
                percent(endpoint.GetChannelVolumeLevelScalar(1).map_err(err)?)
            } else {
                left
            };
            let device = match id {
                Some(id) => self
                    .enumerator
                    .GetDevice(PCWSTR(wide(id).as_ptr()))
                    .map_err(err)?,
                None => self.default_device(flow)?,
            };
            let peak = device
                .Activate::<IAudioMeterInformation>(CLSCTX_ALL, None)
                .ok()
                .and_then(|meter| meter.GetPeakValue().ok())
                .unwrap_or(0.0);
            Ok(VolumeState {
                percent: pct,
                muted,
                left,
                right,
                peak,
            })
        }
    }

    pub fn master_control(&self, id: &str) -> Result<MasterControl, String> {
        self.endpoint(Some(id), eRender).map(MasterControl)
    }

    pub fn mixer_apps_on_device(&self, device_id: &str) -> Result<Vec<MixerApp>, String> {
        let endpoint = unsafe {
            self.enumerator
                .GetDevice(PCWSTR(wide(device_id).as_ptr()))
                .map_err(err)?
        };
        let manager: IAudioSessionManager2 =
            unsafe { endpoint.Activate(CLSCTX_ALL, None).map_err(err)? };
        let enumerator = unsafe { manager.GetSessionEnumerator().map_err(err)? };
        let mut apps: Vec<MixerApp> = Vec::new();
        let mut by_name: HashMap<String, usize> = HashMap::new();
        for index in 0..unsafe { enumerator.GetCount().map_err(err)? } {
            let control = unsafe { enumerator.GetSession(index).map_err(err)? };
            let control2: IAudioSessionControl2 = match control.cast() {
                Ok(value) => value,
                Err(_) => continue,
            };
            let pid = unsafe { control2.GetProcessId().map_err(err)? };
            if pid == 0 {
                continue;
            }
            let process = process_name(pid).unwrap_or_else(|| format!("PID {pid}"));
            let volume: ISimpleAudioVolume = match control.cast() {
                Ok(value) => value,
                Err(_) => continue,
            };
            let key = process.to_lowercase();
            if let Some(&position) = by_name.get(&key) {
                apps[position].controls.push(volume);
            } else {
                let level = unsafe { volume.GetMasterVolume().map_err(err)? };
                by_name.insert(key, apps.len());
                apps.push(MixerApp {
                    process,
                    pid,
                    volume: percent(level),
                    controls: vec![volume],
                });
            }
        }
        apps.sort_by_key(|app| app.process.to_lowercase());
        Ok(apps)
    }

    pub fn set_volume(&self, id: Option<&str>, flow: EDataFlow, value: u8) -> Result<(), String> {
        if value > 100 {
            return Err("volume must be 0..100".into());
        }
        unsafe {
            self.endpoint(id, flow)?
                .SetMasterVolumeLevelScalar(value as f32 / 100.0, &GUID::zeroed())
                .map_err(err)
        }
    }

    pub fn change_volume(&self, delta: i16) -> Result<u8, String> {
        let old = self.state(None, eRender)?;
        let value = (old.percent as i16 + delta).clamp(0, 100) as u8;
        self.set_volume(None, eRender, value)?;
        if old.muted && delta > 0 {
            self.set_mute(None, eRender, false)?;
        }
        Ok(value)
    }

    pub fn set_mute(&self, id: Option<&str>, flow: EDataFlow, muted: bool) -> Result<(), String> {
        unsafe {
            self.endpoint(id, flow)?
                .SetMute(muted, &GUID::zeroed())
                .map_err(err)
        }
    }

    pub fn toggle_mute(&self, flow: EDataFlow) -> Result<bool, String> {
        let muted = !self.state(None, flow)?.muted;
        self.set_mute(None, flow, muted)?;
        Ok(muted)
    }

    pub fn set_balance(&self, left: u8, right: u8) -> Result<(), String> {
        if left > 100 || right > 100 {
            return Err("balance values must be 0..100".into());
        }
        let endpoint = self.endpoint(None, eRender)?;
        unsafe {
            if endpoint.GetChannelCount().map_err(err)? < 2 {
                return Err("selected device has fewer than two channels".into());
            }
            endpoint
                .SetChannelVolumeLevelScalar(0, left as f32 / 100.0, &GUID::zeroed())
                .map_err(err)?;
            endpoint
                .SetChannelVolumeLevelScalar(1, right as f32 / 100.0, &GUID::zeroed())
                .map_err(err)
        }
    }

    pub fn set_default_device(&self, id: &str, flow: EDataFlow) -> Result<(), String> {
        if !self.devices(flow)?.iter().any(|d| d.id == id) {
            return Err(format!("active device not found: {id}"));
        }
        set_default_endpoint(id)
    }

    pub fn app_sessions(&self) -> Result<Vec<AppSession>, String> {
        let mut sessions = Vec::new();
        for device in self.devices(eRender)? {
            let endpoint = unsafe {
                self.enumerator
                    .GetDevice(PCWSTR(wide(&device.id).as_ptr()))
                    .map_err(err)?
            };
            let manager: IAudioSessionManager2 =
                unsafe { endpoint.Activate(CLSCTX_ALL, None).map_err(err)? };
            let enumerator = unsafe { manager.GetSessionEnumerator().map_err(err)? };
            let count = unsafe { enumerator.GetCount().map_err(err)? };
            for index in 0..count {
                let control = unsafe { enumerator.GetSession(index).map_err(err)? };
                let control2: IAudioSessionControl2 = match control.cast() {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let pid = unsafe { control2.GetProcessId().map_err(err)? };
                if pid == 0 {
                    continue;
                }
                let process = process_name(pid).unwrap_or_else(|| format!("PID {pid}"));
                let volume: ISimpleAudioVolume = match control.cast() {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let level = unsafe { volume.GetMasterVolume().map_err(err)? };
                let muted = unsafe { volume.GetMute().map_err(err)?.as_bool() };
                sessions.push(AppSession {
                    process,
                    pid,
                    device_id: device.id.clone(),
                    volume: percent(level),
                    muted,
                });
            }
        }
        Ok(sessions)
    }

    pub fn set_app_volume(&self, name: &str, value: u8) -> Result<usize, String> {
        self.change_app(name, Some(value), None)
    }

    pub fn set_app_mute(&self, name: &str, value: bool) -> Result<usize, String> {
        self.change_app(name, None, Some(value))
    }

    fn change_app(
        &self,
        name: &str,
        level: Option<u8>,
        mute: Option<bool>,
    ) -> Result<usize, String> {
        if level.is_some_and(|v| v > 100) {
            return Err("volume must be 0..100".into());
        }
        let mut changed = 0;
        for device in self.devices(eRender)? {
            let endpoint = unsafe {
                self.enumerator
                    .GetDevice(PCWSTR(wide(&device.id).as_ptr()))
                    .map_err(err)?
            };
            let manager: IAudioSessionManager2 =
                unsafe { endpoint.Activate(CLSCTX_ALL, None).map_err(err)? };
            let enumerator = unsafe { manager.GetSessionEnumerator().map_err(err)? };
            for index in 0..unsafe { enumerator.GetCount().map_err(err)? } {
                let control = unsafe { enumerator.GetSession(index).map_err(err)? };
                let control2: IAudioSessionControl2 = match control.cast() {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let pid = unsafe { control2.GetProcessId().map_err(err)? };
                if !process_name(pid).is_some_and(|p| p.eq_ignore_ascii_case(name)) {
                    continue;
                }
                let volume: ISimpleAudioVolume = match control.cast() {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if let Some(level) = level {
                    unsafe {
                        volume
                            .SetMasterVolume(level as f32 / 100.0, &GUID::zeroed())
                            .map_err(err)?;
                    }
                }
                if let Some(mute) = mute {
                    unsafe {
                        volume.SetMute(mute, &GUID::zeroed()).map_err(err)?;
                    }
                }
                changed += 1;
            }
        }
        Ok(changed)
    }

    pub fn apply_profile(&self, profile: &Profile) -> Result<(), String> {
        if let Some(id) = &profile.output_device {
            self.set_default_device(id, eRender)?;
        }
        for (id, level) in &profile.device_volumes {
            if self.devices(eRender)?.iter().any(|device| device.id == *id) {
                self.set_volume(Some(id), eRender, *level)?;
            }
        }
        for (name, level) in &profile.application_volumes {
            self.set_app_volume(name, *level)?;
        }
        Ok(())
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

fn percent(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 100.0).round() as u8
}
fn err(error: windows::core::Error) -> String {
    error.to_string()
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn device_id(device: &IMMDevice) -> Result<String, String> {
    unsafe {
        let ptr = device.GetId().map_err(err)?;
        let result = ptr.to_string().map_err(|e| e.to_string());
        CoTaskMemFree(Some(ptr.0.cast()));
        result
    }
}

fn device_name(device: &IMMDevice) -> Result<String, String> {
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).map_err(err)?;
        let mut value = store.GetValue(&PKEY_Device_FriendlyName).map_err(err)?;
        let ptr = PropVariantToStringAlloc(&value).map_err(err)?;
        let result = ptr.to_string().map_err(|e| e.to_string());
        CoTaskMemFree(Some(ptr.0.cast()));
        let _ = PropVariantClear(&mut value);
        result
    }
}

pub fn process_path(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut path = vec![0u16; 32768];
        let mut len = path.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        if !ok {
            return None;
        }
        Some(String::from_utf16_lossy(&path[..len as usize]))
    }
}

fn process_name(pid: u32) -> Option<String> {
    let path = process_path(pid)?;
    Path::new(&path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

// Windows exposes no public desktop API for changing the default endpoint.
// Isolate the private IPolicyConfig call here; the rest uses documented Core Audio.
fn set_default_endpoint(id: &str) -> Result<(), String> {
    #[link(name = "ole32")]
    extern "system" {
        fn CoCreateInstance(
            clsid: *const GUID,
            outer: *mut c_void,
            context: u32,
            iid: *const GUID,
            object: *mut *mut c_void,
        ) -> i32;
    }
    let clsid = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
    let iid = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);
    let mut object: *mut c_void = std::ptr::null_mut();
    unsafe {
        let hr = CoCreateInstance(
            &clsid,
            std::ptr::null_mut(),
            CLSCTX_ALL.0 as u32,
            &iid,
            &mut object,
        );
        if hr < 0 {
            return Err(format!("PolicyConfig activation failed: HRESULT {hr:#x}"));
        }
        let vtable = *(object as *const *const usize);
        let set_default: unsafe extern "system" fn(*mut c_void, *const u16, i32) -> i32 =
            std::mem::transmute(*vtable.add(13));
        let release: unsafe extern "system" fn(*mut c_void) -> u32 =
            std::mem::transmute(*vtable.add(2));
        let name = wide(id);
        let mut failure = None;
        for role in [eConsole, eMultimedia, eCommunications] {
            let hr = set_default(object, name.as_ptr(), role.0);
            if hr < 0 {
                failure = Some(format!("SetDefaultEndpoint failed: HRESULT {hr:#x}"));
                break;
            }
        }
        release(object);
        failure.map_or(Ok(()), Err)
    }
}

pub fn output_flow() -> EDataFlow {
    eRender
}
pub fn input_flow() -> EDataFlow {
    eCapture
}
