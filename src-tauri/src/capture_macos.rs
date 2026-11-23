use crate::models::CaptureStatus;
use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::sync::{mpsc, Mutex};
use tauri::{Emitter, Manager};

#[path = "capture_macos_image.rs"]
mod image_capture;

const UNAVAILABLE: &str = "macOS 截屏快捷键服务不可用，请重新打开墨译。";
const CONFLICT: &str = "这个快捷键已被 macOS 或其他应用占用，请换一组组合键。";
const INVALID: &str = "请使用 Cmd、Ctrl 或 Option，可配合 Shift，再加一个字母、数字或 F1–F20。";

fn parse_shortcut(value: &str) -> Result<(String, Option<HotKey>), String> {
    if value.trim().is_empty() {
        return Ok((String::new(), None));
    }
    if value.len() > 64 {
        return Err(INVALID.into());
    }
    let mut modifiers = [false; 4];
    let mut key = None;
    for token in value.split('+') {
        let token = token.trim().to_ascii_uppercase();
        let index = match token.as_str() {
            "CMD" | "COMMAND" | "META" => 0,
            "CTRL" | "CONTROL" => 1,
            "ALT" | "OPTION" => 2,
            "SHIFT" => 3,
            _ => {
                let is_function_key = token
                    .strip_prefix('F')
                    .and_then(|n| n.parse::<u8>().ok())
                    .is_some_and(|n| (1..=20).contains(&n) && token == format!("F{n}"));
                if !(is_function_key
                    || token.len() == 1 && token.as_bytes()[0].is_ascii_alphanumeric())
                    || key.replace(token).is_some()
                {
                    return Err(INVALID.into());
                }
                continue;
            }
        };
        if std::mem::replace(&mut modifiers[index], true) {
            return Err(INVALID.into());
        }
    }
    if !modifiers[..3].contains(&true) {
        return Err(INVALID.into());
    }
    let key = key.ok_or(INVALID)?;
    let mut names = ["Cmd", "Ctrl", "Alt", "Shift"]
        .into_iter()
        .zip(modifiers)
        .filter_map(|(name, enabled)| enabled.then_some(name))
        .collect::<Vec<_>>();
    names.push(&key);
    let normalized = names.join("+");
    let hotkey = normalized.parse().map_err(|_| INVALID.to_string())?;
    Ok((normalized, Some(hotkey)))
}

trait KeyRegistry {
    fn claim(&self, key: HotKey) -> Result<(), String>;
    fn release(&self, key: HotKey) -> Result<(), String>;
}
impl KeyRegistry for GlobalHotKeyManager {
    fn claim(&self, key: HotKey) -> Result<(), String> {
        self.register(key).map_err(|error| match error {
            global_hotkey::Error::AlreadyRegistered(_) => CONFLICT.to_string(),
            _ => UNAVAILABLE.to_string(),
        })
    }
    fn release(&self, key: HotKey) -> Result<(), String> {
        self.unregister(key).map_err(|_| UNAVAILABLE.to_string())
    }
}

struct Runtime {
    manager: GlobalHotKeyManager,
    bindings: Mutex<Bindings>,
}

#[derive(Default)]
struct Bindings {
    // None until the startup preference has been read. An empty string means disabled.
    shortcut: Option<String>,
    active: Option<HotKey>,
    recording: bool,
    error: Option<String>,
}

impl Bindings {
    fn snapshot(&self) -> CaptureStatus {
        CaptureStatus {
            shortcut: self.shortcut.clone().unwrap_or_default(),
            active: self.active.is_some(),
            capturing: image_capture::is_capturing(),
            error: self.error.clone(),
        }
    }

    fn replace(
        &mut self,
        manager: &impl KeyRegistry,
        new_key: Option<HotKey>,
    ) -> Result<(), String> {
        if new_key == self.active {
            return Ok(());
        }
        // Claim the new binding first, so a conflict cannot displace a working binding.
        if let Some(key) = new_key {
            manager.claim(key)?;
        }
        if let Some(old) = self.active.take() {
            if manager.release(old).is_err() {
                if let Some(new) = new_key {
                    let _ = manager.release(new);
                }
                return Err(UNAVAILABLE.into());
            }
        }
        self.active = new_key;
        Ok(())
    }

    fn configure(&mut self, manager: &impl KeyRegistry, value: &str) -> Result<(), String> {
        let previous = self
            .shortcut
            .get_or_insert_with(|| value.trim().to_string())
            .clone();
        let (normalized, next) = parse_shortcut(value)?;
        let was_recording = std::mem::replace(&mut self.recording, false);
        if let Err(error) = self.replace(manager, next) {
            if was_recording {
                if let Ok((_, previous_key)) = parse_shortcut(&previous) {
                    let _ = self.replace(manager, previous_key);
                }
            }
            return Err(error);
        }
        self.shortcut = Some(normalized);
        Ok(())
    }

    fn record(&mut self, manager: &impl KeyRegistry, recording: bool) -> Result<(), String> {
        let key = if recording {
            None
        } else {
            parse_shortcut(self.shortcut.as_deref().unwrap_or(""))?.1
        };
        self.recording = recording;
        self.replace(manager, key)
    }
}

pub fn initialize(app: &tauri::AppHandle) -> Result<(), String> {
    if app.try_state::<Runtime>().is_some() {
        return Ok(());
    }
    // Tauri setup runs on the AppKit main thread that will receive Carbon key events.
    let manager = GlobalHotKeyManager::new().map_err(|_| UNAVAILABLE)?;
    app.manage(Runtime {
        manager,
        bindings: Mutex::new(Bindings::default()),
    });
    let app = app.clone();
    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        if event.state != HotKeyState::Pressed || image_capture::is_capturing() {
            return;
        }
        let runtime = app.state::<Runtime>();
        let active = runtime.bindings.lock().ok().and_then(|state| state.active);
        if active.is_some_and(|key| key.id() == event.id) {
            if let Err(error) = start_capture(&app) {
                let _ = app.emit_to("main", "capture-error", error);
            }
        }
    }));
    Ok(())
}

fn update(
    app: &tauri::AppHandle,
    change: impl FnOnce(&mut Bindings, &GlobalHotKeyManager) -> Option<Result<(), String>>
        + Send
        + 'static,
) -> Result<CaptureStatus, String> {
    let (sender, receiver) = mpsc::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let result = (|| {
            let runtime = handle.try_state::<Runtime>().ok_or(UNAVAILABLE)?;
            let mut bindings = runtime.bindings.lock().map_err(|_| UNAVAILABLE)?;
            let Some(result) = change(&mut bindings, &runtime.manager) else {
                return Ok(bindings.snapshot());
            };
            bindings.error = result.as_ref().err().cloned();
            let status = bindings.snapshot();
            drop(bindings);
            let _ = handle.emit_to("main", "capture-status", &status);
            result.map(|_| status)
        })();
        let _ = sender.send(result);
    })
    .map_err(|_| UNAVAILABLE)?;
    receiver.recv().map_err(|_| UNAVAILABLE)?
}

pub fn configure(app: &tauri::AppHandle, shortcut: &str) -> Result<CaptureStatus, String> {
    let shortcut = shortcut.to_owned();
    update(app, move |bindings, manager| {
        Some(bindings.configure(manager, &shortcut))
    })
}

pub fn set_recording(app: &tauri::AppHandle, recording: bool) -> Result<(), String> {
    update(app, move |bindings, manager| {
        (bindings.recording != recording).then(|| bindings.record(manager, recording))
    })
    .map(|_| ())
}

pub fn status(app: &tauri::AppHandle) -> Result<CaptureStatus, String> {
    let runtime = app.try_state::<Runtime>().ok_or(UNAVAILABLE)?;
    let bindings = runtime.bindings.lock().map_err(|_| UNAVAILABLE)?;
    Ok(bindings.snapshot())
}

pub fn start_capture(app: &tauri::AppHandle) -> Result<(), String> {
    image_capture::start_capture(app)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_macos_modifiers_and_stored_windows_shortcuts() {
        for (input, wanted) in [
            (
                " shift + option + control + command + F20 ",
                "Cmd+Ctrl+Alt+Shift+F20",
            ),
            ("Ctrl+Alt+T", "Ctrl+Alt+T"),
            ("META+shift+2", "Cmd+Shift+2"),
            ("option+F12", "Alt+F12"),
        ] {
            let (label, key) = parse_shortcut(input).unwrap();
            assert_eq!(label, wanted);
            assert_eq!(key, Some(wanted.parse().unwrap()));
        }
        assert_eq!(parse_shortcut("  ").unwrap(), (String::new(), None));
        for value in [
            "A",
            "Shift+A",
            "Cmd",
            "Cmd+Command+A",
            "Alt++X",
            "Cmd+A+B",
            "Ctrl+F21",
            "Cmd+F01",
            "Ctrl+VolumeUp",
            "Cmd+é",
        ] {
            assert!(parse_shortcut(value).is_err(), "{value}");
        }
    }
    #[test]
    fn switches_suspends_disables_and_retains_previous_binding_on_conflict() {
        use std::{cell::RefCell, collections::HashSet};
        #[derive(Default)]
        struct Keys(RefCell<HashSet<HotKey>>);
        impl KeyRegistry for Keys {
            fn claim(&self, key: HotKey) -> Result<(), String> {
                self.0
                    .borrow_mut()
                    .insert(key)
                    .then_some(())
                    .ok_or_else(|| CONFLICT.into())
            }
            fn release(&self, key: HotKey) -> Result<(), String> {
                assert!(self.0.borrow_mut().remove(&key));
                Ok(())
            }
        }
        let registry = Keys::default();
        let mut bindings = Bindings::default();
        let labels = [
            "Cmd+Ctrl+Alt+Shift+F17",
            "Cmd+Ctrl+Alt+Shift+F18",
            "Cmd+Ctrl+Alt+Shift+F19",
        ];
        let [first, blocked, last] = labels.map(|s| s.parse::<HotKey>().unwrap());
        registry.claim(blocked).unwrap();
        bindings.configure(&registry, labels[0]).unwrap();
        assert_eq!(bindings.active, Some(first));
        assert_eq!(
            bindings.configure(&registry, labels[1]).unwrap_err(),
            CONFLICT
        );
        assert!(registry.0.borrow().contains(&first));
        assert_eq!(bindings.shortcut.as_deref(), Some(labels[0]));
        bindings.record(&registry, true).unwrap();
        assert!(!registry.0.borrow().contains(&first));
        bindings.record(&registry, false).unwrap();
        assert!(registry.0.borrow().contains(&first));
        bindings.record(&registry, true).unwrap();
        assert_eq!(
            bindings.configure(&registry, labels[1]).unwrap_err(),
            CONFLICT
        );
        assert!(registry.0.borrow().contains(&first));
        bindings.configure(&registry, labels[2]).unwrap();
        assert_eq!(bindings.active, Some(last));
        assert!(!registry.0.borrow().contains(&first));
        bindings.configure(&registry, "").unwrap();
        assert!(!registry.0.borrow().contains(&last));
        assert_eq!(bindings.active, None);
        assert!(bindings.configure(&registry, labels[1]).is_err());
        assert_eq!(bindings.shortcut.as_deref(), Some(""));
        let mut startup = Bindings::default();
        assert!(startup.configure(&registry, labels[1]).is_err());
        assert_eq!(startup.shortcut.as_deref(), Some(labels[1]));
        assert_eq!(startup.active, None);
    }
}
