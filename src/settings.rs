mod devices;

use std::{cell::RefCell, mem::size_of, path::PathBuf, rc::Rc};

use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::{
            Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE},
            Gdi::{
                BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateSolidBrush,
                DeleteDC, DeleteObject, EndPaint, GetMonitorInfoW, InvalidateRect,
                MonitorFromPoint, SelectObject, SetBkMode, SetTextColor, FW_NORMAL, FW_SEMIBOLD,
                HBRUSH, HDC, HFONT, HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
                SRCCOPY, TRANSPARENT,
            },
        },
        UI::{
            Controls::{SetWindowTheme, DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_SELECTED},
            Input::KeyboardAndMouse::{EnableWindow, GetFocus, VK_ESCAPE, VK_RETURN, VK_SPACE},
            Shell::SetWindowSubclass,
            WindowsAndMessaging::{
                AdjustWindowRectEx, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect,
                GetCursorPos, GetDlgCtrlID, GetDlgItem, GetWindowLongPtrW, GetWindowTextLengthW,
                GetWindowTextW, LoadCursorW, PostMessageW, RegisterClassExW, SendMessageW,
                SetForegroundWindow, SetWindowLongPtrW, SetWindowTextW, ShowWindow, BM_SETCHECK,
                BS_AUTOCHECKBOX, BS_OWNERDRAW, CBN_SELCHANGE, CBS_DROPDOWNLIST, CBS_HASSTRINGS,
                CBS_OWNERDRAWFIXED, CB_ADDSTRING, CB_GETCURSEL, CB_RESETCONTENT, CB_SETCURSEL,
                ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY, GWLP_USERDATA, HICON, HMENU, IDC_ARROW,
                LBS_HASSTRINGS, LBS_NOINTEGRALHEIGHT, LBS_OWNERDRAWFIXED, LB_ADDSTRING,
                LB_GETTOPINDEX, LB_RESETCONTENT, LB_SETTOPINDEX, SW_HIDE, SW_RESTORE, SW_SHOW,
                WA_ACTIVE, WINDOW_EX_STYLE, WINDOW_STYLE, WM_ACTIVATE, WM_APP, WM_CLOSE,
                WM_COMMAND, WM_CTLCOLORBTN, WM_CTLCOLOREDIT, WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC,
                WM_DRAWITEM, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONUP, WM_MEASUREITEM, WM_MOUSEMOVE,
                WM_PAINT, WM_SETFONT, WNDCLASSEXW, WS_BORDER, WS_CAPTION, WS_CHILD,
                WS_CLIPCHILDREN, WS_EX_APPWINDOW, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU,
                WS_TABSTOP, WS_VSCROLL,
            },
        },
    },
};

use crate::{
    audio::{self, Audio, Device},
    config::Config,
    startup,
    theme::{self, circle, draw_text, fill, font, rgb, rounded},
    updates::Updater,
    BUILD_VERSION,
};

const CLIENT_WIDTH: i32 = 860;
const CLIENT_HEIGHT: i32 = 620;
const SIDEBAR_WIDTH: i32 = 210;
const CONTENT_LEFT: i32 = 238;
const CARD_TOP: i32 = 150;
const CARD_BOTTOM: i32 = 248;
pub(crate) const CHECK_UPDATES: u32 = 13;
pub(crate) const AUTO_UPDATES: u32 = 15;
pub(crate) const PRERELEASE_UPDATES: u32 = 16;
pub(crate) const CHECK_AUTOMATICALLY: u32 = 17;
pub(crate) const CANCEL_UPDATE: u32 = 18;
pub(crate) const INSTALL_SELECTED: u32 = 19;
const SELECT_VERSION: u32 = 20;
const RELEASE_NOTES: u32 = 21;
const UPDATE_STATUS: u32 = 22;
const OUTPUT_DEVICES: u32 = 23;
const TOGGLE_DEVICE: u32 = WM_APP + 2;
const PAGE_LABELS: [&str; 3] = ["General", "Output devices", "Updates"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    General,
    OutputDevices,
    Updates,
}

pub struct SettingsWindow {
    hwnd: HWND,
    config_path: PathBuf,
    autostart_enabled: bool,
    hovered: bool,
    title_font: HFONT,
    label_font: HFONT,
    body_font: HFONT,
    background: HBRUSH,
    owner: HWND,
    page: Page,
    controls: Vec<HWND>,
    versions: Vec<String>,
    devices: Vec<Device>,
    hovered_device: Option<usize>,
    config: Rc<RefCell<Config>>,
    updater: Rc<RefCell<Updater>>,
}

impl SettingsWindow {
    pub(crate) fn new(
        config_path: PathBuf,
        config: Rc<RefCell<Config>>,
        updater: Rc<RefCell<Updater>>,
    ) -> Self {
        Self {
            hwnd: HWND::default(),
            config_path,
            autostart_enabled: false,
            hovered: false,
            title_font: font(25, FW_SEMIBOLD.0 as i32),
            label_font: font(17, FW_SEMIBOLD.0 as i32),
            body_font: font(14, FW_NORMAL.0 as i32),
            background: unsafe { CreateSolidBrush(rgb(20, 24, 32)) },
            owner: HWND::default(),
            page: Page::General,
            controls: Vec::new(),
            versions: Vec::new(),
            devices: Vec::new(),
            hovered_device: None,
            config,
            updater,
        }
    }

    pub fn open(&mut self, owner: HWND) -> Result<HWND, String> {
        self.owner = owner;
        self.autostart_enabled = startup::is_enabled(&self.config_path)?;
        if !self.hwnd.0.is_null() {
            self.refresh_devices()?;
            unsafe {
                let _ = InvalidateRect(Some(self.hwnd), None, false);
            }
            return Ok(self.hwnd);
        }
        let module = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }
            .map_err(|error| error.to_string())?;
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
        let ex_style = WS_EX_APPWINDOW;
        let mut bounds = RECT {
            left: 0,
            top: 0,
            right: CLIENT_WIDTH,
            bottom: CLIENT_HEIGHT,
        };
        unsafe { AdjustWindowRectEx(&mut bounds, style, false, ex_style) }
            .map_err(|error| error.to_string())?;
        let width = bounds.right - bounds.left;
        let height = bounds.bottom - bounds.top;
        let mut cursor = POINT::default();
        unsafe { GetCursorPos(&mut cursor) }.map_err(|error| error.to_string())?;
        let monitor = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let work = info.rcWork;
        let x = work.left + (work.right - work.left - width) / 2;
        let y = work.top + (work.bottom - work.top - height) / 2;
        let hwnd = unsafe {
            CreateWindowExW(
                ex_style,
                w!("Volume3SettingsWindow"),
                w!("Volume3 Settings"),
                style,
                x,
                y,
                width,
                height,
                None,
                None,
                Some(HINSTANCE(module.0)),
                None,
            )
        }
        .map_err(|error| error.to_string())?;
        self.hwnd = hwnd;
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, self as *mut SettingsWindow as isize);
            let dark = 1i32;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&dark as *const i32).cast(),
                size_of::<i32>() as u32,
            );
        }
        if let Err(error) = self.create_controls().and_then(|()| self.refresh_devices()) {
            self.close();
            return Err(error);
        }
        self.select_page(self.page);
        Ok(hwnd)
    }

    pub fn is_open(&self) -> bool {
        !self.hwnd.0.is_null()
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    fn create_controls(&mut self) -> Result<(), String> {
        for (id, class, text, x, y, width, height, style) in [
            (
                CHECK_AUTOMATICALLY,
                w!("BUTTON"),
                "Check for updates automatically",
                CONTENT_LEFT,
                112,
                580,
                28,
                BS_AUTOCHECKBOX as u32,
            ),
            (
                AUTO_UPDATES,
                w!("BUTTON"),
                "Download, install, and restart automatically",
                CONTENT_LEFT,
                146,
                580,
                28,
                BS_AUTOCHECKBOX as u32,
            ),
            (
                PRERELEASE_UPDATES,
                w!("BUTTON"),
                "Include prereleases",
                CONTENT_LEFT,
                180,
                580,
                28,
                BS_AUTOCHECKBOX as u32,
            ),
            (
                CHECK_UPDATES,
                w!("BUTTON"),
                "Check for updates",
                CONTENT_LEFT,
                222,
                180,
                30,
                BS_OWNERDRAW as u32,
            ),
            (
                CANCEL_UPDATE,
                w!("BUTTON"),
                "Cancel",
                CONTENT_LEFT + 194,
                222,
                100,
                30,
                BS_OWNERDRAW as u32,
            ),
            (
                UPDATE_STATUS,
                w!("STATIC"),
                "",
                CONTENT_LEFT,
                263,
                596,
                43,
                0,
            ),
            (
                SELECT_VERSION,
                w!("COMBOBOX"),
                "",
                CONTENT_LEFT,
                338,
                352,
                240,
                (CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED | CBS_HASSTRINGS) as u32 | WS_VSCROLL.0,
            ),
            (
                INSTALL_SELECTED,
                w!("BUTTON"),
                "Install selected version",
                CONTENT_LEFT + 366,
                336,
                230,
                30,
                BS_OWNERDRAW as u32,
            ),
            (
                RELEASE_NOTES,
                w!("EDIT"),
                "",
                CONTENT_LEFT,
                381,
                596,
                110,
                ES_MULTILINE as u32
                    | ES_READONLY as u32
                    | ES_AUTOVSCROLL as u32
                    | WS_VSCROLL.0
                    | WS_BORDER.0,
            ),
            (
                OUTPUT_DEVICES,
                w!("LISTBOX"),
                "Output devices",
                CONTENT_LEFT,
                120,
                596,
                420,
                (LBS_OWNERDRAWFIXED | LBS_HASSTRINGS | LBS_NOINTEGRALHEIGHT) as u32 | WS_VSCROLL.0,
            ),
        ] {
            let text: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
            let control = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    class,
                    PCWSTR(text.as_ptr()),
                    WS_CHILD | WS_TABSTOP | WINDOW_STYLE(style),
                    x,
                    y,
                    width,
                    height,
                    Some(self.hwnd),
                    Some(HMENU(id as *mut _)),
                    None,
                    None,
                )
            }
            .map_err(|error| error.to_string())?;
            self.controls.push(control);
            unsafe {
                SendMessageW(
                    control,
                    WM_SETFONT,
                    Some(WPARAM(self.body_font.0 as usize)),
                    Some(LPARAM(1)),
                );
                match id {
                    CHECK_AUTOMATICALLY | AUTO_UPDATES | PRERELEASE_UPDATES => {
                        theme::style_control(control, 1)
                    }
                    SELECT_VERSION => theme::style_control(control, 2),
                    CHECK_UPDATES | CANCEL_UPDATE | INSTALL_SELECTED => {
                        theme::style_control(control, 0)
                    }
                    OUTPUT_DEVICES => {
                        let _ = SetWindowSubclass(
                            control,
                            Some(devices::device_cards_proc),
                            2,
                            self.hwnd.0 as usize,
                        );
                    }
                    _ => {}
                }
                let _ = SetWindowTheme(control, w!("DarkMode_Explorer"), None);
            }
        }
        Ok(())
    }

    fn refresh_devices(&mut self) -> Result<(), String> {
        let mut devices = Audio::new()?.devices(audio::output_flow())?;
        devices.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        });
        unsafe {
            let control = GetDlgItem(Some(self.hwnd), OUTPUT_DEVICES as i32)
                .map_err(|error| error.to_string())?;
            let top = SendMessageW(control, LB_GETTOPINDEX, None, None);
            self.devices = devices;
            self.hovered_device = None;
            SendMessageW(control, LB_RESETCONTENT, None, None);
            for device in &self.devices {
                let text: Vec<u16> = device.name.encode_utf16().chain(Some(0)).collect();
                if SendMessageW(
                    control,
                    LB_ADDSTRING,
                    None,
                    Some(LPARAM(text.as_ptr() as isize)),
                )
                .0 < 0
                {
                    return Err("Cannot display output devices".into());
                }
            }
            SendMessageW(
                control,
                LB_SETTOPINDEX,
                Some(WPARAM(top.0.max(0) as usize)),
                None,
            );
            let _ = ShowWindow(
                control,
                if self.page == Page::OutputDevices && !self.devices.is_empty() {
                    SW_SHOW
                } else {
                    SW_HIDE
                },
            );
            let _ = InvalidateRect(Some(control), None, false);
        }
        Ok(())
    }

    fn set_device_visible(&self, index: usize, visible: bool) -> Result<(), String> {
        let Some(device) = self.devices.get(index) else {
            return Ok(());
        };
        let mut config = self.config.borrow().clone();
        if visible {
            config.hidden_output_devices.remove(&device.id);
        } else {
            config.hidden_output_devices.insert(device.id.clone());
        }
        config.save(&self.config_path)?;
        *self.config.borrow_mut() = config;
        Ok(())
    }

    fn select_page(&mut self, page: Page) {
        self.page = page;
        unsafe {
            for control in &self.controls {
                let control_page = if GetDlgCtrlID(*control) == OUTPUT_DEVICES as i32 {
                    Page::OutputDevices
                } else {
                    Page::Updates
                };
                let _ = ShowWindow(
                    *control,
                    if page == control_page
                        && (page != Page::OutputDevices || !self.devices.is_empty())
                    {
                        SW_SHOW
                    } else {
                        SW_HIDE
                    },
                );
            }
        }
        self.refresh();
    }

    pub fn refresh(&mut self) {
        if !self.is_open() {
            return;
        }
        let updater = self.updater.borrow();
        let config = self.config.borrow();
        unsafe {
            for (id, enabled) in [
                (CHECK_AUTOMATICALLY, config.updates.check_automatically),
                (AUTO_UPDATES, config.updates.automatic_install),
                (PRERELEASE_UPDATES, config.updates.include_prereleases),
            ] {
                if let Ok(control) = GetDlgItem(Some(self.hwnd), id as i32) {
                    SendMessageW(
                        control,
                        BM_SETCHECK,
                        Some(WPARAM(usize::from(enabled))),
                        None,
                    );
                    let _ = EnableWindow(
                        control,
                        updater.supported
                            && (id != AUTO_UPDATES || config.updates.check_automatically),
                    );
                }
            }
            for (id, enabled) in [
                (CHECK_UPDATES, updater.supported && !updater.busy()),
                (
                    CANCEL_UPDATE,
                    updater.busy() || updater.downloaded.is_some(),
                ),
                (
                    INSTALL_SELECTED,
                    !updater.busy()
                        && updater
                            .releases
                            .get(updater.selected)
                            .is_some_and(|release| {
                                config.updates.include_prereleases || release.version.pre.is_empty()
                            }),
                ),
                (SELECT_VERSION, !updater.busy()),
            ] {
                if let Ok(control) = GetDlgItem(Some(self.hwnd), id as i32) {
                    let _ = EnableWindow(control, enabled);
                }
            }
            let versions: Vec<_> = updater
                .releases
                .iter()
                .map(|release| {
                    if config.updates.include_prereleases || release.version.pre.is_empty() {
                        release.version.to_string()
                    } else {
                        String::new()
                    }
                })
                .collect();
            if self.versions != versions {
                if let Ok(control) = GetDlgItem(Some(self.hwnd), SELECT_VERSION as i32) {
                    SendMessageW(control, CB_RESETCONTENT, None, None);
                    for version in versions.iter().filter(|version| !version.is_empty()) {
                        let text: Vec<u16> = version.encode_utf16().chain(Some(0)).collect();
                        SendMessageW(
                            control,
                            CB_ADDSTRING,
                            None,
                            Some(LPARAM(text.as_ptr() as isize)),
                        );
                    }
                }
                self.versions = versions;
            }
            if let Ok(control) = GetDlgItem(Some(self.hwnd), SELECT_VERSION as i32) {
                let index = self
                    .versions
                    .iter()
                    .take(updater.selected)
                    .filter(|version| !version.is_empty())
                    .count();
                SendMessageW(control, CB_SETCURSEL, Some(WPARAM(index)), None);
            }
            set_text(self.hwnd, UPDATE_STATUS, &updater.progress());
            let notes = updater.releases.get(updater.selected).map_or(
                "Check for updates to load available versions and release notes.",
                |release| release.notes.as_str(),
            );
            set_text(self.hwnd, RELEASE_NOTES, notes);
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    pub fn show(hwnd: HWND) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
    }

    pub fn close(&mut self) {
        if !self.hwnd.0.is_null() {
            let hwnd = self.hwnd;
            self.hwnd = HWND::default();
            self.controls.clear();
            self.versions.clear();
            self.devices.clear();
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let _ = DestroyWindow(hwnd);
            }
        }
    }

    fn toggle(&mut self) -> Result<(), String> {
        let enabled = startup::is_enabled(&self.config_path)?;
        startup::set_enabled(&self.config_path, !enabled)?;
        self.autostart_enabled = !enabled;
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
        Ok(())
    }

    fn paint(&self, hwnd: HWND) {
        unsafe {
            let mut paint = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut paint);
            let mut client = RECT::default();
            if GetClientRect(hwnd, &mut client).is_ok() {
                let width = client.right - client.left;
                let height = client.bottom - client.top;
                let buffer = CreateCompatibleDC(Some(hdc));
                let bitmap = CreateCompatibleBitmap(hdc, width, height);
                if !buffer.0.is_null() && !bitmap.0.is_null() {
                    let old = SelectObject(buffer, HGDIOBJ(bitmap.0));
                    self.draw(buffer, client);
                    let _ = BitBlt(hdc, 0, 0, width, height, Some(buffer), 0, 0, SRCCOPY);
                    let _ = SelectObject(buffer, old);
                } else {
                    self.draw(hdc, client);
                }
                if !bitmap.0.is_null() {
                    let _ = DeleteObject(HGDIOBJ(bitmap.0));
                }
                if !buffer.0.is_null() {
                    let _ = DeleteDC(buffer);
                }
            }
            let _ = EndPaint(hwnd, &paint);
        }
    }

    unsafe fn draw(&self, hdc: HDC, client: RECT) {
        fill(hdc, client, rgb(20, 24, 32));
        fill(
            hdc,
            RECT {
                right: SIDEBAR_WIDTH,
                ..client
            },
            rgb(29, 35, 46),
        );
        fill(
            hdc,
            RECT {
                top: client.bottom - 68,
                ..client
            },
            rgb(29, 35, 46),
        );
        let _ = SetBkMode(hdc, TRANSPARENT);
        draw_text(
            hdc,
            "Volume3",
            RECT {
                left: 20,
                top: 20,
                right: SIDEBAR_WIDTH - 20,
                bottom: 54,
            },
            self.title_font,
            rgb(242, 246, 250),
        );
        draw_text(
            hdc,
            "SETTINGS",
            RECT {
                left: 21,
                top: 58,
                right: SIDEBAR_WIDTH - 20,
                bottom: 82,
            },
            self.body_font,
            rgb(153, 167, 187),
        );
        for (index, label) in PAGE_LABELS.iter().enumerate() {
            let top = 110 + index as i32 * 50;
            if self.page as usize == index {
                rounded(
                    hdc,
                    RECT {
                        left: 12,
                        top,
                        right: SIDEBAR_WIDTH - 12,
                        bottom: top + 42,
                    },
                    rgb(38, 68, 88),
                    9,
                );
                fill(
                    hdc,
                    RECT {
                        left: 12,
                        top: top + 6,
                        right: 16,
                        bottom: top + 36,
                    },
                    rgb(48, 126, 164),
                );
            }
            draw_text(
                hdc,
                label,
                RECT {
                    left: 29,
                    top: top + 4,
                    right: SIDEBAR_WIDTH - 24,
                    bottom: top + 38,
                },
                self.label_font,
                rgb(242, 246, 250),
            );
        }
        draw_text(
            hdc,
            PAGE_LABELS[self.page as usize],
            RECT {
                left: CONTENT_LEFT,
                top: 29,
                right: client.right - 26,
                bottom: 66,
            },
            self.title_font,
            rgb(242, 246, 250),
        );
        if self.page == Page::Updates {
            for (text, top) in [
                (format!("Current version: {BUILD_VERSION}"), 68),
                ("Available versions and release notes".into(), 308),
                (
                    "Automatic installation waits until this window is closed.".into(),
                    500,
                ),
                (
                    "Protected folders and MSI updates require administrator approval.".into(),
                    522,
                ),
                (
                    "Older versions disable auto-install; data formats may differ.".into(),
                    544,
                ),
            ] {
                draw_text(
                    hdc,
                    &text,
                    RECT {
                        left: CONTENT_LEFT,
                        top,
                        right: client.right - 26,
                        bottom: top + 25,
                    },
                    self.body_font,
                    rgb(153, 167, 187),
                );
            }
        } else if self.page == Page::General {
            draw_text(
                hdc,
                "App behavior and startup.",
                RECT {
                    left: CONTENT_LEFT,
                    top: 68,
                    right: client.right - 26,
                    bottom: 96,
                },
                self.body_font,
                rgb(153, 167, 187),
            );
            draw_text(
                hdc,
                "STARTUP",
                RECT {
                    left: CONTENT_LEFT,
                    top: 117,
                    right: client.right - 26,
                    bottom: 145,
                },
                self.body_font,
                rgb(153, 167, 187),
            );
            rounded(
                hdc,
                RECT {
                    left: CONTENT_LEFT,
                    top: CARD_TOP,
                    right: client.right - 26,
                    bottom: CARD_BOTTOM,
                },
                if self.hovered {
                    rgb(41, 50, 64)
                } else {
                    rgb(29, 35, 46)
                },
                10,
            );
            draw_text(
                hdc,
                "Start with Windows",
                RECT {
                    left: CONTENT_LEFT + 20,
                    top: 166,
                    right: client.right - 120,
                    bottom: 196,
                },
                self.label_font,
                rgb(242, 246, 250),
            );
            draw_text(
                hdc,
                "Launch Volume3 when you sign in to Windows.",
                RECT {
                    left: CONTENT_LEFT + 20,
                    top: 196,
                    right: client.right - 120,
                    bottom: 226,
                },
                self.body_font,
                rgb(153, 167, 187),
            );
            let track_left = client.right - 104;
            rounded(
                hdc,
                RECT {
                    left: track_left,
                    top: 171,
                    right: track_left + 58,
                    bottom: 201,
                },
                if self.autostart_enabled {
                    rgb(48, 126, 164)
                } else {
                    rgb(91, 105, 123)
                },
                30,
            );
            circle(
                hdc,
                if self.autostart_enabled {
                    track_left + 32
                } else {
                    track_left + 4
                },
                175,
                22,
                rgb(255, 255, 255),
            );
            draw_text(
                hdc,
                if self.autostart_enabled { "On" } else { "Off" },
                RECT {
                    left: track_left + 18,
                    top: 203,
                    right: track_left + 58,
                    bottom: 229,
                },
                self.body_font,
                rgb(153, 167, 187),
            );
            draw_text(
                hdc,
                "Volume3 starts in the notification area.",
                RECT {
                    left: CONTENT_LEFT,
                    top: 271,
                    right: client.right - 26,
                    bottom: 301,
                },
                self.body_font,
                rgb(153, 167, 187),
            );
        } else {
            for (text, top) in [
                ("Choose which devices appear in the tray menu.", 68),
                (
                    "Turn off a card to hide its device. Your audio keeps playing.",
                    92,
                ),
            ] {
                draw_text(
                    hdc,
                    text,
                    RECT {
                        left: CONTENT_LEFT,
                        top,
                        right: client.right - 26,
                        bottom: top + 25,
                    },
                    self.body_font,
                    rgb(153, 167, 187),
                );
            }
            if self.devices.is_empty() {
                draw_text(
                    hdc,
                    "No output devices connected.",
                    RECT {
                        left: CONTENT_LEFT,
                        top: 160,
                        right: client.right - 26,
                        bottom: 190,
                    },
                    self.label_font,
                    rgb(153, 167, 187),
                );
            }
        }
        draw_text(
            hdc,
            &format!("Version {BUILD_VERSION}"),
            RECT {
                left: 21,
                top: client.bottom - 57,
                right: SIDEBAR_WIDTH - 20,
                bottom: client.bottom - 11,
            },
            self.body_font,
            rgb(153, 167, 187),
        );
        draw_text(
            hdc,
            "Changes are applied immediately.",
            RECT {
                left: CONTENT_LEFT,
                top: client.bottom - 57,
                right: client.right - 26,
                bottom: client.bottom - 11,
            },
            self.body_font,
            rgb(153, 167, 187),
        );
    }
}

impl Drop for SettingsWindow {
    fn drop(&mut self) {
        self.close();
        unsafe {
            for font in [self.title_font, self.label_font, self.body_font] {
                if !font.0.is_null() {
                    let _ = DeleteObject(HGDIOBJ(font.0));
                }
            }
            let _ = DeleteObject(HGDIOBJ(self.background.0));
        }
    }
}

pub fn register_window_class(module: HINSTANCE, icon: HICON) -> Result<(), String> {
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.map_err(|error| error.to_string())?;
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(window_proc),
        hInstance: module,
        hIcon: icon,
        hIconSm: icon,
        hCursor: cursor,
        lpszClassName: w!("Volume3SettingsWindow"),
        ..Default::default()
    };
    if unsafe { RegisterClassExW(&class) } == 0 {
        Err(format!(
            "RegisterClassExW(settings): {}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(())
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_ERASEBKGND {
        return LRESULT(1);
    }
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SettingsWindow;
    if pointer.is_null() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    match message {
        TOGGLE_DEVICE => {
            let window = &*pointer;
            if let Some(device) = window.devices.get(wparam.0) {
                let visible = window
                    .config
                    .borrow()
                    .hidden_output_devices
                    .contains(&device.id);
                if let Err(error) = window.set_device_visible(wparam.0, visible) {
                    theme::show_message(hwnd, "Settings error", &error);
                }
                if let Ok(control) = GetDlgItem(Some(hwnd), OUTPUT_DEVICES as i32) {
                    devices::invalidate_card(control, wparam.0);
                }
            }
            return LRESULT(0);
        }
        WM_MEASUREITEM if wparam.0 == OUTPUT_DEVICES as usize => {
            (*(lparam.0 as *mut MEASUREITEMSTRUCT)).itemHeight = 100;
            return LRESULT(1);
        }
        WM_MEASUREITEM if wparam.0 == SELECT_VERSION as usize => {
            (*(lparam.0 as *mut MEASUREITEMSTRUCT)).itemHeight = 28;
            return LRESULT(1);
        }
        WM_DRAWITEM => {
            let item = &*(lparam.0 as *const DRAWITEMSTRUCT);
            if item.CtlID == OUTPUT_DEVICES {
                (&*pointer).draw_device_card(item);
            } else if item.CtlID == SELECT_VERSION {
                fill(
                    item.hDC,
                    item.rcItem,
                    if item.itemState.0 & ODS_SELECTED.0 != 0 {
                        rgb(38, 68, 88)
                    } else {
                        rgb(29, 35, 46)
                    },
                );
                let _ = SetBkMode(item.hDC, TRANSPARENT);
                if let Some(version) = (&*pointer)
                    .versions
                    .iter()
                    .filter(|version| !version.is_empty())
                    .nth(item.itemID as usize)
                {
                    draw_text(
                        item.hDC,
                        version,
                        RECT {
                            left: item.rcItem.left + 12,
                            ..item.rcItem
                        },
                        (&*pointer).body_font,
                        rgb(242, 246, 250),
                    );
                }
            } else {
                theme::draw_button(
                    item,
                    matches!(item.CtlID, CHECK_UPDATES | INSTALL_SELECTED),
                    theme::BACKGROUND,
                );
            }
            return LRESULT(1);
        }
        WM_COMMAND => {
            let id = wparam.0 as u16 as u32;
            if id == 2 {
                (&mut *pointer).close();
                return LRESULT(0);
            }
            if id == 1 && (&*pointer).page == Page::General && GetFocus() == hwnd {
                if let Err(error) = (&mut *pointer).toggle() {
                    theme::show_message(hwnd, "Settings error", &error);
                }
                return LRESULT(0);
            }
            if !matches!(
                id,
                SELECT_VERSION
                    | CHECK_UPDATES
                    | AUTO_UPDATES
                    | PRERELEASE_UPDATES
                    | CHECK_AUTOMATICALLY
                    | CANCEL_UPDATE
                    | INSTALL_SELECTED
            ) {
                return LRESULT(0);
            }
            let window = &mut *pointer;
            if id == SELECT_VERSION && (wparam.0 >> 16) as u32 == CBN_SELCHANGE {
                let index = SendMessageW(HWND(lparam.0 as *mut _), CB_GETCURSEL, None, None).0;
                if index >= 0 {
                    if let Some((selected, _)) = window
                        .versions
                        .iter()
                        .enumerate()
                        .filter(|(_, version)| !version.is_empty())
                        .nth(index as usize)
                    {
                        window.updater.borrow_mut().selected = selected;
                    }
                }
                window.refresh();
            } else if matches!(
                id,
                CHECK_UPDATES
                    | AUTO_UPDATES
                    | PRERELEASE_UPDATES
                    | CHECK_AUTOMATICALLY
                    | CANCEL_UPDATE
                    | INSTALL_SELECTED
            ) {
                let _ = PostMessageW(
                    Some(window.owner),
                    WM_COMMAND,
                    WPARAM(id as usize),
                    LPARAM(0),
                );
            }
            return LRESULT(0);
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            let hdc = HDC(wparam.0 as *mut _);
            let _ = SetTextColor(hdc, rgb(242, 246, 250));
            let _ = windows::Win32::Graphics::Gdi::SetBkColor(hdc, rgb(20, 24, 32));
            return LRESULT((&*pointer).background.0 as isize);
        }
        WM_PAINT => {
            (&*pointer).paint(hwnd);
            return LRESULT(0);
        }
        WM_CLOSE => {
            (&mut *pointer).close();
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            let x = lparam.0 as u16 as i16 as i32;
            let y = (lparam.0 >> 16) as u16 as i16 as i32;
            let mut client = RECT::default();
            let window = &mut *pointer;
            if (12..SIDEBAR_WIDTH - 12).contains(&x)
                && (110..260).contains(&y)
                && (y - 110) % 50 < 42
            {
                window.select_page(match (y - 110) / 50 {
                    0 => Page::General,
                    1 => Page::OutputDevices,
                    _ => Page::Updates,
                });
                return LRESULT(0);
            }
            if window.page == Page::General
                && GetClientRect(hwnd, &mut client).is_ok()
                && in_card(x, y, client.right)
            {
                let result = (&mut *pointer).toggle();
                if let Err(error) = result {
                    theme::show_message(hwnd, "Settings error", &error);
                }
            }
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            let x = lparam.0 as u16 as i16 as i32;
            let y = (lparam.0 >> 16) as u16 as i16 as i32;
            let mut client = RECT::default();
            if GetClientRect(hwnd, &mut client).is_ok() {
                let hovered = in_card(x, y, client.right);
                let window = &mut *pointer;
                if window.hovered != hovered {
                    window.hovered = hovered;
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            return LRESULT(0);
        }
        WM_KEYDOWN => {
            if (&*pointer).page == Page::General
                && (wparam.0 as u16 == VK_SPACE.0 || wparam.0 as u16 == VK_RETURN.0)
            {
                let result = (&mut *pointer).toggle();
                if let Err(error) = result {
                    theme::show_message(hwnd, "Settings error", &error);
                }
                return LRESULT(0);
            }
            if wparam.0 as u16 == VK_ESCAPE.0 {
                (&mut *pointer).close();
                return LRESULT(0);
            }
        }
        WM_ACTIVATE if (wparam.0 as u32 & 0xffff) == WA_ACTIVE => {
            if !(&*pointer).hwnd.0.is_null() {
                let _ = (&mut *pointer).refresh_devices();
            }
            if let Ok(enabled) = startup::is_enabled(&(&*pointer).config_path) {
                (&mut *pointer).autostart_enabled = enabled;
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        }
        _ => {}
    }
    DefWindowProcW(hwnd, message, wparam, lparam)
}

unsafe fn set_text(hwnd: HWND, id: u32, text: &str) {
    if let Ok(control) = GetDlgItem(Some(hwnd), id as i32) {
        let mut previous = vec![0; GetWindowTextLengthW(control) as usize + 1];
        let len = GetWindowTextW(control, &mut previous);
        if String::from_utf16_lossy(&previous[..len as usize]) != text {
            let text: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
            let _ = SetWindowTextW(control, PCWSTR(text.as_ptr()));
        }
    }
}

fn in_card(x: i32, y: i32, width: i32) -> bool {
    (CONTENT_LEFT..width - 26).contains(&x) && (CARD_TOP..CARD_BOTTOM).contains(&y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Gdi::{GetUpdateRect, ValidateRect};
    use windows::Win32::UI::Controls::WM_MOUSELEAVE;
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_DOWN;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, GWL_EXSTYLE, GWL_STYLE, LB_GETCOUNT, LB_GETTOPINDEX, LB_SETCURSEL,
        SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SW_SHOWNOACTIVATE, WM_LBUTTONDOWN, WM_MOUSEWHEEL,
        WS_EX_TOOLWINDOW, WS_VISIBLE,
    };

    #[test]
    fn device_cards_have_their_own_page_and_preserve_output() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("volume3.json");
        let config = Rc::new(RefCell::new(Config::default()));
        config
            .borrow_mut()
            .hidden_output_devices
            .insert("disconnected-device".into());
        config.borrow().save(&path).unwrap();
        let updater = Rc::new(RefCell::new(Updater::new().unwrap()));
        let module =
            unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }.unwrap();
        register_window_class(HINSTANCE(module.0), HICON::default()).unwrap();
        let mut window = Box::new(SettingsWindow::new(path.clone(), config.clone(), updater));
        window.open(HWND::default()).unwrap();
        let audio = Audio::new().unwrap();
        let current = audio.current_device_id(audio::output_flow()).ok();
        let control = unsafe { GetDlgItem(Some(window.hwnd), OUTPUT_DEVICES as i32) }.unwrap();
        assert_eq!(
            unsafe { SendMessageW(control, LB_GETCOUNT, None, None) }.0 as usize,
            window.devices.len()
        );
        assert!(window.page == Page::General);
        assert_eq!(
            unsafe { GetWindowLongPtrW(control, GWL_STYLE) } & WS_VISIBLE.0 as isize,
            0
        );
        unsafe {
            SendMessageW(
                window.hwnd,
                WM_LBUTTONUP,
                None,
                Some(LPARAM((180 << 16) | 40)),
            );
        }
        assert!(window.page == Page::OutputDevices);
        let Some(device) = window.devices.first() else {
            return;
        };
        let id = device.id.clone();
        assert_ne!(
            unsafe { GetWindowLongPtrW(control, GWL_STYLE) } & WS_VISIBLE.0 as isize,
            0
        );
        unsafe {
            // Windows only tracks dirty regions for visible controls; keep this window off-screen.
            SetWindowLongPtrW(window.hwnd, GWL_EXSTYLE, WS_EX_TOOLWINDOW.0 as isize);
            let _ = SetWindowPos(
                window.hwnd,
                None,
                -10000,
                -10000,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER,
            );
            let _ = ShowWindow(window.hwnd, SW_SHOWNOACTIVATE);
            let mut dirty = RECT::default();
            let _ = ValidateRect(Some(control), None);
            SendMessageW(control, WM_MOUSEMOVE, None, Some(LPARAM((50 << 16) | 40)));
            assert!(GetUpdateRect(control, Some(&mut dirty), false).as_bool());
            assert_eq!((dirty.top, dirty.bottom), (0, 100));
            let _ = ValidateRect(Some(control), None);
            SendMessageW(control, WM_MOUSEMOVE, None, Some(LPARAM((51 << 16) | 41)));
            assert!(!GetUpdateRect(control, None, false).as_bool());
            if window.devices.len() > 1 {
                SendMessageW(control, WM_MOUSEMOVE, None, Some(LPARAM((150 << 16) | 40)));
                assert!(GetUpdateRect(control, Some(&mut dirty), false).as_bool());
                assert_eq!((dirty.top, dirty.bottom), (0, 200));
                let _ = ValidateRect(Some(control), None);
                SendMessageW(control, WM_MOUSELEAVE, None, None);
                assert!(GetUpdateRect(control, Some(&mut dirty), false).as_bool());
                assert_eq!((dirty.top, dirty.bottom), (100, 200));
            }
        }
        for index in 0..window.devices.len() {
            unsafe {
                SendMessageW(control, LB_SETCURSEL, Some(WPARAM(index)), None);
                SendMessageW(control, WM_KEYDOWN, Some(WPARAM(VK_SPACE.0 as usize)), None);
            }
        }
        let loaded = Config::load(&path).unwrap();
        assert!(window
            .devices
            .iter()
            .all(|device| loaded.hidden_output_devices.contains(&device.id)));
        assert!(loaded.hidden_output_devices.contains("disconnected-device"));
        assert_eq!(loaded.active().output_device, None);
        assert_eq!(audio.current_device_id(audio::output_flow()).ok(), current);
        window.close();
        *config.borrow_mut() = loaded;
        window.open(HWND::default()).unwrap();
        let control = unsafe { GetDlgItem(Some(window.hwnd), OUTPUT_DEVICES as i32) }.unwrap();
        assert_eq!(window.devices[0].id, id);
        assert!(config.borrow().hidden_output_devices.contains(&id));
        unsafe {
            SendMessageW(control, LB_SETTOPINDEX, Some(WPARAM(0)), None);
            SendMessageW(control, WM_LBUTTONDOWN, None, Some(LPARAM((50 << 16) | 40)));
            SendMessageW(control, WM_LBUTTONUP, None, Some(LPARAM((50 << 16) | 40)));
        }
        assert!(!Config::load(&path)
            .unwrap()
            .hidden_output_devices
            .contains(&id));
        assert!(!config.borrow().hidden_output_devices.contains(&id));
        unsafe {
            SendMessageW(control, LB_SETCURSEL, Some(WPARAM(0)), None);
            SendMessageW(control, WM_KEYDOWN, Some(WPARAM(VK_DOWN.0 as usize)), None);
        }
        assert!(!config.borrow().hidden_output_devices.contains(&id));
        if window.devices.len() > 4 {
            unsafe {
                SendMessageW(
                    control,
                    WM_MOUSEWHEEL,
                    Some(WPARAM((-360i16 as u16 as usize) << 16)),
                    None,
                );
            }
            assert!(unsafe { SendMessageW(control, LB_GETTOPINDEX, None, None) }.0 > 0);
        }
        window.select_page(Page::Updates);
        assert_eq!(
            unsafe { GetWindowLongPtrW(control, GWL_STYLE) } & WS_VISIBLE.0 as isize,
            0
        );
        window.select_page(Page::General);
        assert_eq!(
            unsafe { GetWindowLongPtrW(control, GWL_STYLE) } & WS_VISIBLE.0 as isize,
            0
        );
        window.config_path = folder.path().to_owned();
        assert!(window.set_device_visible(0, false).is_err());
        assert!(!config.borrow().hidden_output_devices.contains(&id));
        assert_eq!(audio.current_device_id(audio::output_flow()).ok(), current);
    }
}
