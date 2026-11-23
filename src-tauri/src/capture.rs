//! Desktop screenshot selection and global shortcuts. Pixels stay in process memory;
//! only the selected PNG is sent to the existing translation UI.
#[cfg(not(any(windows, target_os = "macos")))]
use crate::models::CaptureStatus;

#[cfg(any(windows, test))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct Shortcut {
    normalized: String,
    modifiers: u32,
    key: u32,
}

#[cfg(any(windows, test))]
fn parse_shortcut(value: &str) -> Result<Option<Shortcut>, String> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    let invalid = || {
        "请使用 Ctrl 或 Alt，可配合 Shift，再加一个字母、数字或 F1–F24（不含 F12）。".to_string()
    };
    if value.len() > 64 {
        return Err(invalid());
    }
    let mut modifiers = 0;
    let mut key = None;
    let mut key_name = String::new();
    for part in value.split('+').map(str::trim) {
        let upper = part.to_ascii_uppercase();
        let modifier = match upper.as_str() {
            "CTRL" | "CONTROL" => 2,
            "ALT" => 1,
            "SHIFT" => 4,
            _ => 0,
        };
        if modifier != 0 {
            if modifiers & modifier != 0 {
                return Err(invalid());
            }
            modifiers |= modifier;
        } else {
            if key.is_some() {
                return Err(invalid());
            }
            let parsed = if upper.len() == 1 && upper.as_bytes()[0].is_ascii_alphanumeric() {
                upper.as_bytes()[0] as u32
            } else if let Some(number) = upper.strip_prefix('F').and_then(|n| n.parse::<u32>().ok())
            {
                if !(1..=24).contains(&number) || number == 12 || upper != format!("F{number}") {
                    return Err(invalid());
                }
                0x70 + number - 1
            } else {
                return Err(invalid());
            };
            key = Some(parsed);
            key_name = upper;
        }
    }
    let key = key.ok_or_else(invalid)?;
    if modifiers & 3 == 0 || (modifiers & 1 != 0 && key_name == "F4") {
        return Err(invalid());
    }
    let mut names = Vec::new();
    if modifiers & 2 != 0 {
        names.push("Ctrl");
    }
    if modifiers & 1 != 0 {
        names.push("Alt");
    }
    if modifiers & 4 != 0 {
        names.push("Shift");
    }
    names.push(&key_name);
    Ok(Some(Shortcut {
        normalized: names.join("+"),
        modifiers,
        key,
    }))
}

#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Selection {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
}

#[cfg(any(windows, test))]
fn selection(start: (i32, i32), end: (i32, i32), width: i32, height: i32) -> Option<Selection> {
    if width <= 0 || height <= 0 {
        return None;
    }
    let x1 = start.0.clamp(0, width);
    let y1 = start.1.clamp(0, height);
    let x2 = end.0.clamp(0, width);
    let y2 = end.1.clamp(0, height);
    let result = Selection {
        left: x1.min(x2),
        top: y1.min(y2),
        width: (x2 - x1).abs(),
        height: (y2 - y1).abs(),
    };
    (result.width >= 2 && result.height >= 2).then_some(result)
}

#[cfg(windows)]
#[path = "capture_windows.rs"]
mod platform;

#[cfg(target_os = "macos")]
#[path = "capture_macos.rs"]
mod platform;

#[cfg(any(windows, target_os = "macos"))]
pub use platform::{configure, initialize, set_recording, start_capture, status};

#[cfg(not(any(windows, target_os = "macos")))]
pub fn initialize(_: &tauri::AppHandle) -> Result<(), String> {
    Ok(())
}
#[cfg(not(any(windows, target_os = "macos")))]
pub fn configure(_: &tauri::AppHandle, _: &str) -> Result<CaptureStatus, String> {
    Err("截屏快捷键目前仅支持 Windows 和 macOS。".into())
}
#[cfg(not(any(windows, target_os = "macos")))]
pub fn set_recording(_: &tauri::AppHandle, _: bool) -> Result<(), String> {
    Err("截屏快捷键目前仅支持 Windows 和 macOS。".into())
}
#[cfg(not(any(windows, target_os = "macos")))]
pub fn start_capture(_: &tauri::AppHandle) -> Result<(), String> {
    Err("截屏目前仅支持 Windows 和 macOS。".into())
}
#[cfg(not(any(windows, target_os = "macos")))]
pub fn status(_: &tauri::AppHandle) -> Result<CaptureStatus, String> {
    Ok(CaptureStatus {
        shortcut: String::new(),
        active: false,
        capturing: false,
        error: Some("截屏快捷键目前仅支持 Windows 和 macOS。".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcut_normalization_disable_and_reserved_keys() {
        assert_eq!(
            parse_shortcut(" alt + control + shift + t ")
                .unwrap()
                .unwrap()
                .normalized,
            "Ctrl+Alt+Shift+T"
        );
        assert_eq!(parse_shortcut("Ctrl+F24").unwrap().unwrap().key, 0x87);
        assert!(parse_shortcut("").unwrap().is_none());
        for text in [
            "T",
            "Shift+T",
            "Ctrl+F12",
            "Alt+F4",
            "Win+T",
            "Ctrl+Alt+Delete",
            "Alt+Tab",
            "Ctrl++T",
            "Ctrl+Ctrl+T",
            "Ctrl+Q+T",
            "Ctrl+F01",
            "Ctrl+F25",
            "Ctrl+Escape",
            "Ctrl+é",
        ] {
            assert!(parse_shortcut(text).is_err(), "{text}");
        }
    }
    #[test]
    fn reverse_drag_and_offscreen_coordinates_are_clamped_without_dpi_scaling() {
        assert_eq!(
            selection((1600, 700), (100, 80), 3840, 2160),
            Some(Selection {
                left: 100,
                top: 80,
                width: 1500,
                height: 620
            })
        );
        assert_eq!(
            selection((-900, -50), (200, 400), 1920, 1080),
            Some(Selection {
                left: 0,
                top: 0,
                width: 200,
                height: 400
            })
        );
        assert_eq!(
            selection((1900, 1000), (5000, 3000), 1920, 1080),
            Some(Selection {
                left: 1900,
                top: 1000,
                width: 20,
                height: 80
            })
        );
        assert!(selection((4, 4), (4, 8), 1920, 1080).is_none());
        // A monitor at x=-1920 maps physical desktop pixels into the same zero-based snapshot.
        assert_eq!((-1700i32 - -1920, 100i32 - -200), (220, 300));
    }
}
