//! The user's nudge increments are independent of the artwork and its history.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NudgeAmounts {
    pub small: f64,
    pub big: f64,
}

impl Default for NudgeAmounts {
    fn default() -> Self {
        Self {
            small: 1.0,
            big: 10.0,
        }
    }
}

impl NudgeAmounts {
    pub fn step(self, big: bool) -> f64 {
        if big {
            self.big
        } else {
            self.small
        }
    }
    pub fn valid(value: f64) -> bool {
        value > 0.0 && value <= 1e7
    }

    pub fn load() -> Self {
        let data = host::load();
        serde_json::from_str::<[f64; 2]>(&data)
            .ok()
            .filter(|pair| pair.iter().all(|&v| Self::valid(v)))
            .map(|[small, big]| Self { small, big })
            .unwrap_or_default()
    }
    pub fn save(self) -> bool {
        Self::valid(self.small)
            && Self::valid(self.big)
            && host::save(
                &serde_json::to_string(&[self.small, self.big]).expect("finite nudge values"),
            )
    }
}

#[cfg(target_os = "emscripten")]
mod host {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen(inline_js = r#"
const nudgeKey = "ferrodesign.nudgeAmounts.v1";
export function loadNudges() { try { return localStorage.getItem(nudgeKey) || ""; } catch { return ""; } }
export function saveNudges(text) { try { localStorage.setItem(nudgeKey, text); return true; } catch { return false; } }
"#)]
    extern "C" {
        #[wasm_bindgen(js_name=loadNudges)]
        pub fn load() -> String;
        #[wasm_bindgen(js_name=saveNudges)]
        pub fn save(text: &str) -> bool;
    }
}

#[cfg(all(not(test), not(target_os = "emscripten")))]
mod host {
    use std::{env, fs, path::PathBuf};

    fn path() -> Option<PathBuf> {
        let directory = if cfg!(windows) {
            env::var_os("APPDATA").map(PathBuf::from)
        } else if cfg!(target_os = "macos") {
            env::var_os("HOME").map(|p| PathBuf::from(p).join("Library/Application Support"))
        } else {
            env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .or_else(|| env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
        }?;
        Some(directory.join("FerroDesign").join("nudge-amounts.json"))
    }
    pub fn load() -> String {
        path()
            .and_then(|p| fs::read_to_string(p).ok())
            .unwrap_or_default()
    }
    pub fn save(text: &str) -> bool {
        let Some(path) = path() else { return false };
        fs::create_dir_all(path.parent().unwrap()).is_ok()
            && super::super::write_document_atomically(&path, text).is_ok()
    }
}

#[cfg(all(test, not(target_os = "emscripten")))]
mod host {
    ferroui_base::ferro_thread_local! {
        static VALUES: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    }
    pub fn load() -> String {
        VALUES.with(|v| v.borrow().clone())
    }
    pub fn save(text: &str) -> bool {
        VALUES.with(|v| text.clone_into(&mut v.borrow_mut()));
        true
    }
}
