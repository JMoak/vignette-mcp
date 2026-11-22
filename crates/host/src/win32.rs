//! Every Win32 call the host makes. `unsafe` appears nowhere else in the crate.

use std::ffi::c_void;
use std::ptr::{null, null_mut};

use vignette_core::lattice::Rect;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWM_TIMING_INFO, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmFlush,
    DwmGetCompositionTimingInfo, DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CLEARTYPE_QUALITY, CreateCompatibleBitmap,
    CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush, DEFAULT_CHARSET, DIB_RGB_COLORS,
    DT_CALCRECT, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_TOP, DT_WORDBREAK, DeleteDC,
    DeleteObject, DrawTextW, EndPaint, FillRect, GdiFlush, PAINTSTRUCT, SRCCOPY, SelectObject, SetBkMode,
    SetTextColor, SetViewportOrgEx, TRANSPARENT, UpdateWindow,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{
    AreDpiAwarenessContextsEqual, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem,
    GetThreadDpiAwarenessContext, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BeginDeferWindowPos, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DeferWindowPos,
    DestroyWindow, DispatchMessageW, EndDeferWindowPos, GWLP_USERDATA, GetClientRect, GetWindowLongPtrW,
    IDC_ARROW, LoadCursorW, MA_NOACTIVATE, MSG, PM_REMOVE, PeekMessageW, RegisterClassExW, SPI_GETWORKAREA,
    SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW,
    ShowWindow, SystemParametersInfoW, TranslateMessage, WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_PAINT, WM_QUIT,
    WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
};

use crate::look::Rgb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window(pub isize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Title,
    Small,
    Prose,
    Heading,
}

pub struct Fonts {
    title: isize,
    small: isize,
    prose: isize,
    heading: isize,
}

pub trait Painted {
    fn paint(&self, canvas: &Canvas, width: i32, height: i32);
}

pub struct Canvas {
    dc: isize,
    fonts: *const Fonts,
}

const CLASS: &str = "VignettePane";

const WITHOUT_FOCUS: u32 = SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE;

pub fn enable_per_monitor_dpi() -> bool {
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let current = unsafe { GetThreadDpiAwarenessContext() };
    unsafe { AreDpiAwarenessContextsEqual(current, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) != 0 }
}

pub fn scale() -> f64 {
    f64::from(unsafe { GetDpiForSystem() }) / 96.0
}

pub fn work_area() -> Rect {
    let mut area = RECT::default();
    unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, (&raw mut area).cast(), 0) };
    Rect { x: area.left, y: area.top, w: area.right - area.left, h: area.bottom - area.top }
}

pub fn fonts(scale: f64) -> Fonts {
    let sized = |points: f64| -(points * scale * 96.0 / 72.0).round() as i32;
    Fonts {
        title: font("Cascadia Mono", sized(10.0), 400),
        small: font("Cascadia Mono", sized(9.0), 400),
        prose: font("Segoe UI", sized(11.0), 400),
        heading: font("Segoe UI", sized(14.0), 600),
    }
}

pub fn register_class() -> bool {
    let name = wide(CLASS);
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(pane_proc),
        hInstance: unsafe { GetModuleHandleW(null()) },
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        lpszClassName: name.as_ptr(),
        ..Default::default()
    };
    unsafe { RegisterClassExW(&class) != 0 }
}

/// Creates a frameless pane that never takes focus. `painted` must outlive the window.
pub fn create_pane(bounds: Rect, painted: *const PaneFace) -> Option<Window> {
    let name = wide(CLASS);
    let window = unsafe {
        CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            name.as_ptr(),
            name.as_ptr(),
            WS_POPUP,
            bounds.x,
            bounds.y,
            bounds.w,
            bounds.h,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        )
    };
    if window.is_null() {
        return None;
    }
    unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, painted as isize) };
    let round = DWMWCP_ROUND;
    unsafe {
        DwmSetWindowAttribute(
            window,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            (&raw const round).cast(),
            size_of_val(&round) as u32,
        )
    };
    Some(Window(window as isize))
}

pub fn set_border(window: Window, colour: Rgb) {
    let colour = colorref(colour);
    unsafe {
        DwmSetWindowAttribute(
            handle(window),
            DWMWA_BORDER_COLOR as u32,
            (&raw const colour).cast(),
            size_of_val(&colour) as u32,
        )
    };
}

pub fn show_without_focus(window: Window) {
    unsafe { ShowWindow(handle(window), SW_SHOWNOACTIVATE) };
    unsafe { UpdateWindow(handle(window)) };
}

pub fn destroy(window: Window) {
    unsafe { SetWindowLongPtrW(handle(window), GWLP_USERDATA, 0) };
    unsafe { DestroyWindow(handle(window)) };
}

pub fn move_as_one_batch(moves: &[(Window, Rect)], sized: bool) -> bool {
    let flags = if sized { WITHOUT_FOCUS } else { WITHOUT_FOCUS | SWP_NOSIZE };
    let mut batch = unsafe { BeginDeferWindowPos(moves.len() as i32) };
    for (window, to) in moves {
        if batch.is_null() {
            return false;
        }
        batch = unsafe { DeferWindowPos(batch, handle(*window), null_mut(), to.x, to.y, to.w, to.h, flags) };
    }
    !batch.is_null() && unsafe { EndDeferWindowPos(batch) } != 0
}

pub fn wait_for_composition() -> bool {
    unsafe { DwmFlush() >= 0 }
}

pub fn refresh_period_us() -> Option<u64> {
    let mut timing = DWM_TIMING_INFO { cbSize: size_of::<DWM_TIMING_INFO>() as u32, ..Default::default() };
    let known = unsafe { DwmGetCompositionTimingInfo(null_mut(), &mut timing) } >= 0;
    let rate = timing.rateRefresh;
    (known && rate.uiNumerator > 0)
        .then(|| 1_000_000 * u64::from(rate.uiDenominator) / u64::from(rate.uiNumerator))
}

/// Handles every waiting message. False once the thread was asked to quit.
pub fn pump() -> bool {
    let mut message = MSG::default();
    while unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
        if message.message == WM_QUIT {
            return false;
        }
        unsafe { TranslateMessage(&message) };
        unsafe { DispatchMessageW(&message) };
    }
    true
}

/// Paints each face where it stands into one picture of `area` and returns it as a BMP file.
pub fn picture_of(area: Rect, backdrop: Rgb, fonts: &Fonts, faces: &[(Rect, &PaneFace)]) -> Vec<u8> {
    let header = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: area.w,
        biHeight: area.h,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        ..Default::default()
    };
    let info = BITMAPINFO { bmiHeader: header, ..Default::default() };
    let mut bits: *mut c_void = null_mut();
    let dc = unsafe { CreateCompatibleDC(null_mut()) };
    let bitmap = unsafe { CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0) };
    if bitmap.is_null() || bits.is_null() {
        unsafe { DeleteDC(dc) };
        return Vec::new();
    }
    let before = unsafe { SelectObject(dc, bitmap) };
    let canvas = Canvas { dc: dc as isize, fonts };
    canvas.fill(Rect { x: 0, y: 0, w: area.w, h: area.h }, backdrop);
    for (bounds, face) in faces {
        unsafe { SetViewportOrgEx(dc, bounds.x - area.x, bounds.y - area.y, null_mut()) };
        face.painted.paint(&canvas, bounds.w, bounds.h);
    }
    unsafe { GdiFlush() };
    let size = (area.w * area.h * 4) as usize;
    let pixels = unsafe { std::slice::from_raw_parts(bits.cast::<u8>(), size) };
    let mut file = Vec::with_capacity(54 + size);
    file.extend_from_slice(b"BM");
    file.extend_from_slice(&(54 + size as u32).to_le_bytes());
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(&54u32.to_le_bytes());
    file.extend_from_slice(&40u32.to_le_bytes());
    file.extend_from_slice(&area.w.to_le_bytes());
    file.extend_from_slice(&area.h.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&32u16.to_le_bytes());
    file.extend_from_slice(&[0u8; 24]);
    file.extend_from_slice(pixels);
    unsafe { SelectObject(dc, before) };
    unsafe { DeleteObject(bitmap) };
    unsafe { DeleteDC(dc) };
    file
}

/// What a window paints: the fonts it draws with and the thing that draws.
pub struct PaneFace {
    pub fonts: *const Fonts,
    pub painted: Box<dyn Painted>,
}

impl Canvas {
    pub fn fill(&self, area: Rect, colour: Rgb) {
        let rect = RECT { left: area.x, top: area.y, right: area.x + area.w, bottom: area.y + area.h };
        let brush = unsafe { CreateSolidBrush(colorref(colour)) };
        unsafe { FillRect(self.dc as _, &rect, brush) };
        unsafe { DeleteObject(brush) };
    }

    /// Draws one line, cut short with an ellipsis, and returns its width.
    pub fn line(&self, at: Rect, words: &str, face: Face, colour: Rgb) -> i32 {
        self.draw(at, words, face, colour, DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS);
        self.measured(at.w, words, face, DT_SINGLELINE | DT_NOPREFIX).0.min(at.w)
    }

    /// Draws wrapped words and returns the height they took.
    pub fn wrapped(&self, at: Rect, words: &str, face: Face, colour: Rgb) -> i32 {
        self.draw(at, words, face, colour, DT_LEFT | DT_TOP | DT_WORDBREAK | DT_NOPREFIX)
    }

    pub fn width_of(&self, words: &str, face: Face) -> i32 {
        self.measured(i32::MAX / 4, words, face, DT_SINGLELINE | DT_NOPREFIX).0
    }

    pub fn height_of(&self, width: i32, words: &str, face: Face) -> i32 {
        self.measured(width, words, face, DT_WORDBREAK | DT_NOPREFIX).1
    }

    fn measured(&self, width: i32, words: &str, face: Face, format: u32) -> (i32, i32) {
        let text = wide(words);
        let mut rect = RECT { left: 0, top: 0, right: width, bottom: 0 };
        let before = unsafe { SelectObject(self.dc as _, self.font(face) as _) };
        unsafe { DrawTextW(self.dc as _, text.as_ptr(), -1, &mut rect, format | DT_CALCRECT) };
        unsafe { SelectObject(self.dc as _, before) };
        (rect.right - rect.left, rect.bottom - rect.top)
    }

    fn draw(&self, at: Rect, words: &str, face: Face, colour: Rgb, format: u32) -> i32 {
        let text = wide(words);
        let mut rect = RECT { left: at.x, top: at.y, right: at.x + at.w, bottom: at.y + at.h };
        let before = unsafe { SelectObject(self.dc as _, self.font(face) as _) };
        unsafe { SetBkMode(self.dc as _, TRANSPARENT as _) };
        unsafe { SetTextColor(self.dc as _, colorref(colour)) };
        let height = unsafe { DrawTextW(self.dc as _, text.as_ptr(), -1, &mut rect, format) };
        unsafe { SelectObject(self.dc as _, before) };
        height
    }

    fn font(&self, face: Face) -> isize {
        let fonts = unsafe { &*self.fonts };
        match face {
            Face::Title => fonts.title,
            Face::Small => fonts.small,
            Face::Prose => fonts.prose,
            Face::Heading => fonts.heading,
        }
    }
}

unsafe extern "system" fn pane_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_ERASEBKGND => 1,
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_PAINT => {
            paint(window);
            0
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

fn paint(window: HWND) {
    let mut painting = PAINTSTRUCT::default();
    let screen = unsafe { BeginPaint(window, &mut painting) };
    let face = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *const PaneFace;
    let mut client = RECT::default();
    unsafe { GetClientRect(window, &mut client) };
    let (width, height) = (client.right - client.left, client.bottom - client.top);
    if !face.is_null() && width > 0 && height > 0 {
        let face = unsafe { &*face };
        let memory = unsafe { CreateCompatibleDC(screen) };
        let bitmap = unsafe { CreateCompatibleBitmap(screen, width, height) };
        let before = unsafe { SelectObject(memory, bitmap) };
        face.painted.paint(&Canvas { dc: memory as isize, fonts: face.fonts }, width, height);
        unsafe { BitBlt(screen, 0, 0, width, height, memory, 0, 0, SRCCOPY) };
        unsafe { SelectObject(memory, before) };
        unsafe { DeleteObject(bitmap) };
        unsafe { DeleteDC(memory) };
    }
    unsafe { EndPaint(window, &painting) };
}

fn font(face: &str, height: i32, weight: i32) -> isize {
    let name = wide(face);
    let made = unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            0,
            0,
            CLEARTYPE_QUALITY as u32,
            0,
            name.as_ptr(),
        )
    };
    made as isize
}

fn handle(window: Window) -> HWND {
    window.0 as HWND
}

fn colorref(Rgb(red, green, blue): Rgb) -> u32 {
    u32::from(red) | u32::from(green) << 8 | u32::from(blue) << 16
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
