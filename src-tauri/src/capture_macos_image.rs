//! The native selector runs only after an explicit button or hotkey request.
//! Its callback borrows the selected PNG without a file or a pasteboard round trip.
use crate::models::ImageInput;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{
    ffi::c_void,
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::{Emitter, Manager};

const MAX_PNG_BYTES: usize = 32 * 1024 * 1024;
const CAPTURE_ERROR: &str = "无法读取所选区域，请检查显示器连接和录屏权限后重试。";
static CAPTURING: AtomicBool = AtomicBool::new(false);

extern "C" {
    fn moyu_capture_region(
        main_window: *mut c_void,
        context: *mut c_void,
        receive: extern "C" fn(*mut c_void, *const u8, usize),
    ) -> i32;
}

pub(super) fn is_capturing() -> bool {
    CAPTURING.load(Ordering::Acquire)
}

pub(super) fn start_capture(app: &tauri::AppHandle) -> Result<(), String> {
    if CAPTURING.swap(true, Ordering::AcqRel) {
        return Err("正在截屏，请先完成框选或按 Esc 取消。".into());
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("moyu-mac-capture".into())
        .spawn(move || {
            let captured = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| capture(&app)))
                .unwrap_or_else(|_| Err(CAPTURE_ERROR.into()));
            CAPTURING.store(false, Ordering::Release);
            match captured {
                Ok(Some(image)) => {
                    let _ = app.emit_to("main", "capture-ready", image);
                }
                Ok(None) => {}
                Err(message) => {
                    crate::tray::show_main(&app);
                    let _ = app.emit_to("main", "capture-error", message);
                }
            }
        });
    if spawned.is_err() {
        CAPTURING.store(false, Ordering::Release);
        return Err("截屏服务暂时不可用，请重试。".into());
    }
    Ok(())
}

fn capture(app: &tauri::AppHandle) -> Result<Option<ImageInput>, String> {
    let main = app.get_webview_window("main").ok_or(CAPTURE_ERROR)?;
    let window = main.ns_window().map_err(|_| CAPTURE_ERROR)?;
    let mut image: Option<ImageInput> = None;
    // The native entry point is synchronous on this worker. It dispatches AppKit
    // work to the main thread and borrows the Rust output only inside receive_png.
    let status = unsafe {
        moyu_capture_region(
            window,
            (&mut image as *mut Option<ImageInput>).cast(),
            receive_png,
        )
    };
    capture_result(status, image)
}

extern "C" fn receive_png(context: *mut c_void, bytes: *const u8, length: usize) {
    if bytes.is_null() || length == 0 || length > MAX_PNG_BYTES {
        return;
    }
    // Native holds the encoded NSData until this callback returns.
    let png = unsafe { std::slice::from_raw_parts(bytes, length) };
    let image = ImageInput {
        data_url: format!("data:image/png;base64,{}", STANDARD.encode(png)),
        name: format!("截图-{}.png", chrono::Local::now().format("%Y%m%d-%H%M%S")),
    };
    unsafe { *context.cast::<Option<ImageInput>>() = Some(image) };
}

fn capture_result(status: i32, image: Option<ImageInput>) -> Result<Option<ImageInput>, String> {
    match status {
        0 => Ok(None),
        1 => image.map(Some).ok_or_else(|| CAPTURE_ERROR.into()),
        2 => Err("请在「系统设置 → 隐私与安全 → 录屏与系统录音」（旧版为「屏幕录制」）中允许墨译，然后完全退出并重新打开应用。".into()),
        3 => Err("所选区域过大，请缩小选区。图片每边最多 8192 像素，PNG 最大 32 MiB。".into()),
        _ => Err(CAPTURE_ERROR.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" {
        fn moyu_display_region(
            width: f64,
            height: f64,
            scale: f64,
            x1: f64,
            y1: f64,
            x2: f64,
            y2: f64,
            rect: *mut f64,
        ) -> bool;
    }

    #[test]
    fn native_selection_clips_reverse_drags_and_maps_retina_points() {
        let mut rect = [0.0; 4];
        let select = |coordinates: [f64; 4], output: &mut [f64; 4]| unsafe {
            let [x1, y1, x2, y2] = coordinates;
            moyu_display_region(200.0, 160.0, 2.0, x1, y1, x2, y2, output.as_mut_ptr())
        };
        assert!(select([184.1, 130.1, -30.0, 20.3], &mut rect));
        assert_eq!(rect, [0.0, 30.0, 184.0, 109.5]);
        assert!(!select([5.0, 2.0, 5.1, 2.0], &mut rect));
    }

    #[test]
    fn only_a_completed_capture_delivers_the_selected_png() {
        let bytes = include_bytes!("../../tests/fixtures/vision-english.png");
        let mut image: Option<ImageInput> = None;
        receive_png(
            (&mut image as *mut Option<ImageInput>).cast(),
            bytes.as_ptr(),
            bytes.len(),
        );
        let result = capture_result(1, image).unwrap().unwrap();
        assert_eq!(
            result.data_url,
            format!("data:image/png;base64,{}", STANDARD.encode(bytes))
        );
        assert!(capture_result(0, None).unwrap().is_none());
        for code in [1, 2, 3, 4] {
            assert!(capture_result(code, None).is_err());
        }
    }
}
