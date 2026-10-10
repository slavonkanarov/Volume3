use std::{
    cell::{Cell, RefCell},
    mem::size_of,
    path::PathBuf,
};

use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Shell::{
                Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
                NOTIFYICONDATAW,
            },
            WindowsAndMessaging::{
                AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW,
                DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW,
                GetCursorPos, GetMessageW, GetWindowLongPtrW, MessageBoxW, PostQuitMessage,
                RegisterClassExW, RegisterWindowMessageW, SetForegroundWindow, SetWindowLongPtrW,
                TrackPopupMenu, TranslateMessage, GWLP_USERDATA, HICON, LR_DEFAULTCOLOR,
                MB_ICONERROR, MB_OK, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG,
                TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_APP, WM_DESTROY, WM_LBUTTONUP,
                WM_RBUTTONUP, WNDCLASSEXW, WS_OVERLAPPED,
            },
        },
    },
};

use crate::{
    audio::{self, Audio},
    config::Config,
};

const TRAY_MESSAGE: u32 = WM_APP + 1;
const VOLUME_UP: u32 = 1;
const VOLUME_DOWN: u32 = 2;
const MUTE: u32 = 3;
const EXIT: u32 = 4;
const FIRST_DEVICE: u32 = 100;

struct TrayState {
    audio: Audio,
    config: RefCell<Config>,
    menu_open: Cell<bool>,
    config_path: PathBuf,
    icon: HICON,
    taskbar_created: u32,
}

pub fn run_gui(config_path: PathBuf) -> Result<(), String> {
    let config = Config::load(&config_path)?;
    let audio = Audio::new()?;
    let icon = load_icon()?;
    let taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    let mut state = Box::new(TrayState {
        audio,
        config: RefCell::new(config),
        menu_open: Cell::new(false),
        config_path,
        icon,
        taskbar_created,
    });

    let module = unsafe { GetModuleHandleW(None).map_err(error)? };
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(window_proc),
        hInstance: HINSTANCE(module.0),
        lpszClassName: w!("Volume3TrayWindow"),
        ..Default::default()
    };
    if unsafe { RegisterClassExW(&class) } == 0 {
        return Err(format!(
            "RegisterClassExW: {}",
            std::io::Error::last_os_error()
        ));
    }
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("Volume3TrayWindow"),
            w!("Volume3"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(HINSTANCE(module.0)),
            None,
        )
        .map_err(error)?
    };
    unsafe {
        SetWindowLongPtrW(
            hwnd,
            GWLP_USERDATA,
            (&mut *state as *mut TrayState) as isize,
        );
    }

    let saved_device = state.config.borrow().active().output_device.clone();
    if let Some(saved) = &saved_device {
        if state
            .audio
            .devices(audio::output_flow())?
            .iter()
            .any(|d| &d.id == saved)
        {
            if let Err(e) = state.audio.set_default_device(saved, audio::output_flow()) {
                show_error(hwnd, &e);
            }
        }
    }
    if let Err(e) = add_icon(hwnd, state.icon) {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return Err(e);
    }

    let result = unsafe {
        let mut message = MSG::default();
        loop {
            let status = GetMessageW(&mut message, None, 0, 0).0;
            if status == -1 {
                break Err(std::io::Error::last_os_error().to_string());
            }
            if status == 0 {
                break Ok(());
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    };
    unsafe {
        let _ = DestroyIcon(state.icon);
    }
    result
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == TRAY_MESSAGE && matches!(lparam.0 as u32, WM_LBUTTONUP | WM_RBUTTONUP) {
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut TrayState;
        if !state.is_null() {
            if let Err(e) = (*state).open_menu(hwnd) {
                show_error(hwnd, &e);
            }
        }
        return LRESULT(0);
    }
    let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut TrayState;
    if !state.is_null() && message == (*state).taskbar_created {
        if let Err(e) = add_icon(hwnd, (*state).icon) {
            show_error(hwnd, &e);
        }
        return LRESULT(0);
    }
    if message == WM_DESTROY {
        let data = icon_data(hwnd, HICON::default());
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        PostQuitMessage(0);
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, message, wparam, lparam)
}

impl TrayState {
    fn open_menu(&self, hwnd: HWND) -> Result<(), String> {
        if self.menu_open.replace(true) {
            return Ok(());
        }
        let result = self.show_menu(hwnd);
        self.menu_open.set(false);
        result
    }

    fn show_menu(&self, hwnd: HWND) -> Result<(), String> {
        *self.config.borrow_mut() = Config::load(&self.config_path)?;
        let devices = self.audio.devices(audio::output_flow())?;
        let state = self.audio.state(None, audio::output_flow()).ok();
        let menu = unsafe { CreatePopupMenu().map_err(error)? };
        let result = (|| {
            let headline = match state {
                Some(v) => format!(
                    "Volume3  •  {}%{}",
                    v.percent,
                    if v.muted { "  •  muted" } else { "" }
                ),
                None => "Volume3  •  no active output".into(),
            };
            menu_item(menu, MF_STRING | MF_GRAYED, 0, &headline)?;
            menu_item(menu, MF_SEPARATOR, 0, "")?;
            menu_item(menu, MF_STRING | MF_GRAYED, 0, "Playback devices")?;
            if devices.is_empty() {
                menu_item(menu, MF_STRING | MF_GRAYED, 0, "No devices found")?;
            }
            for (index, device) in devices.iter().enumerate() {
                let flags = if device.is_default {
                    MF_STRING | MF_CHECKED
                } else {
                    MF_STRING
                };
                menu_item(menu, flags, FIRST_DEVICE + index as u32, &device.name)?;
            }
            if let Some(saved) = &self.config.borrow().active().output_device {
                if !devices.iter().any(|device| &device.id == saved) {
                    menu_item(
                        menu,
                        MF_STRING | MF_GRAYED,
                        0,
                        "Saved device is unavailable",
                    )?;
                }
            }
            menu_item(menu, MF_SEPARATOR, 0, "")?;
            menu_item(menu, MF_STRING, VOLUME_UP, "Volume +5%")?;
            menu_item(menu, MF_STRING, VOLUME_DOWN, "Volume -5%")?;
            menu_item(
                menu,
                MF_STRING,
                MUTE,
                if state.is_some_and(|v| v.muted) {
                    "Unmute"
                } else {
                    "Mute"
                },
            )?;
            menu_item(menu, MF_SEPARATOR, 0, "")?;
            menu_item(menu, MF_STRING, EXIT, "Exit")?;

            let mut point = POINT::default();
            unsafe {
                GetCursorPos(&mut point).map_err(error)?;
                let _ = SetForegroundWindow(hwnd);
                let chosen = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_RIGHTBUTTON,
                    point.x,
                    point.y,
                    None,
                    hwnd,
                    None,
                )
                .0 as u32;
                self.handle_choice(hwnd, chosen, &devices)
            }
        })();
        unsafe {
            let _ = DestroyMenu(menu);
        }
        result
    }

    fn handle_choice(
        &self,
        hwnd: HWND,
        choice: u32,
        devices: &[audio::Device],
    ) -> Result<(), String> {
        match choice {
            VOLUME_UP => {
                self.audio.change_volume(5)?;
            }
            VOLUME_DOWN => {
                self.audio.change_volume(-5)?;
            }
            MUTE => {
                self.audio.toggle_mute(audio::output_flow())?;
            }
            EXIT => unsafe {
                DestroyWindow(hwnd).map_err(error)?;
            },
            n if n >= FIRST_DEVICE => {
                if let Some(device) = devices.get((n - FIRST_DEVICE) as usize) {
                    self.audio
                        .set_default_device(&device.id, audio::output_flow())?;
                    let mut config = self.config.borrow_mut();
                    let active = config.active_profile.clone();
                    config.profiles.get_mut(&active).unwrap().output_device =
                        Some(device.id.clone());
                    config.save(&self.config_path)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn menu_item(
    menu: windows::Win32::UI::WindowsAndMessaging::HMENU,
    flags: windows::Win32::UI::WindowsAndMessaging::MENU_ITEM_FLAGS,
    id: u32,
    label: &str,
) -> Result<(), String> {
    let text = wide(label);
    unsafe { AppendMenuW(menu, flags, id as usize, PCWSTR(text.as_ptr())).map_err(error) }
}

fn add_icon(hwnd: HWND, icon: HICON) -> Result<(), String> {
    let mut data = icon_data(hwnd, icon);
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    let tip = wide("Volume3 • audio output and volume");
    let len = tip.len().min(data.szTip.len());
    data.szTip[..len].copy_from_slice(&tip[..len]);
    if unsafe { Shell_NotifyIconW(NIM_ADD, &data).as_bool() } {
        Ok(())
    } else {
        Err(format!(
            "Shell_NotifyIconW: {}",
            std::io::Error::last_os_error()
        ))
    }
}

fn icon_data(hwnd: HWND, icon: HICON) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uCallbackMessage: TRAY_MESSAGE,
        hIcon: icon,
        ..Default::default()
    }
}

fn load_icon() -> Result<HICON, String> {
    let ico = include_bytes!("../assets/volume3.ico");
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    for index in 0..count {
        let entry = 6 + index * 16;
        if ico[entry] != 32 {
            continue;
        }
        let size = u32::from_le_bytes(ico[entry + 8..entry + 12].try_into().unwrap()) as usize;
        let offset = u32::from_le_bytes(ico[entry + 12..entry + 16].try_into().unwrap()) as usize;
        let image = ico
            .get(offset..offset + size)
            .ok_or("invalid embedded icon")?;
        return unsafe {
            CreateIconFromResourceEx(image, true, 0x0003_0000, 32, 32, LR_DEFAULTCOLOR)
                .map_err(error)
        };
    }
    Err("embedded icon has no 32 px image".into())
}

fn show_error(hwnd: HWND, message: &str) {
    let text = wide(message);
    unsafe {
        MessageBoxW(
            Some(hwnd),
            PCWSTR(text.as_ptr()),
            w!("Volume3"),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn error(error: windows::core::Error) -> String {
    error.to_string()
}
