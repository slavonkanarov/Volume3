use std::{mem::size_of, sync::OnceLock};

use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::{
            Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE},
            Gdi::{
                BeginPaint, CreateFontW, CreatePen, CreateSolidBrush, DeleteObject, DrawTextW,
                Ellipse, EndPaint, ExcludeClipRect, FillRect, GetDC, GetMonitorInfoW,
                GetStockObject, InvalidateRect, LineTo, MonitorFromPoint, MoveToEx, ReleaseDC,
                RestoreDC, RoundRect, SaveDC, SelectObject, SetBkColor, SetBkMode, SetDCBrushColor,
                SetTextColor, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DT_CENTER,
                DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FW_NORMAL, FW_SEMIBOLD,
                HDC, HFONT, HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST, NULL_PEN,
                OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID, TRANSPARENT,
            },
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{
                GetComboBoxInfo, SetWindowTheme, COMBOBOXINFO, DRAWITEMSTRUCT, ODS_DISABLED,
                ODS_FOCUS, ODS_SELECTED, WM_MOUSELEAVE,
            },
            Input::KeyboardAndMouse::{
                EnableWindow, GetFocus, IsWindowEnabled, SetFocus, TrackMouseEvent, TME_LEAVE,
                TRACKMOUSEEVENT,
            },
            Shell::{DefSubclassProc, SetWindowSubclass},
            WindowsAndMessaging::{
                AdjustWindowRectEx, CreateWindowExW, DefWindowProcW, DestroyWindow,
                DispatchMessageW, GetClientRect, GetCursorPos, GetMessageW, GetWindowLongPtrW,
                GetWindowTextLengthW, GetWindowTextW, IsDialogMessageW, IsWindow, LoadCursorW,
                MessageBoxW, PostQuitMessage, RegisterClassExW, SendMessageW, SetForegroundWindow,
                SetWindowLongPtrW, ShowWindow, TranslateMessage, BM_GETCHECK, BS_OWNERDRAW,
                ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY, GWLP_USERDATA, HMENU, IDC_ARROW,
                MB_ICONERROR, MB_OK, MSG, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE,
                WM_COMMAND, WM_CTLCOLORSTATIC, WM_DRAWITEM, WM_ERASEBKGND, WM_GETFONT,
                WM_KILLFOCUS, WM_MOUSEMOVE, WM_PAINT, WM_PRINTCLIENT, WM_SETFOCUS, WM_SETFONT,
                WNDCLASSEXW, WS_CAPTION, WS_CHILD, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
};

pub(crate) const BACKGROUND: COLORREF = rgb(20, 24, 32);
const SURFACE: COLORREF = rgb(29, 35, 46);
const ACCENT: COLORREF = rgb(48, 126, 164);
const TEXT: COLORREF = rgb(242, 246, 250);
const MUTED: COLORREF = rgb(153, 167, 187);

pub(crate) const fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF(red as u32 | ((green as u32) << 8) | ((blue as u32) << 16))
}

pub(crate) fn font(size: i32, weight: i32) -> HFONT {
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

pub(crate) unsafe fn fill(hdc: HDC, rect: RECT, color: COLORREF) {
    let brush = CreateSolidBrush(color);
    FillRect(hdc, &rect, brush);
    let _ = DeleteObject(HGDIOBJ(brush.0));
}

pub(crate) unsafe fn rounded(hdc: HDC, rect: RECT, color: COLORREF, radius: i32) {
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

pub(crate) unsafe fn circle(hdc: HDC, x: i32, y: i32, size: i32, color: COLORREF) {
    let brush = CreateSolidBrush(color);
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
    let _ = Ellipse(hdc, x, y, x + size, y + size);
    let _ = SelectObject(hdc, old_pen);
    let _ = SelectObject(hdc, old_brush);
    let _ = DeleteObject(HGDIOBJ(brush.0));
}

pub(crate) unsafe fn draw_text(hdc: HDC, text: &str, mut rect: RECT, font: HFONT, color: COLORREF) {
    let old = SelectObject(hdc, HGDIOBJ(font.0));
    let _ = SetTextColor(hdc, color);
    let mut value: Vec<u16> = text.encode_utf16().collect();
    DrawTextW(
        hdc,
        &mut value,
        &mut rect,
        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
    let _ = SelectObject(hdc, old);
}

unsafe fn window_text(hwnd: HWND) -> String {
    let mut text = vec![0; GetWindowTextLengthW(hwnd) as usize + 1];
    let len = GetWindowTextW(hwnd, &mut text);
    String::from_utf16_lossy(&text[..len as usize])
}

unsafe fn hovered(hwnd: HWND) -> bool {
    let mut cursor = POINT::default();
    let mut rect = RECT::default();
    GetCursorPos(&mut cursor).is_ok()
        && windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut cursor).as_bool()
        && GetClientRect(hwnd, &mut rect).is_ok()
        && (rect.left..rect.right).contains(&cursor.x)
        && (rect.top..rect.bottom).contains(&cursor.y)
}

pub(crate) unsafe fn draw_button(item: &DRAWITEMSTRUCT, primary: bool, background: COLORREF) {
    let disabled = item.itemState.0 & ODS_DISABLED.0 != 0;
    let focused = item.itemState.0 & ODS_FOCUS.0 != 0;
    let mut rect = item.rcItem;
    fill(item.hDC, rect, background);
    rounded(
        item.hDC,
        rect,
        if focused {
            rgb(121, 194, 229)
        } else {
            rgb(61, 73, 92)
        },
        8,
    );
    rect.left += 1;
    rect.top += 1;
    rect.right -= 1;
    rect.bottom -= 1;
    let color = if disabled {
        SURFACE
    } else if item.itemState.0 & ODS_SELECTED.0 != 0 {
        rgb(33, 86, 114)
    } else if hovered(item.hwndItem) {
        rgb(54, 109, 140)
    } else if primary {
        ACCENT
    } else {
        rgb(38, 46, 61)
    };
    rounded(item.hDC, rect, color, 7);
    let font = SendMessageW(item.hwndItem, WM_GETFONT, None, None);
    let old = SelectObject(item.hDC, HGDIOBJ(font.0 as *mut _));
    let _ = SetBkMode(item.hDC, TRANSPARENT);
    let _ = SetTextColor(item.hDC, if disabled { rgb(105, 118, 137) } else { TEXT });
    let mut label: Vec<u16> = window_text(item.hwndItem).encode_utf16().collect();
    DrawTextW(
        item.hDC,
        &mut label,
        &mut rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );
    let _ = SelectObject(item.hDC, old);
}

// Keep native input, focus, and checkbox state; replace only their classic painting.
pub(crate) unsafe fn style_control(hwnd: HWND, kind: usize) {
    let _ = SetWindowSubclass(hwnd, Some(control_proc), 1, kind);
}

unsafe extern "system" fn control_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    kind: usize,
) -> LRESULT {
    match message {
        WM_ERASEBKGND if kind == 1 => return LRESULT(1),
        WM_PAINT | WM_PRINTCLIENT if kind == 2 => {
            let result = DefSubclassProc(hwnd, message, wparam, lparam);
            let hdc = if message == WM_PAINT {
                GetDC(Some(hwnd))
            } else {
                HDC(wparam.0 as *mut _)
            };
            paint_combo_frame(hwnd, hdc);
            if message == WM_PAINT {
                ReleaseDC(Some(hwnd), hdc);
            }
            return result;
        }
        WM_PAINT | WM_PRINTCLIENT if kind == 1 => {
            let mut paint = PAINTSTRUCT::default();
            let hdc = if message == WM_PAINT {
                BeginPaint(hwnd, &mut paint)
            } else {
                HDC(wparam.0 as *mut _)
            };
            paint_toggle(hwnd, hdc);
            if message == WM_PAINT {
                let _ = EndPaint(hwnd, &paint);
            }
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                ..Default::default()
            };
            let _ = TrackMouseEvent(&mut tracking);
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
        WM_MOUSELEAVE | WM_SETFOCUS | WM_KILLFOCUS => {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, message, wparam, lparam)
}

unsafe fn paint_toggle(hwnd: HWND, hdc: HDC) {
    let mut rect = RECT::default();
    if GetClientRect(hwnd, &mut rect).is_err() {
        return;
    }
    let enabled = IsWindowEnabled(hwnd).as_bool();
    let focused = GetFocus() == hwnd;
    let hot = enabled && hovered(hwnd);
    fill(hdc, rect, BACKGROUND);
    rounded(hdc, rect, if focused { ACCENT } else { SURFACE }, 8);
    let inner = RECT {
        left: 1,
        top: 1,
        right: rect.right - 1,
        bottom: rect.bottom - 1,
    };
    rounded(hdc, inner, if hot { rgb(38, 46, 61) } else { SURFACE }, 7);
    let _ = SetBkMode(hdc, TRANSPARENT);
    let font = HFONT(SendMessageW(hwnd, WM_GETFONT, None, None).0 as *mut _);
    let mut label = rect;
    label.left += 12;
    label.right -= 68;
    let text = window_text(hwnd);
    draw_text(hdc, &text, label, font, if enabled { TEXT } else { MUTED });
    let checked = SendMessageW(hwnd, BM_GETCHECK, None, None).0 != 0;
    let left = rect.right - 52;
    let top = (rect.bottom - 22) / 2;
    rounded(
        hdc,
        RECT {
            left,
            top,
            right: left + 40,
            bottom: top + 22,
        },
        if checked && enabled {
            ACCENT
        } else {
            rgb(69, 80, 98)
        },
        22,
    );
    circle(
        hdc,
        left + if checked { 21 } else { 3 },
        top + 3,
        16,
        if enabled { TEXT } else { MUTED },
    );
}

unsafe fn paint_combo_frame(hwnd: HWND, hdc: HDC) {
    let mut info = COMBOBOXINFO {
        cbSize: size_of::<COMBOBOXINFO>() as u32,
        ..Default::default()
    };
    let mut rect = RECT::default();
    if GetComboBoxInfo(hwnd, &mut info).is_err() || GetClientRect(hwnd, &mut rect).is_err() {
        return;
    }
    let saved = SaveDC(hdc);
    ExcludeClipRect(
        hdc,
        info.rcItem.left,
        info.rcItem.top,
        info.rcItem.right,
        info.rcItem.bottom,
    );
    fill(hdc, rect, BACKGROUND);
    rounded(
        hdc,
        rect,
        if GetFocus() == hwnd {
            ACCENT
        } else {
            rgb(61, 73, 92)
        },
        8,
    );
    rounded(
        hdc,
        RECT {
            left: 1,
            top: 1,
            right: rect.right - 1,
            bottom: rect.bottom - 1,
        },
        SURFACE,
        7,
    );
    let pen = CreatePen(PS_SOLID, 2, MUTED);
    let old = SelectObject(hdc, HGDIOBJ(pen.0));
    let x = (info.rcButton.left + info.rcButton.right) / 2;
    let y = (info.rcButton.top + info.rcButton.bottom) / 2 - 2;
    let _ = MoveToEx(hdc, x - 4, y, None);
    let _ = LineTo(hdc, x, y + 4);
    let _ = LineTo(hdc, x + 4, y);
    let _ = SelectObject(hdc, old);
    let _ = DeleteObject(HGDIOBJ(pen.0));
    let _ = RestoreDC(hdc, saved);
}

struct Message {
    heading: String,
    title_font: HFONT,
    body_font: HFONT,
}

/// Display an application-styled message while retaining native keyboard navigation.
pub fn show_message(owner: HWND, heading: &str, text: &str) {
    if let Err(error) = message_window(owner, heading, text) {
        let text: Vec<u16> = format!("{text}\n\n{error}")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        unsafe {
            MessageBoxW(
                Some(owner),
                PCWSTR(text.as_ptr()),
                w!("Volume3"),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}

fn message_window(owner: HWND, heading: &str, text: &str) -> windows::core::Result<()> {
    static CLASS: OnceLock<std::result::Result<(), String>> = OnceLock::new();
    let module = unsafe { GetModuleHandleW(None)? };
    let registered = CLASS.get_or_init(|| {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(message_proc),
            hInstance: HINSTANCE(module.0),
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() },
            lpszClassName: w!("Volume3MessageWindow"),
            ..Default::default()
        };
        if unsafe { RegisterClassExW(&class) } == 0 {
            Err(std::io::Error::last_os_error().to_string())
        } else {
            Ok(())
        }
    });
    if let Err(error) = registered {
        return Err(windows::core::Error::new(
            windows::core::HRESULT(0x80004005u32 as i32),
            error,
        ));
    }
    let mut state = Box::new(Message {
        heading: heading.into(),
        title_font: font(24, FW_SEMIBOLD.0 as i32),
        body_font: font(15, FW_NORMAL.0 as i32),
    });
    let result = (|| unsafe {
        let style = WS_CAPTION | WS_SYSMENU;
        let mut bounds = RECT {
            right: 640,
            bottom: 330,
            ..Default::default()
        };
        AdjustWindowRectEx(&mut bounds, style, false, WINDOW_EX_STYLE::default())?;
        let mut cursor = POINT::default();
        GetCursorPos(&mut cursor)?;
        let mut monitor = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(
            MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST),
            &mut monitor,
        );
        let width = bounds.right - bounds.left;
        let height = bounds.bottom - bounds.top;
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("Volume3MessageWindow"),
            w!("Volume3"),
            style,
            monitor.rcWork.left + (monitor.rcWork.right - monitor.rcWork.left - width) / 2,
            monitor.rcWork.top + (monitor.rcWork.bottom - monitor.rcWork.top - height) / 2,
            width,
            height,
            Some(owner),
            None,
            Some(HINSTANCE(module.0)),
            None,
        )?;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&mut *state as *mut Message) as isize);
        let setup = (|| -> windows::core::Result<()> {
            let dark = 1i32;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&dark as *const i32).cast(),
                size_of::<i32>() as u32,
            );
            let text: Vec<u16> = text
                .replace("\r\n", "\n")
                .replace('\n', "\r\n")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let body = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("EDIT"),
                PCWSTR(text.as_ptr()),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_TABSTOP
                    | WS_VSCROLL
                    | WINDOW_STYLE((ES_READONLY | ES_MULTILINE | ES_AUTOVSCROLL) as u32),
                24,
                76,
                592,
                174,
                Some(hwnd),
                Some(HMENU(10 as *mut _)),
                None,
                None,
            )?;
            let button = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("BUTTON"),
                w!("OK"),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
                508,
                282,
                108,
                34,
                Some(hwnd),
                Some(HMENU(1 as *mut _)),
                None,
                None,
            )?;
            for control in [body, button] {
                SendMessageW(
                    control,
                    WM_SETFONT,
                    Some(WPARAM(state.body_font.0 as usize)),
                    Some(LPARAM(1)),
                );
            }
            let _ = SetWindowTheme(body, w!("DarkMode_Explorer"), None);
            style_control(button, 0);
            let owner_enabled = !owner.0.is_null() && IsWindowEnabled(owner).as_bool();
            if owner_enabled {
                let _ = EnableWindow(owner, false);
            }
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(button));
            let mut message = MSG::default();
            while IsWindow(Some(hwnd)).as_bool() {
                let status = GetMessageW(&mut message, None, 0, 0).0;
                if status <= 0 {
                    if status == 0 {
                        PostQuitMessage(message.wParam.0 as i32);
                    }
                    break;
                }
                if !IsDialogMessageW(hwnd, &message).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            if owner_enabled {
                let _ = EnableWindow(owner, true);
                let _ = SetForegroundWindow(owner);
            }
            Ok(())
        })();
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        if IsWindow(Some(hwnd)).as_bool() {
            let _ = DestroyWindow(hwnd);
        }
        setup
    })();
    unsafe {
        for font in [state.title_font, state.body_font] {
            let _ = DeleteObject(HGDIOBJ(font.0));
        }
    }
    result
}

unsafe extern "system" fn message_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Message;
    if state.is_null() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    match message {
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT | WM_PRINTCLIENT => {
            let mut paint = PAINTSTRUCT::default();
            let hdc = if message == WM_PAINT {
                BeginPaint(hwnd, &mut paint)
            } else {
                HDC(wparam.0 as *mut _)
            };
            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut rect);
            fill(hdc, rect, BACKGROUND);
            fill(hdc, RECT { top: 270, ..rect }, SURFACE);
            let _ = SetBkMode(hdc, TRANSPARENT);
            draw_text(
                hdc,
                &(*state).heading,
                RECT {
                    left: 24,
                    top: 18,
                    right: rect.right - 24,
                    bottom: 58,
                },
                (*state).title_font,
                TEXT,
            );
            if message == WM_PAINT {
                let _ = EndPaint(hwnd, &paint);
            }
            return LRESULT(0);
        }
        WM_CTLCOLORSTATIC => {
            let hdc = HDC(wparam.0 as *mut _);
            let _ = SetTextColor(hdc, TEXT);
            let _ = SetBkColor(hdc, BACKGROUND);
            let _ = SetDCBrushColor(hdc, BACKGROUND);
            return LRESULT(GetStockObject(windows::Win32::Graphics::Gdi::DC_BRUSH).0 as isize);
        }
        WM_DRAWITEM => {
            draw_button(&*(lparam.0 as *const DRAWITEMSTRUCT), true, SURFACE);
            return LRESULT(1);
        }
        WM_COMMAND if matches!(wparam.0 as u16, 1 | 2) => {
            let _ = DestroyWindow(hwnd);
            return LRESULT(0);
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            return LRESULT(0);
        }
        _ => {}
    }
    DefWindowProcW(hwnd, message, wparam, lparam)
}
