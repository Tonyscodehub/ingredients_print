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
    /// Business name printed at the bottom of each label; empty for none.
    pub business_name: String,
    /// Business address, printed under the name; empty for none.
    pub address: String,
    /// Contact details (email/phone) printed under the address; empty for none.
    pub contact: String,
    /// Cross-contamination warning printed at the very bottom; empty for none.
    pub warning: String,
    /// true = text in (brackets) is the allergen; false = bold text in the spreadsheet.
    pub allergens_in_brackets: bool,
    /// Print helper: web server address (empty = off), e.g. https://192.168.1.128:8443
    pub server_url: String,
    /// Print helper: key, if the server has PRINT_HELPER_KEY set.
    pub server_key: String,
    /// Print helper: accept the server's self-signed certificate.
    pub server_allow_self_signed: bool,
    /// Start hidden in the notification area (and minimise to it).
    pub start_in_tray: bool,
    /// Best before = day printed + this many days. 0 = don't print a date.
    pub best_before_days: u32,
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
            business_name: "PoFfles".into(),
            address: "Grantham House, 423 Nottingham Road, Mansfield, NG18 4SU".into(),
            contact: "amy@poffles.co.uk".into(),
            warning: "Made in a kitchen that handles a range of ingredients. No peanuts are used, but traces of other allergens may be present due to cross-contamination.".into(),
            best_before_days: 3,
            allergens_in_brackets: true,
            server_url: "https://192.168.1.128:8443".into(),
            server_key: String::new(),
            server_allow_self_signed: true,
            start_in_tray: false,
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
    pub fn marking(&self) -> crate::xlsx::Marking {
        if self.allergens_in_brackets {
            crate::xlsx::Marking::Brackets
        } else {
            crate::xlsx::Marking::Bold
        }
    }

    pub fn load() -> Config {
        Self::load_from(&config_path())
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_to(&config_path())
    }

    pub fn load_from(p: &std::path::Path) -> Config {
        std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// Like `load`, but if the settings file exists and can't be read (e.g. it was
    /// cut short by a power cut), it's kept as settings.damaged.json and a
    /// message is returned instead of quietly starting from the defaults.
    pub fn load_checked() -> (Config, Option<String>) {
        let p = config_path();
        let Ok(text) = std::fs::read_to_string(&p) else { return (Config::default(), None) };
        match serde_json::from_str(&text) {
            Ok(c) => (c, None),
            Err(_) => {
                let bad = p.with_file_name("settings.damaged.json");
                let _ = std::fs::rename(&p, &bad);
                (
                    Config::default(),
                    Some(format!(
                        "The saved settings couldn't be read, so the defaults are being used. Please check Settings.\n\nThe old file was kept as {}",
                        bad.display()
                    )),
                )
            }
        }
    }

    pub fn save_to(&self, p: &std::path::Path) -> Result<(), String> {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let s = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        // Write to a temporary file and swap it in, so a power cut can't leave a half-written file.
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, s).map_err(|e| format!("couldn't save settings to {}: {e}", p.display()))?;
        std::fs::rename(&tmp, p).map_err(|e| format!("couldn't save settings to {}: {e}", p.display()))
    }
}
