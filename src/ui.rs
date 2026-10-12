use std::{
    cell::{Cell, RefCell},
    mem::size_of,
    path::PathBuf,
};

use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            CreateSolidBrush, DeleteObject, DrawTextW, FillRect, SetBkMode, SetTextColor,
            DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, HGDIOBJ, TRANSPARENT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_SELECTED, ODT_MENU},
            Shell::{
                ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD,
                NIM_DELETE, NOTIFYICONDATAW,
            },
            WindowsAndMessaging::{
                CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
                DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos,
                GetMenuItemCount, GetMessageW, GetWindowLongPtrW, InsertMenuItemW, LoadCursorW,
                MessageBoxW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW,
                SetForegroundWindow, SetMenuInfo, SetWindowLongPtrW, TrackPopupMenu,
                TranslateMessage, GWLP_USERDATA, HICON, IDC_ARROW, LR_DEFAULTCOLOR, MB_ICONERROR,
                MB_OK, MENUINFO, MENUITEMINFOW, MFS_DISABLED, MFT_OWNERDRAW, MIIM_DATA, MIIM_FTYPE,
                MIIM_ID, MIIM_STATE, MIIM_SUBMENU, MIM_BACKGROUND, MIM_STYLE, MNS_NOCHECK, MSG,
                SW_SHOWNORMAL, TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_APP, WM_DESTROY,
                WM_DRAWITEM, WM_LBUTTONUP, WM_MEASUREITEM, WM_RBUTTONUP, WNDCLASSEXW,
                WS_OVERLAPPED,
            },
        },
    },
};

use crate::{
    audio::{self, Audio},
    config::Config,
    panel::{self, Panel},
};

const TRAY_MESSAGE: u32 = WM_APP + 1;
const EXIT: u32 = 4;
const FIRST_DEVICE: u32 = 100;
const LEGACY_MIXER: u32 = 5;
const LEGACY_PLAYBACK: u32 = 6;
const LEGACY_RECORDING: u32 = 7;
const LEGACY_SOUNDS: u32 = 8;
const LEGACY_SOUND_SETTINGS: u32 = 9;
const LEGACY_MODERN_MIXER: u32 = 10;
const VOLUME_MIXER: u32 = 11;
const SETTINGS: u32 = 12;

#[derive(Clone, Copy)]
enum MenuStyle {
    Separator,
    Device,
    Action,
    Submenu,
    Disabled,
}

struct MenuEntry {
    label: Vec<u16>,
    style: MenuStyle,
    checked: bool,
}

struct TrayState {
    audio: Audio,
    config: RefCell<Config>,
    menu_open: Cell<bool>,
    panel: RefCell<Box<Panel>>,
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
        panel: RefCell::new(Box::new(Panel::new())),
        config_path,
        icon,
        taskbar_created,
    });

    let module = unsafe { GetModuleHandleW(None).map_err(error)? };
    panel::register_window_class(HINSTANCE(module.0))?;
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW).map_err(error)? };
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(window_proc),
        hInstance: HINSTANCE(module.0),
        hCursor: cursor,
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
    if message == WM_MEASUREITEM {
        let item = &mut *(lparam.0 as *mut MEASUREITEMSTRUCT);
        if item.CtlType == ODT_MENU && item.itemData != 0 {
            let entry = &*(item.itemData as *const MenuEntry);
            item.itemWidth = 296;
            item.itemHeight = match entry.style {
                MenuStyle::Separator => 12,
                MenuStyle::Device | MenuStyle::Action | MenuStyle::Submenu => 34,
                MenuStyle::Disabled => 33,
            };
            return LRESULT(1);
        }
    }
    if message == WM_DRAWITEM {
        let item = &*(lparam.0 as *const DRAWITEMSTRUCT);
        if item.CtlType == ODT_MENU && item.itemData != 0 {
            draw_menu_entry(item);
            return LRESULT(1);
        }
    }
    if message == TRAY_MESSAGE && matches!(lparam.0 as u32, WM_LBUTTONUP | WM_RBUTTONUP) {
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut TrayState;
        if !state.is_null() {
            let result = if lparam.0 as u32 == WM_LBUTTONUP {
                (*state).toggle_panel()
            } else {
                (*state).open_menu(hwnd)
            };
            if let Err(e) = result {
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
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut TrayState;
        if !state.is_null() {
            (*state).panel.borrow_mut().close();
        }
        let data = icon_data(hwnd, HICON::default());
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        PostQuitMessage(0);
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, message, wparam, lparam)
}

impl TrayState {
    fn toggle_panel(&self) -> Result<(), String> {
        let hwnd = {
            let mut panel = self.panel.borrow_mut();
            if panel.is_open() {
                panel.close();
                return Ok(());
            }
            panel.open(&self.audio, self.icon)?
        };
        Panel::show(hwnd);
        Ok(())
    }

    fn open_menu(&self, hwnd: HWND) -> Result<(), String> {
        self.panel.borrow_mut().close();
        if self.menu_open.replace(true) {
            return Ok(());
        }
        let result = self.show_menu(hwnd);
        self.menu_open.set(false);
        result
    }

    fn show_menu(&self, hwnd: HWND) -> Result<(), String> {
        *self.config.borrow_mut() = Config::load(&self.config_path)?;
        let mut devices = self.audio.devices(audio::output_flow())?;
        devices.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        });
        let menu = unsafe { CreatePopupMenu().map_err(error)? };
        let background = unsafe { CreateSolidBrush(rgb(27, 26, 34)) };
        if background.0.is_null() {
            unsafe {
                let _ = DestroyMenu(menu);
            }
            return Err(std::io::Error::last_os_error().to_string());
        }
        let menu_info = MENUINFO {
            cbSize: size_of::<MENUINFO>() as u32,
            fMask: MIM_BACKGROUND | MIM_STYLE,
            dwStyle: MNS_NOCHECK,
            hbrBack: background,
            ..Default::default()
        };
        let mut entries: Vec<Box<MenuEntry>> = Vec::new();
        let result = (|| {
            unsafe {
                SetMenuInfo(menu, &menu_info).map_err(error)?;
            }
            if devices.is_empty() {
                menu_item(
                    menu,
                    &mut entries,
                    MenuStyle::Disabled,
                    0,
                    "No devices found",
                    false,
                )?;
            }
            for (index, device) in devices.iter().enumerate() {
                menu_item(
                    menu,
                    &mut entries,
                    MenuStyle::Device,
                    FIRST_DEVICE + index as u32,
                    &device.name,
                    device.is_default,
                )?;
            }
            if let Some(saved) = &self.config.borrow().active().output_device {
                if !devices.iter().any(|device| &device.id == saved) {
                    menu_item(
                        menu,
                        &mut entries,
                        MenuStyle::Disabled,
                        0,
                        "Saved device is unavailable",
                        false,
                    )?;
                }
            }
            menu_item(menu, &mut entries, MenuStyle::Separator, 0, "", false)?;
            let legacy = unsafe { CreatePopupMenu().map_err(error)? };
            if let Err(e) = unsafe { SetMenuInfo(legacy, &menu_info).map_err(error) } {
                unsafe {
                    let _ = DestroyMenu(legacy);
                }
                return Err(e);
            }
            if let Err(e) = submenu_item(menu, &mut entries, legacy, "Windows Legacy") {
                unsafe {
                    let _ = DestroyMenu(legacy);
                }
                return Err(e);
            }
            for (id, label) in [
                (LEGACY_MIXER, "Volume Mixer"),
                (LEGACY_PLAYBACK, "Playback devices"),
                (LEGACY_RECORDING, "Recording devices"),
                (LEGACY_SOUNDS, "Sounds"),
                (LEGACY_SOUND_SETTINGS, "Sound settings"),
                (LEGACY_MODERN_MIXER, "Volume mixer"),
            ] {
                menu_item(legacy, &mut entries, MenuStyle::Action, id, label, false)?;
            }
            menu_item(menu, &mut entries, MenuStyle::Separator, 0, "", false)?;
            menu_item(
                menu,
                &mut entries,
                MenuStyle::Action,
                VOLUME_MIXER,
                "Volume Mixer",
                false,
            )?;
            menu_item(
                menu,
                &mut entries,
                MenuStyle::Action,
                SETTINGS,
                "Settings",
                false,
            )?;
            menu_item(menu, &mut entries, MenuStyle::Action, EXIT, "Exit", false)?;

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
            let _ = DeleteObject(HGDIOBJ(background.0));
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
            LEGACY_MIXER => launch(hwnd, "SndVol.exe", "")?,
            LEGACY_PLAYBACK => launch(hwnd, "control.exe", "mmsys.cpl,,0")?,
            LEGACY_RECORDING => launch(hwnd, "control.exe", "mmsys.cpl,,1")?,
            LEGACY_SOUNDS => launch(hwnd, "control.exe", "mmsys.cpl,,2")?,
            LEGACY_SOUND_SETTINGS => launch(hwnd, "ms-settings:sound", "")?,
            LEGACY_MODERN_MIXER => launch(hwnd, "ms-settings:apps-volume", "")?,
            VOLUME_MIXER => launch(hwnd, "ms-settings:apps-volume", "")?,
            SETTINGS => launch(hwnd, "ms-settings:sound", "")?,
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
    entries: &mut Vec<Box<MenuEntry>>,
    style: MenuStyle,
    id: u32,
    label: &str,
    checked: bool,
) -> Result<(), String> {
    let entry = Box::new(MenuEntry {
        label: label.encode_utf16().collect(),
        style,
        checked,
    });
    let disabled = matches!(style, MenuStyle::Separator | MenuStyle::Disabled);
    let info = MENUITEMINFOW {
        cbSize: size_of::<MENUITEMINFOW>() as u32,
        fMask: MIIM_FTYPE | MIIM_ID | MIIM_DATA | MIIM_STATE,
        fType: MFT_OWNERDRAW,
        fState: if disabled {
            MFS_DISABLED
        } else {
            Default::default()
        },
        wID: id,
        dwItemData: (&*entry as *const MenuEntry) as usize,
        ..Default::default()
    };
    unsafe {
        InsertMenuItemW(menu, next_menu_position(menu)?, true, &info).map_err(error)?;
    }
    entries.push(entry);
    Ok(())
}

fn next_menu_position(menu: windows::Win32::UI::WindowsAndMessaging::HMENU) -> Result<u32, String> {
    let count = unsafe { GetMenuItemCount(Some(menu)) };
    if count < 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(count as u32)
    }
}

fn submenu_item(
    menu: windows::Win32::UI::WindowsAndMessaging::HMENU,
    entries: &mut Vec<Box<MenuEntry>>,
    submenu: windows::Win32::UI::WindowsAndMessaging::HMENU,
    label: &str,
) -> Result<(), String> {
    let entry = Box::new(MenuEntry {
        label: label.encode_utf16().collect(),
        style: MenuStyle::Submenu,
        checked: false,
    });
    let info = MENUITEMINFOW {
        cbSize: size_of::<MENUITEMINFOW>() as u32,
        fMask: MIIM_FTYPE | MIIM_ID | MIIM_DATA | MIIM_SUBMENU,
        fType: MFT_OWNERDRAW,
        hSubMenu: submenu,
        dwItemData: (&*entry as *const MenuEntry) as usize,
        ..Default::default()
    };
    unsafe {
        InsertMenuItemW(menu, next_menu_position(menu)?, true, &info).map_err(error)?;
    }
    entries.push(entry);
    Ok(())
}

fn launch(hwnd: HWND, target: &str, parameters: &str) -> Result<(), String> {
    let file = wide(target);
    let args = wide(parameters);
    let result = unsafe {
        ShellExecuteW(
            Some(hwnd),
            w!("open"),
            PCWSTR(file.as_ptr()),
            PCWSTR(args.as_ptr()),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    let code = result.0 as usize;
    if code <= 32 {
        Err(format!(
            "Cannot open {target}: ShellExecuteW returned {code}"
        ))
    } else {
        Ok(())
    }
}

unsafe fn draw_menu_entry(item: &DRAWITEMSTRUCT) {
    let entry = &*(item.itemData as *const MenuEntry);
    let hovered = (item.itemState.0 & ODS_SELECTED.0) != 0;
    let background = if hovered
        && matches!(
            entry.style,
            MenuStyle::Action | MenuStyle::Device | MenuStyle::Submenu
        ) {
        rgb(65, 47, 77)
    } else if entry.checked {
        rgb(44, 36, 53)
    } else {
        rgb(27, 26, 34)
    };
    let brush = CreateSolidBrush(background);
    FillRect(item.hDC, &item.rcItem, brush);
    let _ = DeleteObject(HGDIOBJ(brush.0));
    if matches!(entry.style, MenuStyle::Separator) {
        let y = (item.rcItem.top + item.rcItem.bottom) / 2;
        let line = RECT {
            left: item.rcItem.left + 18,
            top: y,
            right: item.rcItem.right - 18,
            bottom: y + 1,
        };
        let brush = CreateSolidBrush(rgb(58, 53, 65));
        FillRect(item.hDC, &line, brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
        return;
    }
    let _ = SetBkMode(item.hDC, TRANSPARENT);
    if entry.checked {
        let check: Vec<u16> = "✓".encode_utf16().collect();
        draw_menu_text(
            item,
            &check,
            item.rcItem.left + 15,
            item.rcItem.left + 37,
            rgb(195, 140, 229),
        );
    }
    let (left, color) = match entry.style {
        MenuStyle::Disabled => (42, rgb(143, 137, 151)),
        MenuStyle::Device => (42, rgb(246, 242, 249)),
        MenuStyle::Action | MenuStyle::Submenu => (42, rgb(246, 242, 249)),
        MenuStyle::Separator => unreachable!(),
    };
    draw_menu_text(
        item,
        &entry.label,
        item.rcItem.left + left,
        item.rcItem.right - 16,
        color,
    );
}

unsafe fn draw_menu_text(
    item: &DRAWITEMSTRUCT,
    text: &[u16],
    left: i32,
    right: i32,
    color: COLORREF,
) {
    let _ = SetTextColor(item.hDC, color);
    let mut rect = RECT {
        left,
        top: item.rcItem.top,
        right,
        bottom: item.rcItem.bottom,
    };
    let mut text = text.to_vec();
    DrawTextW(
        item.hDC,
        &mut text,
        &mut rect,
        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
}

fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF(u32::from(red) | (u32::from(green) << 8) | (u32::from(blue) << 16))
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
