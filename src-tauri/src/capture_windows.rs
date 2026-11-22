use super::{parse_shortcut, selection, Selection, Shortcut};
use crate::models::{CaptureStatus, ImageInput};
use base64::{engine::general_purpose::STANDARD, Engine};
use image::{ImageFormat, RgbImage};
use std::{
    ffi::c_void,
    io::Cursor,
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
use tauri::{Emitter, Manager};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::{Dwm::DwmFlush, Gdi::*},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{
        HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::*,
    },
};

const CONFIGURE_MESSAGE: u32 = WM_APP + 18;
const RECORDING_TIMER: usize = 0x4d59;
const CONFLICT: &str = "这个快捷键已被 Windows 或其他应用占用，请换一组组合键。";
const UNAVAILABLE: &str = "截屏服务暂时不可用，请重新启动软件。";
const CAPTURE_FAILED: &str = "无法读取屏幕，请检查远程桌面或受保护窗口，并重试。";

struct Shared {
    status: Mutex<CaptureStatus>,
    capturing: AtomicBool,
}
struct CaptureRuntime {
    sender: mpsc::Sender<Command>,
    thread: u32,
    shared: Arc<Shared>,
}
enum Command {
    Configure(
        Option<Shortcut>,
        mpsc::Sender<Result<CaptureStatus, String>>,
    ),
    Recording(bool, mpsc::Sender<Result<(), String>>),
    Stop,
}
impl Drop for CaptureRuntime {
    fn drop(&mut self) {
        let _ = self.sender.send(Command::Stop);
        unsafe {
            PostThreadMessageW(self.thread, CONFIGURE_MESSAGE, 0, 0);
        }
    }
}

pub fn initialize(app: &tauri::AppHandle) -> Result<(), String> {
    if app.try_state::<CaptureRuntime>().is_some() {
        return Ok(());
    }
    let shared = Arc::new(Shared {
        status: Mutex::new(CaptureStatus {
            shortcut: String::new(),
            active: false,
            capturing: false,
            error: None,
        }),
        capturing: AtomicBool::new(false),
    });
    let (sender, receiver) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::channel();
    let app_copy = app.clone();
    let shared_copy = shared.clone();
    std::thread::Builder::new()
        .name("moyu-shortcut".into())
        .spawn(move || unsafe {
            let mut message: MSG = zeroed();
            // PeekMessage creates this thread's queue before its ID can be used by callers.
            PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE);
            let _ = ready_tx.send(GetCurrentThreadId());
            shortcut_loop(app_copy, shared_copy, receiver);
        })
        .map_err(|_| UNAVAILABLE.to_string())?;
    let thread = ready_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| UNAVAILABLE.to_string())?;
    app.manage(CaptureRuntime {
        sender,
        thread,
        shared,
    });
    Ok(())
}

fn dispatch(app: &tauri::AppHandle, command: Command) -> Result<(), String> {
    let runtime = app.try_state::<CaptureRuntime>().ok_or(UNAVAILABLE)?;
    runtime
        .sender
        .send(command)
        .map_err(|_| UNAVAILABLE.to_string())?;
    if unsafe { PostThreadMessageW(runtime.thread, CONFIGURE_MESSAGE, 0, 0) } == 0 {
        return Err(UNAVAILABLE.into());
    }
    Ok(())
}
pub fn configure(app: &tauri::AppHandle, shortcut: &str) -> Result<CaptureStatus, String> {
    let shortcut = parse_shortcut(shortcut)?;
    let (sender, receiver) = mpsc::channel();
    dispatch(app, Command::Configure(shortcut, sender))?;
    receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| UNAVAILABLE.to_string())?
}
pub fn set_recording(app: &tauri::AppHandle, recording: bool) -> Result<(), String> {
    let (sender, receiver) = mpsc::channel();
    dispatch(app, Command::Recording(recording, sender))?;
    receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| UNAVAILABLE.to_string())?
}
pub fn status(app: &tauri::AppHandle) -> Result<CaptureStatus, String> {
    let runtime = app.try_state::<CaptureRuntime>().ok_or(UNAVAILABLE)?;
    let mut status = runtime
        .shared
        .status
        .lock()
        .map_err(|_| UNAVAILABLE)?
        .clone();
    status.capturing = runtime.shared.capturing.load(Ordering::SeqCst);
    Ok(status)
}

struct Registration {
    current: Option<Shortcut>,
    id: i32,
    registered: bool,
    suspended: bool,
    timer: usize,
}
impl Registration {
    fn new() -> Self {
        Self {
            current: None,
            id: 100,
            registered: false,
            suspended: false,
            timer: 0,
        }
    }
    unsafe fn stop_timer(&mut self) {
        if self.timer != 0 {
            KillTimer(null_mut(), self.timer);
            self.timer = 0;
        }
    }
    unsafe fn register(shortcut: &Shortcut, id: i32) -> bool {
        RegisterHotKey(
            null_mut(),
            id,
            shortcut.modifiers | MOD_NOREPEAT,
            shortcut.key,
        ) != 0
    }
    unsafe fn replace(&mut self, next: Option<Shortcut>) -> Result<(), String> {
        self.suspended = false;
        self.stop_timer();
        if next == self.current {
            return self.resume();
        }
        if let Some(ref shortcut) = next {
            let new_id = if self.id == 100 { 101 } else { 100 };
            if !Self::register(shortcut, new_id) {
                // At startup, retain the requested key so Settings can explain why it is inactive.
                if self.current.is_none() {
                    self.current = next;
                } else if !self.registered {
                    let _ = self.resume();
                }
                return Err(CONFLICT.into());
            }
            if self.registered {
                UnregisterHotKey(null_mut(), self.id);
            }
            self.current = next;
            self.id = new_id;
            self.registered = true;
        } else {
            if self.registered {
                UnregisterHotKey(null_mut(), self.id);
            }
            self.current = None;
            self.registered = false;
        }
        Ok(())
    }
    unsafe fn resume(&mut self) -> Result<(), String> {
        self.suspended = false;
        self.stop_timer();
        if !self.registered {
            if let Some(ref shortcut) = self.current {
                if !Self::register(shortcut, self.id) {
                    return Err(CONFLICT.into());
                }
                self.registered = true;
            }
        }
        Ok(())
    }
    unsafe fn recording(&mut self, recording: bool) -> Result<(), String> {
        if recording {
            if self.registered {
                UnregisterHotKey(null_mut(), self.id);
            }
            self.registered = false;
            self.suspended = true;
            self.stop_timer();
            self.timer = SetTimer(null_mut(), RECORDING_TIMER, 300, None);
            if self.timer == 0 {
                return self.resume();
            }
            Ok(())
        } else {
            self.resume()
        }
    }
    fn publish(&self, shared: &Shared, error: Option<String>) -> CaptureStatus {
        let result = CaptureStatus {
            shortcut: self
                .current
                .as_ref()
                .map(|value| value.normalized.clone())
                .unwrap_or_default(),
            active: self.registered,
            capturing: shared.capturing.load(Ordering::SeqCst),
            error,
        };
        if let Ok(mut stored) = shared.status.lock() {
            *stored = result.clone();
        }
        result
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        unsafe {
            self.stop_timer();
            if self.registered {
                UnregisterHotKey(null_mut(), self.id);
            }
        }
    }
}
unsafe fn shortcut_loop(
    app: tauri::AppHandle,
    shared: Arc<Shared>,
    receiver: mpsc::Receiver<Command>,
) {
    let mut registration = Registration::new();
    let mut message: MSG = zeroed();
    while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
        if message.message == CONFIGURE_MESSAGE {
            for command in receiver.try_iter() {
                match command {
                    Command::Configure(next, sender) => {
                        let result = registration.replace(next);
                        let state = registration.publish(&shared, result.as_ref().err().cloned());
                        let _ = sender.send(result.map(|_| state));
                    }
                    Command::Recording(recording, sender) => {
                        let result = registration.recording(recording);
                        registration.publish(&shared, result.as_ref().err().cloned());
                        let _ = sender.send(result);
                    }
                    Command::Stop => return,
                }
            }
        } else if message.message == WM_TIMER
            && registration.suspended
            && message.wParam == registration.timer
        {
            let focused = app
                .get_webview_window("main")
                .and_then(|w| w.hwnd().ok())
                .map(|h| h.0 as HWND)
                .is_some_and(|hwnd| GetForegroundWindow() == hwnd && IsWindowVisible(hwnd) != 0);
            if !focused {
                let result = registration.resume();
                let status = registration.publish(&shared, result.err());
                let _ = app.emit_to("main", "capture-status", status);
            }
        } else if message.message == WM_HOTKEY
            && message.wParam == registration.id as usize
            && registration.registered
            && !shared.capturing.load(Ordering::SeqCst)
        {
            if let Err(error) = start_capture(&app) {
                let _ = app.emit_to("main", "capture-error", error);
            }
        }
    }
}

pub fn start_capture(app: &tauri::AppHandle) -> Result<(), String> {
    let runtime = app.try_state::<CaptureRuntime>().ok_or(UNAVAILABLE)?;
    if runtime
        .shared
        .capturing
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("正在截屏，请先完成框选或按 Esc 取消。".into());
    }
    let shared = runtime.shared.clone();
    let app_copy = app.clone();
    if std::thread::Builder::new()
        .name("moyu-capture".into())
        .spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                capture_and_restore(&app_copy)
            }));
            shared.capturing.store(false, Ordering::SeqCst);
            match outcome {
                Ok(Ok(Some(image))) => {
                    let _ = app_copy.emit_to("main", "capture-ready", image);
                }
                Ok(Ok(None)) => {}
                Ok(Err(error)) => {
                    let _ = app_copy.emit_to("main", "capture-error", error);
                }
                Err(_) => {
                    if let Some(window) = app_copy.get_webview_window("main") {
                        let _ = window.show();
                    }
                    let _ = app_copy.emit_to("main", "capture-error", CAPTURE_FAILED);
                }
            }
        })
        .is_err()
    {
        runtime.shared.capturing.store(false, Ordering::SeqCst);
        return Err(UNAVAILABLE.into());
    }
    Ok(())
}

fn capture_and_restore(app: &tauri::AppHandle) -> Result<Option<ImageInput>, String> {
    let main = app.get_webview_window("main").ok_or(UNAVAILABLE)?;
    let visible = main.is_visible().unwrap_or(false);
    let minimized = main.is_minimized().unwrap_or(false);
    let focused = main.is_focused().unwrap_or(false);
    if visible {
        main.hide().map_err(|_| CAPTURE_FAILED)?;
    }
    let result = unsafe {
        // DWM flushes the hide before the static desktop snapshot is taken.
        DwmFlush();
        capture_region()
    };
    match &result {
        Ok(Some(_)) => {
            let _ = main.show();
            let _ = main.unminimize();
            let _ = main.set_focus();
        }
        Err(_) => {
            let _ = main.show();
            let _ = main.unminimize();
            let _ = main.set_focus();
        }
        _ if visible => {
            let _ = main.show();
            if minimized {
                let _ = main.minimize();
            } else if focused {
                let _ = main.set_focus();
            }
        }
        _ => {}
    }
    result
}

struct DpiGuard(isize);
impl DpiGuard {
    unsafe fn new() -> Self {
        Self(SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) as isize)
    }
}
impl Drop for DpiGuard {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe {
                SetThreadDpiAwarenessContext(self.0 as _);
            }
        }
    }
}
struct ScreenDc(HDC);
impl ScreenDc {
    unsafe fn new() -> Result<Self, String> {
        let dc = GetDC(null_mut());
        if dc.is_null() {
            Err(CAPTURE_FAILED.into())
        } else {
            Ok(Self(dc))
        }
    }
}
impl Drop for ScreenDc {
    fn drop(&mut self) {
        unsafe {
            ReleaseDC(null_mut(), self.0);
        }
    }
}

struct DibSurface {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    width: i32,
    height: i32,
}
impl DibSurface {
    unsafe fn new(source: HDC, width: i32, height: i32) -> Result<Self, String> {
        let bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or(CAPTURE_FAILED)?;
        // Two DIBs are used by the selector. Reject pathological desktop sizes before allocation.
        if width <= 0 || height <= 0 || bytes > 256 * 1024 * 1024 {
            return Err("显示器总分辨率过大，请暂时减少显示器后重试。".into());
        }
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width;
        info.bmiHeader.biHeight = -height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let dc = CreateCompatibleDC(source);
        if dc.is_null() {
            return Err(CAPTURE_FAILED.into());
        }
        let mut bits: *mut c_void = null_mut();
        let bitmap = CreateDIBSection(source, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if bitmap.is_null() || bits.is_null() {
            DeleteDC(dc);
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            return Err(CAPTURE_FAILED.into());
        }
        let old = SelectObject(dc, bitmap);
        if old.is_null() || old as isize == -1 {
            DeleteObject(bitmap);
            DeleteDC(dc);
            return Err(CAPTURE_FAILED.into());
        }
        Ok(Self {
            dc,
            bitmap,
            old,
            bits: bits.cast(),
            width,
            height,
        })
    }
    unsafe fn bytes(&self) -> &[u8] {
        std::slice::from_raw_parts(self.bits, self.width as usize * self.height as usize * 4)
    }
    unsafe fn bytes_mut(&mut self) -> &mut [u8] {
        std::slice::from_raw_parts_mut(self.bits, self.width as usize * self.height as usize * 4)
    }
    unsafe fn png(&self, area: Selection) -> Result<ImageInput, String> {
        if area.left < 0
            || area.top < 0
            || area.width <= 0
            || area.height <= 0
            || area
                .left
                .checked_add(area.width)
                .is_none_or(|right| right > self.width)
            || area
                .top
                .checked_add(area.height)
                .is_none_or(|bottom| bottom > self.height)
        {
            return Err("截图选区无效，请重新框选。".into());
        }
        if area.width > 8192 || area.height > 8192 {
            return Err("所选区域每边最多 8192 像素，请缩小选区后重试。".into());
        }
        let mut rgb = RgbImage::new(area.width as u32, area.height as u32);
        let source = self.bytes();
        for y in 0..area.height as usize {
            for x in 0..area.width as usize {
                let offset =
                    ((area.top as usize + y) * self.width as usize + area.left as usize + x) * 4;
                rgb.put_pixel(
                    x as u32,
                    y as u32,
                    image::Rgb([source[offset + 2], source[offset + 1], source[offset]]),
                );
            }
        }
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(rgb)
            .write_to(&mut bytes, ImageFormat::Png)
            .map_err(|_| CAPTURE_FAILED)?;
        if bytes.get_ref().len() > 32 * 1024 * 1024 {
            return Err("截图超过 32 MiB，请缩小选区后重试。".into());
        }
        Ok(ImageInput {
            data_url: format!(
                "data:image/png;base64,{}",
                STANDARD.encode(bytes.into_inner())
            ),
            name: format!("截图-{}.png", chrono::Local::now().format("%Y%m%d-%H%M%S")),
        })
    }
}
impl Drop for DibSurface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

struct Overlay {
    original: DibSurface,
    dimmed: DibSurface,
    origin: (i32, i32),
    start: Option<(i32, i32)>,
    end: (i32, i32),
    selected: Option<Selection>,
    finishing: bool,
    #[cfg(test)]
    cursor_override: Option<(i32, i32)>,
}
impl Overlay {
    unsafe fn cursor(&self) -> (i32, i32) {
        #[cfg(test)]
        if let Some(point) = self.cursor_override {
            return point;
        }
        let mut point: POINT = zeroed();
        GetCursorPos(&mut point);
        (
            point
                .x
                .saturating_sub(self.origin.0)
                .clamp(0, self.original.width),
            point
                .y
                .saturating_sub(self.origin.1)
                .clamp(0, self.original.height),
        )
    }
    fn area(&self) -> Option<Selection> {
        self.start
            .and_then(|start| selection(start, self.end, self.original.width, self.original.height))
    }
    // Win32 synchronously re-enters the window procedure during ReleaseCapture/DestroyWindow.
    // Use the owning raw pointer here so no live &mut reference spans those callbacks.
    unsafe fn finish(pointer: *mut Self, hwnd: HWND, area: Option<Selection>) {
        if (*pointer).finishing {
            return;
        }
        (*pointer).finishing = true;
        (*pointer).selected = area;
        if GetCapture() == hwnd {
            ReleaseCapture();
        }
        DestroyWindow(hwnd);
    }
    unsafe fn paint(&self, hwnd: HWND) {
        let mut paint: PAINTSTRUCT = zeroed();
        let dc = BeginPaint(hwnd, &mut paint);
        if dc.is_null() {
            return;
        }
        BitBlt(
            dc,
            0,
            0,
            self.original.width,
            self.original.height,
            self.dimmed.dc,
            0,
            0,
            SRCCOPY,
        );
        if let Some(area) = self.area() {
            BitBlt(
                dc,
                area.left,
                area.top,
                area.width,
                area.height,
                self.original.dc,
                area.left,
                area.top,
                SRCCOPY,
            );
            let pen = CreatePen(PS_SOLID, 2, 0x00ff7a00);
            let old_pen = SelectObject(dc, pen);
            let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
            Rectangle(
                dc,
                area.left,
                area.top,
                area.left + area.width,
                area.top + area.height,
            );
            SelectObject(dc, old_brush);
            SelectObject(dc, old_pen);
            DeleteObject(pen);
        }
        let point = self.cursor();
        let x = (point.0 + 18).clamp(8, (self.original.width - 390).max(8));
        let y = (point.1 + 24).clamp(8, (self.original.height - 48).max(8));
        let mut rect = RECT {
            left: x,
            top: y,
            right: x + 380,
            bottom: y + 34,
        };
        let brush = CreateSolidBrush(0x00302a24);
        FillRect(dc, &rect, brush);
        DeleteObject(brush);
        SetTextColor(dc, 0x00ffffff);
        SetBkMode(dc, TRANSPARENT as i32);
        rect.left += 10;
        let label = self
            .area()
            .map(|a| format!("{} × {}   松开翻译 · Esc / 右键取消", a.width, a.height))
            .unwrap_or_else(|| "拖动框选 · 松开翻译 · Esc / 右键取消".into());
        let label: Vec<u16> = label.encode_utf16().collect();
        DrawTextW(
            dc,
            label.as_ptr(),
            label.len() as i32,
            &mut rect,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );
        EndPaint(hwnd, &paint);
    }
}
unsafe extern "system" fn overlay_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let creation = &*(lparam as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, creation.lpCreateParams as isize);
    }
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Overlay;
    if pointer.is_null() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    match message {
        WM_PAINT => {
            (*pointer).paint(hwnd);
            0
        }
        WM_ERASEBKGND => 1,
        WM_LBUTTONDOWN if !(*pointer).finishing => {
            let point = (*pointer).cursor();
            (*pointer).start = Some(point);
            (*pointer).end = point;
            SetCapture(hwnd);
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_MOUSEMOVE if !(*pointer).finishing => {
            (*pointer).end = (*pointer).cursor();
            InvalidateRect(hwnd, null(), 0);
            UpdateWindow(hwnd);
            0
        }
        WM_LBUTTONUP if !(*pointer).finishing => {
            (*pointer).end = (*pointer).cursor();
            let area = (*pointer).area();
            if area.is_some() {
                Overlay::finish(pointer, hwnd, area);
            } else {
                (*pointer).start = None;
                if GetCapture() == hwnd {
                    ReleaseCapture();
                }
            }
            0
        }
        WM_RBUTTONUP | WM_CLOSE | WM_CANCELMODE | WM_DISPLAYCHANGE => {
            Overlay::finish(pointer, hwnd, None);
            0
        }
        WM_CAPTURECHANGED if !(*pointer).finishing && (*pointer).start.is_some() => {
            Overlay::finish(pointer, hwnd, None);
            0
        }
        WM_KILLFOCUS if !(*pointer).finishing => {
            Overlay::finish(pointer, hwnd, None);
            0
        }
        WM_KEYDOWN | WM_SYSKEYDOWN if wparam == 0x1b => {
            Overlay::finish(pointer, hwnd, None);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn capture_region() -> Result<Option<ImageInput>, String> {
    let _dpi = DpiGuard::new();
    let origin = (
        GetSystemMetrics(SM_XVIRTUALSCREEN),
        GetSystemMetrics(SM_YVIRTUALSCREEN),
    );
    let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
    let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
    let screen = ScreenDc::new()?;
    let original = DibSurface::new(screen.0, width, height)?;
    if BitBlt(
        original.dc,
        0,
        0,
        width,
        height,
        screen.0,
        origin.0,
        origin.1,
        SRCCOPY | CAPTUREBLT,
    ) == 0
    {
        return Err(CAPTURE_FAILED.into());
    }
    GdiFlush();
    let mut dimmed = DibSurface::new(screen.0, width, height)?;
    for (source, target) in original
        .bytes()
        .chunks_exact(4)
        .zip(dimmed.bytes_mut().chunks_exact_mut(4))
    {
        target[0] = (source[0] as u16 * 46 / 100) as u8;
        target[1] = (source[1] as u16 * 46 / 100) as u8;
        target[2] = (source[2] as u16 * 46 / 100) as u8;
        target[3] = 255;
    }
    let mut overlay = Box::new(Overlay {
        original,
        dimmed,
        origin,
        start: None,
        end: (0, 0),
        selected: None,
        finishing: false,
        #[cfg(test)]
        cursor_override: None,
    });
    let class: Vec<u16> = format!("MoyuCapture-{}", GetCurrentThreadId())
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let instance = GetModuleHandleW(null());
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(overlay_proc),
        hInstance: instance,
        lpszClassName: class.as_ptr(),
        hCursor: LoadCursorW(null_mut(), IDC_CROSS),
        ..zeroed()
    };
    if RegisterClassW(&window_class) == 0 {
        return Err(CAPTURE_FAILED.into());
    }
    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
        class.as_ptr(),
        class.as_ptr(),
        WS_POPUP,
        origin.0,
        origin.1,
        width,
        height,
        null_mut(),
        null_mut(),
        instance,
        (&mut *overlay as *mut Overlay).cast(),
    );
    if hwnd.is_null() {
        UnregisterClassW(class.as_ptr(), instance);
        return Err(CAPTURE_FAILED.into());
    }
    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
    SetFocus(hwnd);
    UpdateWindow(hwnd);
    let mut message: MSG = zeroed();
    while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    if IsWindow(hwnd) != 0 {
        Overlay::finish(&mut *overlay, hwnd, None);
    }
    UnregisterClassW(class.as_ptr(), instance);
    overlay
        .selected
        .map(|area| overlay.original.png(area))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dib_crop_uses_exact_selected_pixels_and_valid_png() {
        unsafe {
            let screen = ScreenDc::new().unwrap();
            let mut bitmap = DibSurface::new(screen.0, 120, 80).unwrap();
            for (index, pixel) in bitmap.bytes_mut().chunks_exact_mut(4).enumerate() {
                pixel.copy_from_slice(&[(index % 120) as u8, (index / 120) as u8, 200, 0]);
            }
            let result = bitmap
                .png(Selection {
                    left: 17,
                    top: 22,
                    width: 32,
                    height: 19,
                })
                .unwrap();
            let bytes = STANDARD
                .decode(result.data_url.split_once(',').unwrap().1)
                .unwrap();
            let decoded = image::load_from_memory(&bytes).unwrap().to_rgb8();
            assert_eq!(decoded.dimensions(), (32, 19));
            assert_eq!(decoded.get_pixel(0, 0).0, [200, 22, 17]);
            assert_eq!(decoded.get_pixel(31, 18).0, [200, 40, 48]);
            assert_eq!(bitmap.bytes().len(), 120 * 80 * 4);
        }
    }
    #[test]
    fn native_selector_cancels_and_crops_synthetic_pixels_without_reading_desktop() {
        std::thread::spawn(|| unsafe {
            let screen = ScreenDc::new().unwrap();
            let class: Vec<u16> = format!("MoyuSelectorTest-{}", GetCurrentThreadId())
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let instance = GetModuleHandleW(null());
            let window_class = WNDCLASSW {
                lpfnWndProc: Some(overlay_proc),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..zeroed()
            };
            assert_ne!(RegisterClassW(&window_class), 0);
            let modes = [
                WM_KEYDOWN,
                WM_RBUTTONUP,
                WM_DISPLAYCHANGE,
                WM_CANCELMODE,
                WM_KILLFOCUS,
                WM_CAPTURECHANGED,
                WM_LBUTTONUP,
            ];
            for mode in modes {
                let mut original = DibSurface::new(screen.0, 120, 80).unwrap();
                for pixel in original.bytes_mut().chunks_exact_mut(4) {
                    pixel.copy_from_slice(&[30, 80, 160, 255]);
                }
                let dimmed = DibSurface::new(screen.0, 120, 80).unwrap();
                let mut overlay = Box::new(Overlay {
                    original,
                    dimmed,
                    origin: (-1920, -200),
                    start: Some((10, 12)),
                    end: (10, 12),
                    selected: None,
                    finishing: false,
                    cursor_override: Some((42, 31)),
                });
                // Hidden test-owned window: no desktop pixels, foreground changes, real input or clipboard access.
                let hwnd = CreateWindowExW(
                    WS_EX_TOOLWINDOW,
                    class.as_ptr(),
                    class.as_ptr(),
                    WS_POPUP,
                    -10000,
                    -10000,
                    120,
                    80,
                    null_mut(),
                    null_mut(),
                    instance,
                    (&mut *overlay as *mut Overlay).cast(),
                );
                assert!(!hwnd.is_null());
                if mode == WM_LBUTTONUP {
                    SendMessageW(hwnd, mode, 0, 0);
                    let selected = overlay.selected.unwrap();
                    assert_eq!(
                        selected,
                        Selection {
                            left: 10,
                            top: 12,
                            width: 32,
                            height: 19
                        }
                    );
                    let image = overlay.original.png(selected).unwrap();
                    let bytes = STANDARD
                        .decode(image.data_url.split_once(',').unwrap().1)
                        .unwrap();
                    let decoded = image::load_from_memory(&bytes).unwrap().to_rgb8();
                    assert_eq!(decoded.dimensions(), (32, 19));
                    assert!(decoded.pixels().all(|pixel| pixel.0 == [160, 80, 30]));
                } else {
                    SendMessageW(hwnd, mode, if mode == WM_KEYDOWN { 0x1b } else { 0 }, 0);
                    assert!(overlay.selected.is_none());
                }
                assert!(overlay.finishing);
                assert_eq!(IsWindow(hwnd), 0);
                let mut message: MSG = zeroed();
                while PeekMessageW(&mut message, null_mut(), WM_QUIT, WM_QUIT, PM_REMOVE) != 0 {}
            }
            assert_ne!(UnregisterClassW(class.as_ptr(), instance), 0);
        })
        .join()
        .unwrap();
    }
    #[test]
    #[ignore = "Registers temporary uncommon hotkeys on isolated test threads; no input is injected"]
    fn native_hotkey_conflict_preserves_old_key_and_disable_releases_it() {
        std::thread::spawn(|| unsafe {
            let mut message: MSG = zeroed();
            PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE);
            let mut own = Registration::new();
            let first = parse_shortcut("Ctrl+Alt+Shift+F22").unwrap();
            own.replace(first.clone())
                .expect("Temporary test key is occupied; no existing binding was changed");
            let (ready_tx, ready_rx) = mpsc::channel();
            let (done_tx, done_rx) = mpsc::channel();
            let other = std::thread::spawn(move || {
                let mut other = Registration::new();
                let second = parse_shortcut("Ctrl+Alt+Shift+F23").unwrap();
                let registered = other.replace(second).is_ok();
                ready_tx.send(registered).unwrap();
                done_rx.recv().unwrap();
            });
            let second_available = ready_rx.recv().unwrap();
            assert!(
                second_available,
                "Second temporary test key was unavailable"
            );
            if second_available {
                assert!(own
                    .replace(parse_shortcut("Ctrl+Alt+Shift+F23").unwrap())
                    .is_err());
                assert_eq!(own.current, first);
                assert!(own.registered);
            }
            let key = first.clone().unwrap();
            assert!(!std::thread::spawn(move || {
                let mut other = Registration::new();
                other.replace(Some(key)).is_ok()
            })
            .join()
            .unwrap());
            own.replace(None).unwrap();
            let key = first.clone().unwrap();
            assert!(std::thread::spawn(move || {
                let mut other = Registration::new();
                other.replace(Some(key)).is_ok()
            })
            .join()
            .unwrap());
            own.replace(first.clone()).unwrap();
            own.recording(true).unwrap();
            assert!(!own.registered && own.suspended && own.timer != 0);
            let key = first.unwrap();
            assert!(std::thread::spawn(move || {
                let mut other = Registration::new();
                other.replace(Some(key)).is_ok()
            })
            .join()
            .unwrap());
            own.recording(false).unwrap();
            assert!(own.registered && !own.suspended && own.timer == 0);
            done_tx.send(()).unwrap();
            other.join().unwrap();
        })
        .join()
        .unwrap();
    }
}
