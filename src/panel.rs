use std::{ffi::c_void, mem::size_of};

use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DeleteObject, DrawTextW,
            EndPaint, FillRect, GetMonitorInfoW, GetStockObject, GradientFill, InvalidateRect,
            MonitorFromPoint, RoundRect, SelectObject, SetBkMode, SetTextColor, CLEARTYPE_QUALITY,
            CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DRAW_TEXT_FORMAT, DT_CENTER, DT_END_ELLIPSIS,
            DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, FW_NORMAL,
            GRADIENT_FILL_RECT_V, GRADIENT_RECT, HDC, HFONT, HGDIOBJ, MONITORINFO,
            MONITOR_DEFAULTTONEAREST, NULL_PEN, OUT_DEFAULT_PRECIS, PAINTSTRUCT, TRANSPARENT,
            TRIVERTEX,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{ReleaseCapture, SetCapture},
            Shell::ExtractIconExW,
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, DrawIconEx,
                GetCursorPos, GetWindowLongPtrW, KillTimer, LoadCursorW, RegisterClassExW,
                SetForegroundWindow, SetTimer, SetWindowLongPtrW, ShowWindow, CS_DROPSHADOW,
                DI_NORMAL, GWLP_USERDATA, HICON, IDC_ARROW, SW_SHOW, WA_INACTIVE, WINDOW_EX_STYLE,
                WM_ACTIVATE, WM_CAPTURECHANGED, WM_CLOSE, WM_ERASEBKGND, WM_LBUTTONDOWN,
                WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_TIMER, WNDCLASSEXW,
                WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
            },
        },
    },
};

use crate::audio::{self, Audio, MasterControl, MixerApp};

const WIDTH: i32 = 360;
const HEADER_HEIGHT: i32 = 48;
const MASTER_HEIGHT: i32 = 54;
const ROW_HEIGHT: i32 = 44;
const SLIDER_LEFT: i32 = 65;
const SLIDER_RIGHT: i32 = 296;

struct AppRow {
    app: MixerApp,
    icon: HICON,
}

impl Drop for AppRow {
    fn drop(&mut self) {
        if !self.icon.0.is_null() {
            unsafe {
                let _ = DestroyIcon(self.icon);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum DragTarget {
    Master,
    App(usize),
}

pub struct Panel {
    hwnd: HWND,
    master_control: Option<MasterControl>,
    speaker_icon: HICON,
    device_name: String,
    device_id: String,
    master: u8,
    rows: Vec<AppRow>,
    scroll: usize,
    dragging: Option<DragTarget>,
    pending: Option<DragTarget>,
    font: HFONT,
    number_font: HFONT,
    badge_font: HFONT,
    height: i32,
    error: Option<String>,
}

impl Panel {
    pub fn new() -> Self {
        Self {
            hwnd: HWND::default(),
            master_control: None,
            speaker_icon: HICON::default(),
            device_name: String::new(),
            device_id: String::new(),
            master: 0,
            rows: Vec::new(),
            scroll: 0,
            dragging: None,
            pending: None,
            font: create_font(16),
            number_font: create_font(18),
            badge_font: create_font(14),
            height: 0,
            error: None,
        }
    }

    pub fn is_open(&self) -> bool {
        !self.hwnd.0.is_null()
    }

    pub fn open(&mut self, audio: &Audio, speaker_icon: HICON) -> Result<HWND, String> {
        self.master_control = None;
        self.pending = None;
        self.speaker_icon = speaker_icon;
        self.rows.clear();
        self.scroll = 0;
        self.error = None;

        let device = audio
            .devices(audio::output_flow())?
            .into_iter()
            .find(|device| device.is_default);
        if let Some(device) = device {
            self.device_name = device.name;
            self.device_id = device.id;
            let state = audio.state(Some(&self.device_id), audio::output_flow())?;
            self.master = state.percent;
            self.master_control = Some(audio.master_control(&self.device_id)?);
            let apps = match audio.mixer_apps_on_device(&self.device_id) {
                Ok(apps) => apps,
                Err(error) => {
                    self.error = Some(format!("Applications unavailable: {error}"));
                    Vec::new()
                }
            };
            for app in apps {
                self.rows.push(AppRow {
                    icon: extract_process_icon(app.pid),
                    app,
                });
            }
        } else {
            self.device_name = "No playback device".into();
            self.device_id.clear();
            self.master = 0;
        }

        let module = unsafe { GetModuleHandleW(None).map_err(error)? };
        let mut point = POINT::default();
        unsafe { GetCursorPos(&mut point).map_err(error)? };
        let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let work = info.rcWork;
        let max_height = (work.bottom - work.top - 20)
            .max(142)
            .min(HEADER_HEIGHT + MASTER_HEIGHT + 12 + 10 * ROW_HEIGHT);
        let capacity = ((max_height - HEADER_HEIGHT - MASTER_HEIGHT - 12) / ROW_HEIGHT).max(0);
        let visible = (self.rows.len() as i32).min(capacity);
        self.height = (HEADER_HEIGHT + MASTER_HEIGHT + 12 + visible * ROW_HEIGHT)
            .max(142)
            .min(max_height);
        let x = (work.right - WIDTH - 8).max(work.left + 8);
        let y = (work.bottom - self.height - 8).max(work.top + 8);
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0),
                w!("Volume3VolumePanel"),
                w!("Volume3 volume"),
                WS_POPUP,
                x,
                y,
                WIDTH,
                self.height,
                None,
                None,
                Some(HINSTANCE(module.0)),
                None,
            )
            .map_err(error)?
        };
        self.hwnd = hwnd;
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, self as *mut Panel as isize);
            let region = CreateRoundRectRgn(0, 0, WIDTH, self.height, 16, 16);
            if !region.0.is_null() {
                if windows::Win32::Graphics::Gdi::SetWindowRgn(hwnd, Some(region), true) == 0 {
                    let _ = DeleteObject(HGDIOBJ(region.0));
                }
            }
        }
        Ok(hwnd)
    }

    pub fn show(hwnd: HWND) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
    }

    pub fn close(&mut self) {
        if !self.hwnd.0.is_null() {
            let hwnd = self.hwnd;
            self.dragging = None;
            self.flush_volume();
            self.hwnd = HWND::default();
            unsafe {
                let _ = KillTimer(Some(hwnd), 1);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let _ = DestroyWindow(hwnd);
            }
        }
        self.rows.clear();
        self.master_control = None;
    }

    fn visible_rows(&self) -> usize {
        ((self.height - HEADER_HEIGHT - MASTER_HEIGHT - 12) / ROW_HEIGHT).max(0) as usize
    }

    fn target_at(&self, x: i32, y: i32) -> Option<DragTarget> {
        if !(SLIDER_LEFT - 9..=SLIDER_RIGHT + 9).contains(&x) {
            return None;
        }
        if (HEADER_HEIGHT..HEADER_HEIGHT + MASTER_HEIGHT).contains(&y) {
            return (!self.device_id.is_empty()).then_some(DragTarget::Master);
        }
        let list_top = HEADER_HEIGHT + MASTER_HEIGHT;
        if y < list_top || y >= list_top + self.visible_rows() as i32 * ROW_HEIGHT {
            return None;
        }
        let index = self.scroll + ((y - list_top) / ROW_HEIGHT) as usize;
        (index < self.rows.len()).then_some(DragTarget::App(index))
    }

    fn request_volume(&mut self, target: DragTarget, x: i32) {
        let percent = (((x - SLIDER_LEFT).clamp(0, SLIDER_RIGHT - SLIDER_LEFT) * 100)
            / (SLIDER_RIGHT - SLIDER_LEFT)) as u8;
        let current = match target {
            DragTarget::Master => &mut self.master,
            DragTarget::App(index) => &mut self.rows[index].app.volume,
        };
        if *current == percent {
            return;
        }
        *current = percent;
        self.pending = Some(target);
        let (top, bottom) = match target {
            DragTarget::Master => (HEADER_HEIGHT, HEADER_HEIGHT + MASTER_HEIGHT),
            DragTarget::App(index) => {
                let top = HEADER_HEIGHT + MASTER_HEIGHT + (index - self.scroll) as i32 * ROW_HEIGHT;
                (top, top + ROW_HEIGHT)
            }
        };
        let rect = RECT {
            left: 0,
            top,
            right: WIDTH,
            bottom,
        };
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), Some(&rect), false);
        }
    }

    fn flush_volume(&mut self) {
        let Some(target) = self.pending.take() else {
            return;
        };
        let result = match target {
            DragTarget::Master => self
                .master_control
                .as_ref()
                .unwrap()
                .set_volume(self.master),
            DragTarget::App(index) => self.rows[index].app.set_volume(self.rows[index].app.volume),
        };
        let had_error = self.error.is_some();
        self.error = result.err();
        if had_error || self.error.is_some() {
            unsafe {
                let _ = InvalidateRect(Some(self.hwnd), None, false);
            }
        }
    }

    unsafe fn paint(&self, hwnd: HWND) {
        let mut paint = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut paint);
        let vertices = [
            TRIVERTEX {
                x: 0,
                y: 0,
                Red: 0x9700,
                Green: 0x4600,
                Blue: 0xa700,
                Alpha: 0,
            },
            TRIVERTEX {
                x: WIDTH,
                y: self.height,
                Red: 0x6900,
                Green: 0x2800,
                Blue: 0x7900,
                Alpha: 0,
            },
        ];
        let mesh = GRADIENT_RECT {
            UpperLeft: 0,
            LowerRight: 1,
        };
        let _ = GradientFill(
            hdc,
            &vertices,
            &mesh as *const _ as *const c_void,
            1,
            GRADIENT_FILL_RECT_V,
        );
        fill(
            hdc,
            RECT {
                left: 0,
                top: HEADER_HEIGHT,
                right: WIDTH,
                bottom: HEADER_HEIGHT + MASTER_HEIGHT,
            },
            rgb(112, 44, 130),
        );
        let font = self.font;
        let number_font = self.number_font;
        let clip = paint.rcPaint;
        if intersects(clip, 0, HEADER_HEIGHT) {
            draw_text(
                hdc,
                &self.device_name,
                RECT {
                    left: 12,
                    top: 0,
                    right: WIDTH - 35,
                    bottom: HEADER_HEIGHT,
                },
                rgb(255, 255, 255),
                font,
                DT_LEFT,
            );
            draw_text(
                hdc,
                "⌃",
                RECT {
                    left: WIDTH - 34,
                    top: 0,
                    right: WIDTH - 10,
                    bottom: HEADER_HEIGHT,
                },
                rgb(255, 255, 255),
                font,
                DT_RIGHT,
            );
        }
        if intersects(clip, HEADER_HEIGHT, HEADER_HEIGHT + MASTER_HEIGHT) {
            self.draw_slider_row(
                hdc,
                HEADER_HEIGHT,
                self.master,
                self.speaker_icon,
                None,
                number_font,
            );
        }
        if intersects(clip, HEADER_HEIGHT + MASTER_HEIGHT, self.height) {
            if self.rows.is_empty() {
                draw_text(
                    hdc,
                    "No applications playing audio",
                    RECT {
                        left: 22,
                        top: HEADER_HEIGHT + MASTER_HEIGHT,
                        right: WIDTH - 20,
                        bottom: self.height,
                    },
                    rgb(237, 212, 244),
                    font,
                    DT_LEFT,
                );
            } else {
                for (visible, row) in self
                    .rows
                    .iter()
                    .skip(self.scroll)
                    .take(self.visible_rows())
                    .enumerate()
                {
                    let y = HEADER_HEIGHT + MASTER_HEIGHT + visible as i32 * ROW_HEIGHT;
                    if !intersects(clip, y, y + ROW_HEIGHT) {
                        continue;
                    }
                    self.draw_slider_row(
                        hdc,
                        y,
                        row.app.volume,
                        row.icon,
                        Some(&row.app.process),
                        number_font,
                    );
                }
            }
        }
        if let Some(message) = &self.error {
            if intersects(clip, self.height - 38, self.height - 8) {
                fill(
                    hdc,
                    RECT {
                        left: 8,
                        top: self.height - 38,
                        right: WIDTH - 8,
                        bottom: self.height - 8,
                    },
                    rgb(64, 29, 74),
                );
                draw_text(
                    hdc,
                    message,
                    RECT {
                        left: 16,
                        top: self.height - 38,
                        right: WIDTH - 16,
                        bottom: self.height - 8,
                    },
                    rgb(255, 225, 230),
                    font,
                    DT_LEFT,
                );
            }
        }
        let _ = EndPaint(hwnd, &paint);
    }

    unsafe fn draw_slider_row(
        &self,
        hdc: HDC,
        top: i32,
        value: u8,
        icon: HICON,
        fallback: Option<&str>,
        font: windows::Win32::Graphics::Gdi::HFONT,
    ) {
        let row_height = if fallback.is_some() {
            ROW_HEIGHT
        } else {
            MASTER_HEIGHT
        };
        let center = top + row_height / 2;
        if !icon.0.is_null() {
            let (x, size) = if fallback.is_some() {
                (23, 20)
            } else {
                (21, 24)
            };
            let _ = DrawIconEx(
                hdc,
                x,
                center - size / 2,
                icon,
                size,
                size,
                0,
                None,
                DI_NORMAL,
            );
        } else if let Some(name) = fallback {
            draw_fallback_icon(hdc, center, name, self.badge_font);
        }
        fill(
            hdc,
            RECT {
                left: SLIDER_LEFT,
                top: center - 2,
                right: SLIDER_RIGHT,
                bottom: center + 1,
            },
            rgb(184, 107, 199),
        );
        let thumb = SLIDER_LEFT + (SLIDER_RIGHT - SLIDER_LEFT) * value as i32 / 100;
        if thumb > SLIDER_LEFT {
            fill(
                hdc,
                RECT {
                    left: SLIDER_LEFT,
                    top: center - 2,
                    right: thumb,
                    bottom: center + 1,
                },
                rgb(226, 170, 240),
            );
        }
        rounded(
            hdc,
            RECT {
                left: thumb - 4,
                top: center - 12,
                right: thumb + 4,
                bottom: center + 12,
            },
            rgb(189, 91, 210),
            8,
        );
        draw_text(
            hdc,
            &value.to_string(),
            RECT {
                left: 306,
                top,
                right: WIDTH - 12,
                bottom: top + row_height,
            },
            rgb(255, 255, 255),
            font,
            DT_RIGHT,
        );
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        self.close();
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.font.0));
            let _ = DeleteObject(HGDIOBJ(self.number_font.0));
            let _ = DeleteObject(HGDIOBJ(self.badge_font.0));
        }
    }
}

fn create_font(height: i32) -> HFONT {
    unsafe {
        CreateFontW(
            -height,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
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

pub fn register_window_class(module: HINSTANCE) -> Result<(), String> {
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW).map_err(error)? };
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_DROPSHADOW,
        lpfnWndProc: Some(panel_proc),
        hInstance: module,
        hCursor: cursor,
        lpszClassName: w!("Volume3VolumePanel"),
        ..Default::default()
    };
    if unsafe { RegisterClassExW(&class) } == 0 {
        Err(format!(
            "RegisterClassExW(panel): {}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(())
    }
}

unsafe extern "system" fn panel_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_ERASEBKGND {
        return LRESULT(1);
    }
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Panel;
    if pointer.is_null() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    let panel = &mut *pointer;
    match message {
        WM_PAINT => {
            panel.paint(hwnd);
            return LRESULT(0);
        }
        WM_ACTIVATE if (wparam.0 as u32 & 0xffff) == WA_INACTIVE => {
            panel.close();
            return LRESULT(0);
        }
        WM_CLOSE => {
            panel.close();
            return LRESULT(0);
        }
        WM_LBUTTONDOWN => {
            let (x, y) = mouse_point(lparam);
            if y < HEADER_HEIGHT {
                panel.close();
            } else if let Some(target) = panel.target_at(x, y) {
                panel.dragging = Some(target);
                SetCapture(hwnd);
                SetTimer(Some(hwnd), 1, 25, None);
                panel.request_volume(target, x);
            }
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            if let Some(target) = panel.dragging {
                let (x, _) = mouse_point(lparam);
                panel.request_volume(target, x);
            }
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            if let Some(target) = panel.dragging.take() {
                let (x, _) = mouse_point(lparam);
                panel.request_volume(target, x);
                panel.flush_volume();
                let _ = KillTimer(Some(hwnd), 1);
                let _ = ReleaseCapture();
            }
            return LRESULT(0);
        }
        WM_CAPTURECHANGED => {
            if panel.dragging.take().is_some() {
                panel.flush_volume();
                let _ = KillTimer(Some(hwnd), 1);
            }
            return LRESULT(0);
        }
        WM_TIMER if wparam.0 == 1 => {
            panel.flush_volume();
            return LRESULT(0);
        }
        WM_MOUSEWHEEL => {
            let delta = (wparam.0 >> 16) as u16 as i16;
            let max_scroll = panel.rows.len().saturating_sub(panel.visible_rows());
            if delta > 0 {
                panel.scroll = panel.scroll.saturating_sub(2);
            } else if delta < 0 {
                panel.scroll = (panel.scroll + 2).min(max_scroll);
            }
            let _ = InvalidateRect(Some(hwnd), None, false);
            return LRESULT(0);
        }
        _ => {}
    }
    DefWindowProcW(hwnd, message, wparam, lparam)
}

fn intersects(clip: RECT, top: i32, bottom: i32) -> bool {
    clip.bottom > top && clip.top < bottom
}

fn mouse_point(lparam: LPARAM) -> (i32, i32) {
    (
        lparam.0 as u16 as i16 as i32,
        (lparam.0 >> 16) as u16 as i16 as i32,
    )
}

fn extract_process_icon(pid: u32) -> HICON {
    let Some(path) = audio::process_path(pid) else {
        return HICON::default();
    };
    let path = wide(&path);
    let mut icon = HICON::default();
    unsafe {
        ExtractIconExW(PCWSTR(path.as_ptr()), 0, Some(&mut icon), None, 1);
    }
    icon
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
    SelectObject(hdc, old_pen);
    SelectObject(hdc, old_brush);
    let _ = DeleteObject(HGDIOBJ(brush.0));
}

unsafe fn draw_fallback_icon(hdc: HDC, center: i32, name: &str, font: HFONT) {
    rounded(
        hdc,
        RECT {
            left: 23,
            top: center - 10,
            right: 43,
            bottom: center + 10,
        },
        rgb(48, 160, 206),
        10,
    );
    let initial = name
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string();
    draw_text(
        hdc,
        &initial,
        RECT {
            left: 23,
            top: center - 10,
            right: 43,
            bottom: center + 10,
        },
        rgb(255, 255, 255),
        font,
        DT_CENTER,
    );
}

unsafe fn draw_text(
    hdc: HDC,
    text: &str,
    mut rect: RECT,
    color: COLORREF,
    font: HFONT,
    align: DRAW_TEXT_FORMAT,
) {
    let old = SelectObject(hdc, HGDIOBJ(font.0));
    let _ = SetBkMode(hdc, TRANSPARENT);
    let _ = SetTextColor(hdc, color);
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    DrawTextW(
        hdc,
        &mut wide,
        &mut rect,
        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX | align,
    );
    SelectObject(hdc, old);
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF(u32::from(red) | (u32::from(green) << 8) | (u32::from(blue) << 16))
}
fn error(error: windows::core::Error) -> String {
    error.to_string()
}
