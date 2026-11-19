use super::*;
use windows::Win32::{
    Graphics::Gdi::{
        Arc, CreatePen, GetStockObject, Polygon, ScreenToClient, SetWindowOrgEx, NULL_PEN, PS_SOLID,
    },
    UI::{
        Controls::{ODS_FOCUS, WM_MOUSELEAVE},
        Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT},
        Shell::DefSubclassProc,
        WindowsAndMessaging::{
            DLGC_WANTMESSAGE, LB_GETCURSEL, LB_GETITEMRECT, LB_ITEMFROMPOINT, WM_GETDLGCODE,
            WM_MOUSEWHEEL, WM_VSCROLL,
        },
    },
};

impl SettingsWindow {
    pub(super) unsafe fn draw_device_card(&self, item: &DRAWITEMSTRUCT) {
        let width = item.rcItem.right - item.rcItem.left;
        let height = item.rcItem.bottom - item.rcItem.top;
        let buffer = CreateCompatibleDC(Some(item.hDC));
        let bitmap = CreateCompatibleBitmap(item.hDC, width, height);
        if !buffer.0.is_null() && !bitmap.0.is_null() {
            let old = SelectObject(buffer, HGDIOBJ(bitmap.0));
            let _ = SetWindowOrgEx(buffer, item.rcItem.left, item.rcItem.top, None);
            self.draw_device_card_contents(&DRAWITEMSTRUCT {
                hDC: buffer,
                ..*item
            });
            let _ = BitBlt(
                item.hDC,
                item.rcItem.left,
                item.rcItem.top,
                width,
                height,
                Some(buffer),
                item.rcItem.left,
                item.rcItem.top,
                SRCCOPY,
            );
            SelectObject(buffer, old);
        } else {
            self.draw_device_card_contents(item);
        }
        if !bitmap.0.is_null() {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
        }
        if !buffer.0.is_null() {
            let _ = DeleteDC(buffer);
        }
    }

    unsafe fn draw_device_card_contents(&self, item: &DRAWITEMSTRUCT) {
        fill(item.hDC, item.rcItem, theme::BACKGROUND);
        let Some(device) = self.devices.get(item.itemID as usize) else {
            return;
        };
        let visible = !self
            .config
            .borrow()
            .hidden_output_devices
            .contains(&device.id);
        let mut card = RECT {
            top: item.rcItem.top + 6,
            bottom: item.rcItem.bottom - 6,
            right: item.rcItem.right - 8,
            ..item.rcItem
        };
        rounded(
            item.hDC,
            card,
            if item.itemState.0 & ODS_FOCUS.0 != 0 {
                rgb(78, 161, 255)
            } else {
                rgb(44, 54, 70)
            },
            14,
        );
        card.left += 1;
        card.right -= 1;
        card.top += 1;
        card.bottom -= 1;
        rounded(
            item.hDC,
            card,
            if self.hovered_device == Some(item.itemID as usize) {
                rgb(36, 45, 59)
            } else {
                rgb(27, 34, 46)
            },
            13,
        );
        let text_color = if visible {
            rgb(242, 246, 250)
        } else {
            rgb(163, 174, 192)
        };
        let icon_left = card.left + 18;
        let icon_top = card.top + 21;
        circle(
            item.hDC,
            icon_left,
            icon_top,
            44,
            if visible {
                rgb(35, 58, 83)
            } else {
                rgb(39, 46, 59)
            },
        );
        let brush = CreateSolidBrush(if visible {
            rgb(124, 190, 255)
        } else {
            text_color
        });
        let old_brush = SelectObject(item.hDC, HGDIOBJ(brush.0));
        let old_pen = SelectObject(item.hDC, GetStockObject(NULL_PEN));
        let _ = Polygon(
            item.hDC,
            &[
                POINT {
                    x: icon_left + 11,
                    y: icon_top + 18,
                },
                POINT {
                    x: icon_left + 17,
                    y: icon_top + 18,
                },
                POINT {
                    x: icon_left + 24,
                    y: icon_top + 12,
                },
                POINT {
                    x: icon_left + 24,
                    y: icon_top + 32,
                },
                POINT {
                    x: icon_left + 17,
                    y: icon_top + 26,
                },
                POINT {
                    x: icon_left + 11,
                    y: icon_top + 26,
                },
            ],
        );
        let pen = CreatePen(
            PS_SOLID,
            2,
            if visible {
                rgb(124, 190, 255)
            } else {
                text_color
            },
        );
        SelectObject(item.hDC, HGDIOBJ(pen.0));
        let _ = Arc(
            item.hDC,
            icon_left + 20,
            icon_top + 14,
            icon_left + 34,
            icon_top + 30,
            icon_left + 28,
            icon_top + 30,
            icon_left + 28,
            icon_top + 14,
        );
        SelectObject(item.hDC, old_pen);
        SelectObject(item.hDC, old_brush);
        let _ = DeleteObject(HGDIOBJ(pen.0));
        let _ = DeleteObject(HGDIOBJ(brush.0));
        let _ = SetBkMode(item.hDC, TRANSPARENT);
        draw_text(
            item.hDC,
            &device.name,
            RECT {
                left: card.left + 78,
                top: card.top + 14,
                right: card.right - 94,
                bottom: card.top + 42,
            },
            self.label_font,
            text_color,
        );
        draw_text(
            item.hDC,
            match (device.is_default, visible) {
                (true, true) => "Default output  ·  Visible in tray",
                (true, false) => "Default output  ·  Hidden from tray",
                (false, true) => "Visible in tray menu",
                (false, false) => "Hidden from tray menu",
            },
            RECT {
                left: card.left + 78,
                top: card.top + 43,
                right: card.right - 94,
                bottom: card.top + 68,
            },
            self.body_font,
            rgb(146, 162, 184),
        );
        let track = RECT {
            left: card.right - 70,
            top: card.top + 30,
            right: card.right - 22,
            bottom: card.top + 56,
        };
        rounded(
            item.hDC,
            track,
            if visible {
                rgb(64, 142, 226)
            } else {
                rgb(64, 75, 94)
            },
            26,
        );
        circle(
            item.hDC,
            track.left + if visible { 24 } else { 3 },
            track.top + 3,
            20,
            rgb(245, 249, 255),
        );
    }

    unsafe fn hover_device_at(&mut self, hwnd: HWND, point: Option<POINT>) {
        let hovered = point.and_then(|point| card_at(hwnd, point));
        if self.hovered_device != hovered {
            let previous = std::mem::replace(&mut self.hovered_device, hovered);
            for index in [previous, hovered].into_iter().flatten() {
                invalidate_card(hwnd, index);
            }
        }
    }
}

pub(super) unsafe fn invalidate_card(hwnd: HWND, index: usize) {
    let mut rect = RECT::default();
    if SendMessageW(
        hwnd,
        LB_GETITEMRECT,
        Some(WPARAM(index)),
        Some(LPARAM((&mut rect as *mut RECT) as isize)),
    )
    .0 != -1
    {
        let _ = InvalidateRect(Some(hwnd), Some(&rect), false);
    }
}

unsafe fn card_at(hwnd: HWND, point: POINT) -> Option<usize> {
    let hit = SendMessageW(
        hwnd,
        LB_ITEMFROMPOINT,
        None,
        Some(LPARAM(
            ((point.y as u16 as u32) << 16 | point.x as u16 as u32) as isize,
        )),
    )
    .0;
    if hit >> 16 != 0 {
        return None;
    }
    let mut rect = RECT::default();
    if SendMessageW(
        hwnd,
        LB_GETITEMRECT,
        Some(WPARAM(hit as usize)),
        Some(LPARAM((&mut rect as *mut RECT) as isize)),
    )
    .0 != -1
        && (rect.left..rect.right - 8).contains(&point.x)
        && (rect.top + 6..rect.bottom - 6).contains(&point.y)
    {
        Some(hit as usize)
    } else {
        None
    }
}

pub(super) unsafe extern "system" fn device_cards_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    owner: usize,
) -> LRESULT {
    let owner = HWND(owner as *mut _);
    let pointer = GetWindowLongPtrW(owner, GWLP_USERDATA) as *mut SettingsWindow;
    if pointer.is_null() {
        return DefSubclassProc(hwnd, message, wparam, lparam);
    }
    match message {
        WM_GETDLGCODE if matches!(wparam.0 as u16, key if key == VK_SPACE.0 || key == VK_RETURN.0) =>
        {
            return LRESULT(
                DefSubclassProc(hwnd, message, wparam, lparam).0 | DLGC_WANTMESSAGE as isize,
            );
        }
        WM_KEYDOWN if wparam.0 as u16 == VK_SPACE.0 || wparam.0 as u16 == VK_RETURN.0 => {
            let selected = SendMessageW(hwnd, LB_GETCURSEL, None, None).0;
            if selected >= 0 && lparam.0 & (1 << 30) == 0 {
                SendMessageW(owner, TOGGLE_DEVICE, Some(WPARAM(selected as usize)), None);
            }
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            if let Some(index) = card_at(
                hwnd,
                POINT {
                    x: lparam.0 as u16 as i16 as i32,
                    y: (lparam.0 >> 16) as u16 as i16 as i32,
                },
            ) {
                SendMessageW(owner, TOGGLE_DEVICE, Some(WPARAM(index)), None);
            }
        }
        WM_MOUSEMOVE => {
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                ..Default::default()
            };
            let _ = TrackMouseEvent(&mut tracking);
            (&mut *pointer).hover_device_at(
                hwnd,
                Some(POINT {
                    x: lparam.0 as u16 as i16 as i32,
                    y: (lparam.0 >> 16) as u16 as i16 as i32,
                }),
            );
        }
        WM_MOUSELEAVE => {
            (&mut *pointer).hover_device_at(hwnd, None);
        }
        _ => {}
    }
    let result = DefSubclassProc(hwnd, message, wparam, lparam);
    if matches!(message, WM_MOUSEWHEEL | WM_VSCROLL | WM_KEYDOWN) {
        let mut cursor = POINT::default();
        if GetCursorPos(&mut cursor).is_ok() && ScreenToClient(hwnd, &mut cursor).as_bool() {
            (&mut *pointer).hover_device_at(hwnd, Some(cursor));
        }
    }
    result
}
