//! Talking to the web app (labels-server) from the Windows app: its products,
//! its spreadsheet (download / upload), adding and checking bought-in products.
//! When a web app server is set in Settings, the Windows app uses the web app's
//! spreadsheet so the two always match.

use crate::config::Config;
use crate::helper::parse_server_url;
use crate::http::{send, Req, Resp};
use crate::lookup::Checked;
use crate::xlsx::Product;
use serde::Deserialize;

const AGENT: &str = "AllergenLabels/1.4";

pub struct Server {
    secure: bool,
    host: String,
    port: u16,
    base: String,
    key: String,
    insecure: bool,
    pub label: String,
}

pub enum Upload {
    Saved(String),
    /// The web app's copy changed since it was downloaded; holds its current version.
    Conflict,
    Rejected(String),
}

#[derive(Deserialize)]
struct ProductsReply {
    products: Vec<Product>,
    cfg: Config,
    #[serde(default)]
    version: String,
}

fn error_text(r: &Resp) -> String {
    serde_json::from_slice::<serde_json::Value>(&r.body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
        .unwrap_or_else(|| format!("the web app replied with error {}", r.status))
}

impl Server {
    /// None when no web app server is set (the Windows app then uses its own spreadsheet).
    pub fn from_cfg(cfg: &Config) -> Option<Server> {
        let url = cfg.server_url.trim();
        if url.is_empty() {
            return None;
        }
        let (secure, host, port, base) = parse_server_url(url)?;
        let label = if (secure && port == 443) || (!secure && port == 80) { host.clone() } else { format!("{host}:{port}") };
        Some(Server { secure, host, port, base, key: cfg.server_key.trim().to_string(), insecure: cfg.server_allow_self_signed, label })
    }

    fn call(&self, method: &str, path: &str, body: Option<&[u8]>, extra: &str, timeout_ms: i32) -> Result<Resp, String> {
        let mut headers = String::from("Accept: application/json\r\n");
        if !self.key.is_empty() {
            headers.push_str(&format!("X-Print-Helper-Key: {}\r\n", self.key));
        }
        headers.push_str(extra);
        let full = format!("{}{}", self.base, path);
        let req = Req {
            secure: self.secure,
            host: &self.host,
            port: self.port,
            method,
            path: &full,
            body,
            headers: &headers,
            insecure: self.insecure,
            receive_timeout_ms: timeout_ms,
        };
        send(&req, AGENT).map_err(|e| format!("couldn't reach the web app at {} ({e})", self.label))
    }

    fn post_json(&self, path: &str, v: &serde_json::Value) -> Result<Resp, String> {
        let body = v.to_string();
        self.call("POST", path, Some(body.as_bytes()), "Content-Type: application/json\r\n", 30_000)
    }

    /// The web app's products (sorted A to Z), its label settings, and the spreadsheet's version.
    pub fn products(&self) -> Result<(Vec<Product>, Config, String), String> {
        let r = self.call("GET", "/api/products", None, "", 20_000)?;
        if r.status != 200 {
            return Err(error_text(&r));
        }
        let mut reply: ProductsReply = serde_json::from_slice(&r.body).map_err(|e| format!("the web app sent something unexpected ({e})"))?;
        reply.products.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok((reply.products, reply.cfg, reply.version))
    }

    pub fn download(&self) -> Result<(Vec<u8>, String), String> {
        let r = self.call("GET", "/api/spreadsheet", None, "", 60_000)?;
        if r.status != 200 {
            return Err(error_text(&r));
        }
        let v = r.version.clone().unwrap_or_else(|| crate::xlsx::version_of(&r.body));
        Ok((r.body, v))
    }

    pub fn upload(&self, bytes: &[u8], base_version: &str, force: bool) -> Result<Upload, String> {
        let extra = format!(
            "Content-Type: application/vnd.openxmlformats-officedocument.spreadsheetml.sheet\r\nX-Base-Version: {base_version}\r\n{}",
            if force { "X-Force: 1\r\n" } else { "" }
        );
        let r = self.call("POST", "/api/spreadsheet", Some(bytes), &extra, 120_000)?;
        match r.status {
            200 => {
                let v = serde_json::from_slice::<serde_json::Value>(&r.body)
                    .ok()
                    .and_then(|v| v.get("version").and_then(|x| x.as_str()).map(String::from))
                    .unwrap_or_default();
                Ok(Upload::Saved(v))
            }
            409 => Ok(Upload::Conflict),
            _ => Ok(Upload::Rejected(error_text(&r))),
        }
    }

    /// Adds a bought-in product to the web app's spreadsheet.
    pub fn add(&self, barcode: &str, name: &str, shop: &str, days: u32) -> Result<(), String> {
        let r = self.post_json("/api/add", &serde_json::json!({ "barcode": barcode, "name": name, "shop": shop, "shelf_days": days }))?;
        if r.status == 200 {
            Ok(())
        } else {
            Err(error_text(&r))
        }
    }

    pub fn checked(&self, barcode: &str) -> Result<Option<Checked>, String> {
        let r = self.call("GET", &format!("/api/checked/{barcode}"), None, "", 20_000)?;
        if r.status != 200 {
            return Err(error_text(&r));
        }
        let v: serde_json::Value = serde_json::from_slice(&r.body).map_err(|e| e.to_string())?;
        Ok(v.get("checked").filter(|c| !c.is_null()).and_then(|c| serde_json::from_value(c.clone()).ok()))
    }

    pub fn save_checked(&self, barcode: &str, ingredients: &str, may_contain: &str, found_name: &str) -> Result<(), String> {
        let r = self.post_json(
            "/api/checked",
            &serde_json::json!({ "barcode": barcode, "ingredients": ingredients, "may_contain": may_contain, "found_name": found_name }),
        )?;
        if r.status == 200 {
            Ok(())
        } else {
            Err(error_text(&r))
        }
    }
}
