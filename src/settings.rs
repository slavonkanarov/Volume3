use std::{mem::size_of, path::PathBuf};

use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::{
            Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE},
            Gdi::{
                BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW,
                CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, Ellipse, EndPaint, FillRect,
                GetMonitorInfoW, GetStockObject, InvalidateRect, MonitorFromPoint, RoundRect,
                SelectObject, SetBkMode, SetTextColor, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
                DEFAULT_CHARSET, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER,
                FW_NORMAL, FW_SEMIBOLD, HDC, HFONT, HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST,
                NULL_PEN, OUT_DEFAULT_PRECIS, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
            },
        },
        UI::{
            Input::KeyboardAndMouse::{VK_ESCAPE, VK_RETURN, VK_SPACE},
            WindowsAndMessaging::{
                AdjustWindowRectEx, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect,
                GetCursorPos, GetWindowLongPtrW, LoadCursorW, MessageBoxW, RegisterClassExW,
                SetForegroundWindow, SetWindowLongPtrW, ShowWindow, GWLP_USERDATA, HICON,
                IDC_ARROW, MB_ICONERROR, MB_OK, SW_RESTORE, WA_ACTIVE, WM_ACTIVATE, WM_CLOSE,
                WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WNDCLASSEXW,
                WS_CAPTION, WS_EX_APPWINDOW, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU,
            },
        },
    },
};

use crate::startup;

const CLIENT_WIDTH: i32 = 480;
const CLIENT_HEIGHT: i32 = 300;
const CARD_TOP: i32 = 126;
const CARD_BOTTOM: i32 = 216;

pub struct SettingsWindow {
    hwnd: HWND,
    config_path: PathBuf,
    autostart_enabled: bool,
    hovered: bool,
    title_font: HFONT,
    label_font: HFONT,
    body_font: HFONT,
}

impl SettingsWindow {
    pub fn new(config_path: PathBuf) -> Self {
        Self {
            hwnd: HWND::default(),
            config_path,
            autostart_enabled: false,
            hovered: false,
            title_font: font(25, FW_SEMIBOLD.0 as i32),
            label_font: font(17, FW_SEMIBOLD.0 as i32),
            body_font: font(14, FW_NORMAL.0 as i32),
        }
    }

    pub fn open(&mut self) -> Result<HWND, String> {
        self.autostart_enabled = startup::is_enabled(&self.config_path)?;
        if !self.hwnd.0.is_null() {
            unsafe {
                let _ = InvalidateRect(Some(self.hwnd), None, false);
            }
            return Ok(self.hwnd);
        }
        let module = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }
            .map_err(|error| error.to_string())?;
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
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
        Ok(hwnd)
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
        fill(hdc, client, rgb(24, 22, 29));
        fill(
            hdc,
            RECT {
                bottom: 76,
                ..client
            },
            rgb(61, 34, 74),
        );
        fill(
            hdc,
            RECT {
                right: 5,
                bottom: 76,
                ..client
            },
            rgb(178, 84, 205),
        );
        let _ = SetBkMode(hdc, TRANSPARENT);
        draw_text(
            hdc,
            "Settings",
            RECT {
                left: 25,
                top: 13,
                right: client.right - 25,
                bottom: 47,
            },
            self.title_font,
            rgb(251, 247, 253),
        );
        draw_text(
            hdc,
            "Volume3",
            RECT {
                left: 26,
                top: 48,
                right: client.right - 25,
                bottom: 70,
            },
            self.body_font,
            rgb(213, 187, 223),
        );
        draw_text(
            hdc,
            "GENERAL",
            RECT {
                left: 27,
                top: 92,
                right: client.right - 26,
                bottom: 116,
            },
            self.body_font,
            rgb(181, 127, 204),
        );
        rounded(
            hdc,
            RECT {
                left: 24,
                top: CARD_TOP,
                right: client.right - 24,
                bottom: CARD_BOTTOM,
            },
            if self.hovered {
                rgb(51, 44, 60)
            } else {
                rgb(42, 37, 49)
            },
            16,
        );
        draw_text(
            hdc,
            "Start with Windows",
            RECT {
                left: 44,
                top: 147,
                right: client.right - 114,
                bottom: 177,
            },
            self.label_font,
            rgb(247, 244, 249),
        );
        draw_text(
            hdc,
            "Launch Volume3 when you sign in.",
            RECT {
                left: 44,
                top: 177,
                right: client.right - 114,
                bottom: 205,
            },
            self.body_font,
            rgb(178, 169, 187),
        );
        let track_left = client.right - 104;
        rounded(
            hdc,
            RECT {
                left: track_left,
                top: 154,
                right: track_left + 58,
                bottom: 184,
            },
            if self.autostart_enabled {
                rgb(175, 78, 204)
            } else {
                rgb(100, 94, 110)
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
            158,
            22,
            rgb(255, 255, 255),
        );
        draw_text(
            hdc,
            if self.autostart_enabled { "On" } else { "Off" },
            RECT {
                left: track_left + 18,
                top: 184,
                right: track_left + 58,
                bottom: 210,
            },
            self.body_font,
            rgb(192, 183, 200),
        );
        draw_text(
            hdc,
            "More settings will appear here.",
            RECT {
                left: 27,
                top: 239,
                right: client.right - 26,
                bottom: 269,
            },
            self.body_font,
            rgb(137, 129, 146),
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
            if GetClientRect(hwnd, &mut client).is_ok() && in_card(x, y, client.right) {
                let result = (&mut *pointer).toggle();
                if let Err(error) = result {
                    show_error(hwnd, &error);
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
            if wparam.0 as u16 == VK_SPACE.0 || wparam.0 as u16 == VK_RETURN.0 {
                let result = (&mut *pointer).toggle();
                if let Err(error) = result {
                    show_error(hwnd, &error);
                }
                return LRESULT(0);
            }
            if wparam.0 as u16 == VK_ESCAPE.0 {
                (&mut *pointer).close();
                return LRESULT(0);
            }
        }
        WM_ACTIVATE if (wparam.0 as u32 & 0xffff) == WA_ACTIVE => {
            if let Ok(enabled) = startup::is_enabled(&(&*pointer).config_path) {
                (&mut *pointer).autostart_enabled = enabled;
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        }
        _ => {}
    }
    DefWindowProcW(hwnd, message, wparam, lparam)
}

fn in_card(x: i32, y: i32, width: i32) -> bool {
    (24..width - 24).contains(&x) && (CARD_TOP..CARD_BOTTOM).contains(&y)
}

fn font(size: i32, weight: i32) -> HFONT {
    unsafe {
        CreateFontW(
            -size,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            w!("Segoe UI Variable"),
        )
    }
}

unsafe fn fill(hdc: HDC, rect: RECT, color: COLORREF) {
    let brush = CreateSolidBrush(color);
    FillRect(hdc, &rect, brush);
    let _ = DeleteObject(HGDIOBJ(brush.0));
}

unsafe fn rounded(hdc: HDC, rect: RECT, color: COLORREF, radius: i32) {
    let brush = CreateSolidBrush(color);
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
    let _ = RoundRect(
        hdc,
        rect.left,
        rect.top,
        rect.right,
        rect.bottom,
        radius,
        radius,
    );
    let _ = SelectObject(hdc, old_pen);
    let _ = SelectObject(hdc, old_brush);
    let _ = DeleteObject(HGDIOBJ(brush.0));
}

unsafe fn circle(hdc: HDC, x: i32, y: i32, size: i32, color: COLORREF) {
    let brush = CreateSolidBrush(color);
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
    let _ = Ellipse(hdc, x, y, x + size, y + size);
    let _ = SelectObject(hdc, old_pen);
    let _ = SelectObject(hdc, old_brush);
    let _ = DeleteObject(HGDIOBJ(brush.0));
}

unsafe fn draw_text(hdc: HDC, text: &str, rect: RECT, font: HFONT, color: COLORREF) {
    let old_font = SelectObject(hdc, HGDIOBJ(font.0));
    let _ = SetTextColor(hdc, color);
    let mut bounds = rect;
    let mut value: Vec<u16> = text.encode_utf16().collect();
    DrawTextW(
        hdc,
        &mut value,
        &mut bounds,
        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
    let _ = SelectObject(hdc, old_font);
}

fn show_error(hwnd: HWND, error: &str) {
    let text: Vec<u16> = error.encode_utf16().chain(Some(0)).collect();
    unsafe {
        MessageBoxW(
            Some(hwnd),
            PCWSTR(text.as_ptr()),
            w!("Volume3 Settings"),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF(u32::from(red) | (u32::from(green) << 8) | (u32::from(blue) << 16))
}
