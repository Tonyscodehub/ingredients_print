use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// Full path to the .xlsx file.
    pub spreadsheet: String,
    /// Printer name; empty means the Windows default printer.
    pub printer: String,
    /// Forced re-read interval. The file is also checked every minute for saves.
    pub refresh_minutes: u32,
    /// Printable width of the label, used for the on-screen preview.
    pub label_width_mm: f32,
    /// Extra margin added inside the printer's printable area.
    pub margin_mm: f32,
    pub font_face: String,
    pub title_pt: f32,
    pub body_pt: f32,
    /// false = "Ingredients: a, b, c." (UK label style); true = one per line.
    pub one_per_line: bool,
    /// Printed under the ingredients; leave empty for none.
    pub footer: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            spreadsheet: exe_dir().join("Products.xlsx").to_string_lossy().into_owned(),
            printer: String::new(),
            refresh_minutes: 60,
            label_width_mm: 72.0,
            margin_mm: 1.0,
            font_face: "Arial".into(),
            title_pt: 16.0,
            body_pt: 10.0,
            one_per_line: false,
            footer: "Allergens are shown in bold.".into(),
        }
    }
}

pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn config_path() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(exe_dir);
    base.join("AllergenLabels").join("settings.json")
}

impl Config {
    pub fn load() -> Config {
        std::fs::read_to_string(config_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let p = config_path();
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let s = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&p, s).map_err(|e| format!("couldn't save settings to {}: {e}", p.display()))
    }
}
