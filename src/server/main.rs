//! Allergen Label Printer – web server.
//!
//! Serves the label app as a web page (for a Home Assistant dashboard or any
//! browser), reads the same Products.xlsx, and queues print jobs that the
//! Windows app collects in "print helper" mode and prints on the USB printer.

mod tls;

use allergen_labels::config::Config;
use allergen_labels::content::{self, Para};
use allergen_labels::lookup::{self, Checked, Found};
use allergen_labels::xlsx::{self, Product};
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Datelike, Local};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::Notify;

const INDEX_HTML: &str = include_str!("web/index.html");
const SAMPLE_XLSX: &[u8] = include_bytes!("../../examples/Products.xlsx");
/// Queued jobs nobody collects within this time are dropped, so a label
/// doesn't suddenly print hours later when the PC is switched back on.
const JOB_EXPIRY: Duration = Duration::from_secs(10 * 60);
/// The print helper polls every ~25 s; after this long it's shown as offline.
const HELPER_TIMEOUT: Duration = Duration::from_secs(70);

use allergen_labels::helper::JobMsg;

struct Job {
    msg: JobMsg,
    created: Instant,
}

#[derive(Clone, Serialize)]
struct JobStatus {
    state: &'static str, // queued | printing | done | failed
    message: String,
}

struct Inner {
    cfg: Config,
    products: Vec<Product>,
    unchecked: usize,
    last_mtime: Option<SystemTime>,
    last_load: Option<Instant>,
    loaded_at: String,
    error: Option<String>,
    jobs: VecDeque<Job>,
    results: HashMap<u64, JobStatus>,
    next_id: u64,
    helper: Option<(Instant, String)>,
    found_cache: HashMap<String, Found>,
}

struct AppState {
    inner: Mutex<Inner>,
    notify: Notify,
    data: PathBuf,
    helper_key: Option<String>,
}

type App = Arc<AppState>;

impl AppState {
    fn spreadsheet(&self) -> PathBuf {
        let name = std::env::var("SPREADSHEET").unwrap_or_else(|_| "Products.xlsx".into());
        self.data.join(name)
    }
    fn settings_path(&self) -> PathBuf {
        self.data.join("settings.json")
    }
    fn checked_path(&self) -> PathBuf {
        self.data.join("checked_products.json")
    }

    /// Re-reads the spreadsheet (and the checked bought-in products).
    fn reload(&self) {
        let path = self.spreadsheet();
        let marking = self.inner.lock().unwrap().cfg.marking();
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        let result = xlsx::read_products(&path, marking);
        let store = lookup::Store::load_from(&self.checked_path());
        let mut g = self.inner.lock().unwrap();
        g.last_load = Some(Instant::now());
        match result {
            Ok(mut ps) => {
                g.unchecked = lookup::apply_checked(&mut ps, &store);
                g.products = ps;
                g.last_mtime = mtime;
                g.error = None;
                g.loaded_at = Local::now().format("%H:%M").to_string();
            }
            Err(e) => {
                // Keep the last good list; a file mid-save is retried next minute.
                g.error = Some(e);
            }
        }
    }

    fn poll(&self) {
        let (last, loaded, mins) = {
            let g = self.inner.lock().unwrap();
            (g.last_mtime, g.last_load, g.cfg.refresh_minutes.max(1))
        };
        let mtime = std::fs::metadata(self.spreadsheet()).and_then(|m| m.modified()).ok();
        let stale = loaded.map_or(true, |l| l.elapsed() >= Duration::from_secs(mins as u64 * 60));
        if mtime != last || stale {
            self.reload();
        }
    }
}

fn today() -> (i32, u32, u32) {
    let n = Local::now();
    (n.year(), n.month(), n.day())
}

fn err(code: StatusCode, msg: impl Into<String>) -> Response {
    (code, Json(json!({ "error": msg.into() }))).into_response()
}

// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let data = PathBuf::from(std::env::var("DATA_DIR").unwrap_or_else(|_| "data".into()));
    std::fs::create_dir_all(&data).expect("can't create the data folder");

    let state = Arc::new(AppState {
        inner: Mutex::new(Inner {
            cfg: Config::default(),
            products: Vec::new(),
            unchecked: 0,
            last_mtime: None,
            last_load: None,
            loaded_at: String::new(),
            error: None,
            jobs: VecDeque::new(),
            results: HashMap::new(),
            next_id: 1,
            helper: None,
            found_cache: HashMap::new(),
        }),
        notify: Notify::new(),
        helper_key: std::env::var("PRINT_HELPER_KEY").ok().filter(|k| !k.trim().is_empty()),
        data: data.clone(),
    });

    // Settings: saved ones, or the defaults.
    let settings_path = state.settings_path();
    let cfg = if settings_path.exists() { Config::load_from(&settings_path) } else { Config::default() };
    state.inner.lock().unwrap().cfg = cfg;

    // First run: start with the sample spreadsheet so there's something to see.
    let sheet = state.spreadsheet();
    if !sheet.exists() {
        std::fs::write(&sheet, SAMPLE_XLSX).expect("can't write the sample spreadsheet");
        println!("No spreadsheet found – created a sample one at {}", sheet.display());
    }
    state.reload();

    let bg = state.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            let s = bg.clone();
            let _ = tokio::task::spawn_blocking(move || s.poll()).await;
            // Expire jobs nobody collected.
            let mut g = bg.inner.lock().unwrap();
            while g.jobs.front().map_or(false, |j| j.created.elapsed() > JOB_EXPIRY) {
                let j = g.jobs.pop_front().unwrap();
                g.results.insert(
                    j.msg.id,
                    JobStatus { state: "failed", message: "Nothing collected it – is the print helper running on the PC?".into() },
                );
            }
            if g.results.len() > 500 {
                g.results.clear();
            }
        }
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/api/health", get(|| async { "ok" }))
        .route("/favicon.svg", get(favicon))
        .route("/api/state", get(api_state))
        .route("/api/refresh", post(api_refresh))
        .route("/api/print", post(api_print))
        .route("/api/jobs/{id}", get(api_job))
        .route("/api/settings", get(api_get_settings).post(api_set_settings))
        .route("/api/lookup", post(api_lookup))
        .route("/api/checked/{barcode}", get(api_get_checked))
        .route("/api/checked", post(api_set_checked))
        .route("/api/add", post(api_add))
        .route("/api/helper/next", get(api_helper_next))
        .route("/api/helper/result", post(api_helper_result))
        .with_state(state.clone());

    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let public_port = std::env::var("LABELS_PORT").unwrap_or_else(|_| port.to_string());
    println!("Spreadsheet: {}", state.spreadsheet().display());
    if state.helper_key.is_some() {
        println!("Print helper key is set – the Windows app needs the same key in its Settings.");
    }
    match tls::setup().await {
        Ok(Some((rustls_cfg, about))) => {
            println!("{about}");
            println!("Allergen Label Printer is running at https://<this computer>:{public_port}");
            axum_server::bind_rustls(addr, rustls_cfg).serve(app.into_make_service()).await.unwrap();
        }
        Ok(None) => {
            println!("HTTPS is off. Allergen Label Printer is running at http://<this computer>:{public_port}");
            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            axum::serve(listener, app).await.unwrap();
        }
        Err(e) => {
            eprintln!("Couldn't start HTTPS: {e}");
            std::process::exit(1);
        }
    }
}

async fn favicon() -> Response {
    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect x="7" y="10" width="50" height="44" rx="7" fill="#fff" stroke="#3c3c3c" stroke-width="3"/><path d="M7 17a7 7 0 0 1 7-7h36a7 7 0 0 1 7 7v6H7z" fill="#d65c32"/><rect x="14" y="29" width="36" height="5" rx="2.5" fill="#1e1e1e"/><rect x="14" y="38" width="26" height="3" rx="1.5" fill="#1e1e1e"/><rect x="14" y="45" width="32" height="3" rx="1.5" fill="#1e1e1e"/></svg>"##;
    ([(header::CONTENT_TYPE, "image/svg+xml"), (header::CACHE_CONTROL, "max-age=86400")], SVG).into_response()
}

async fn index() -> Response {
    // Allow embedding in Home Assistant (no X-Frame-Options), never cache the page.
    ([(header::CACHE_CONTROL, "no-store")], Html(INDEX_HTML)).into_response()
}

// ---------------------------------------------------------------------------
// State and printing

#[derive(Serialize)]
struct ProductView {
    name: String,
    needs_check: bool,
    barcode: Option<String>,
    store: Option<String>,
    label: Vec<Para>,
}

async fn api_state(State(app): State<App>) -> Response {
    let g = app.inner.lock().unwrap();
    let t = today();
    let products: Vec<ProductView> = g
        .products
        .iter()
        .map(|p| ProductView {
            name: p.name.clone(),
            needs_check: p.needs_check,
            barcode: p.barcode.clone(),
            store: p.store.clone(),
            label: if p.needs_check { Vec::new() } else { content::paragraphs(p, &g.cfg, t) },
        })
        .collect();
    let helper = g.helper.as_ref().map(|(seen, name)| {
        json!({ "connected": seen.elapsed() < HELPER_TIMEOUT, "name": name, "seconds_ago": seen.elapsed().as_secs() })
    });
    let file = app.spreadsheet().file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    Json(json!({
        "products": products,
        "file": file,
        "loaded_at": g.loaded_at,
        "error": g.error,
        "unchecked": g.unchecked,
        "helper": helper,
        "queued": g.jobs.len(),
        "label_width_mm": g.cfg.label_width_mm,
        "margin_mm": g.cfg.margin_mm,
        "title_pt": g.cfg.title_pt,
        "body_pt": g.cfg.body_pt,
        "font_face": g.cfg.font_face,
        "default_days": g.cfg.best_before_days.clamp(1, 10),
    }))
    .into_response()
}

async fn api_refresh(State(app): State<App>) -> Response {
    let a = app.clone();
    let _ = tokio::task::spawn_blocking(move || a.reload()).await;
    let g = app.inner.lock().unwrap();
    match &g.error {
        Some(e) => err(StatusCode::UNPROCESSABLE_ENTITY, format!("Couldn't read the spreadsheet: {e}")),
        None => Json(json!({ "ok": true, "count": g.products.len() })).into_response(),
    }
}

#[derive(Deserialize)]
struct PrintReq {
    name: String,
    #[serde(default = "one")]
    copies: u32,
}
fn one() -> u32 {
    1
}

async fn api_print(State(app): State<App>, Json(req): Json<PrintReq>) -> Response {
    let mut g = app.inner.lock().unwrap();
    let Some(p) = g.products.iter().find(|p| p.name == req.name).cloned() else {
        return err(StatusCode::NOT_FOUND, "That product isn't in the spreadsheet any more – refresh and try again.");
    };
    if p.needs_check {
        return err(StatusCode::CONFLICT, "Check this product's ingredients against the pack before printing.");
    }
    let id = g.next_id;
    g.next_id += 1;
    let msg = JobMsg { id, product: p, cfg: g.cfg.clone(), copies: req.copies.clamp(1, 20) };
    g.jobs.push_back(Job { msg, created: Instant::now() });
    g.results.insert(id, JobStatus { state: "queued", message: "Waiting for the print helper…".into() });
    let connected = g.helper.as_ref().map_or(false, |(seen, _)| seen.elapsed() < HELPER_TIMEOUT);
    drop(g);
    app.notify.notify_one();
    Json(json!({ "id": id, "helper_connected": connected })).into_response()
}

async fn api_job(State(app): State<App>, UrlPath(id): UrlPath<u64>) -> Response {
    let g = app.inner.lock().unwrap();
    match g.results.get(&id) {
        Some(s) => Json(s.clone()).into_response(),
        None => err(StatusCode::NOT_FOUND, "Unknown print job"),
    }
}

// ---------------------------------------------------------------------------
// Print helper (the Windows app)

fn helper_allowed(app: &AppState, headers: &HeaderMap) -> bool {
    let Some(key) = &app.helper_key else { return true };
    let given = headers
        .get("x-print-helper-key")
        .and_then(|v| v.to_str().ok())
        .or_else(|| headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")));
    given.map(str::trim) == Some(key.trim())
}

#[derive(Deserialize)]
struct NextQuery {
    #[serde(default)]
    name: String,
}

async fn api_helper_next(State(app): State<App>, headers: HeaderMap, Query(q): Query<NextQuery>) -> Response {
    if !helper_allowed(&app, &headers) {
        return err(StatusCode::UNAUTHORIZED, "Wrong print helper key");
    }
    let name = if q.name.trim().is_empty() { "Print helper".to_string() } else { q.name.trim().to_string() };
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        {
            let mut g = app.inner.lock().unwrap();
            g.helper = Some((Instant::now(), name.clone()));
            while let Some(job) = g.jobs.pop_front() {
                if job.created.elapsed() > JOB_EXPIRY {
                    g.results.insert(job.msg.id, JobStatus { state: "failed", message: "Too old – not printed.".into() });
                    continue;
                }
                g.results.insert(job.msg.id, JobStatus { state: "printing", message: format!("Printing on {name}…") });
                return Json(job.msg).into_response();
            }
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return StatusCode::NO_CONTENT.into_response();
        }
        let _ = tokio::time::timeout(left, app.notify.notified()).await;
    }
}

#[derive(Deserialize)]
struct ResultReq {
    id: u64,
    ok: bool,
    #[serde(default)]
    message: String,
}

async fn api_helper_result(State(app): State<App>, headers: HeaderMap, Json(r): Json<ResultReq>) -> Response {
    if !helper_allowed(&app, &headers) {
        return err(StatusCode::UNAUTHORIZED, "Wrong print helper key");
    }
    let mut g = app.inner.lock().unwrap();
    g.results.insert(r.id, JobStatus { state: if r.ok { "done" } else { "failed" }, message: r.message });
    Json(json!({ "ok": true })).into_response()
}

// ---------------------------------------------------------------------------
// Settings

async fn api_get_settings(State(app): State<App>) -> Response {
    let g = app.inner.lock().unwrap();
    Json(g.cfg.clone()).into_response()
}

async fn api_set_settings(State(app): State<App>, Json(new): Json<Config>) -> Response {
    let check = |v: f32, lo: f32, hi: f32, what: &str| -> Result<(), String> {
        if v >= lo && v <= hi {
            Ok(())
        } else {
            Err(format!("{what} should be between {lo} and {hi}."))
        }
    };
    let valid = check(new.label_width_mm, 20.0, 120.0, "Label width")
        .and(check(new.margin_mm, 0.0, 20.0, "Side margin"))
        .and(check(new.title_pt, 6.0, 48.0, "Product name size"))
        .and(check(new.body_pt, 5.0, 30.0, "Ingredient text size"))
        .and(check(new.refresh_minutes as f32, 1.0, 1440.0, "Refresh interval"))
        .and(check(new.best_before_days as f32, 1.0, 10.0, "Default best before"));
    if let Err(e) = valid {
        return err(StatusCode::UNPROCESSABLE_ENTITY, e);
    }
    let marking_changed;
    {
        let mut g = app.inner.lock().unwrap();
        let old = g.cfg.clone();
        marking_changed = old.allergens_in_brackets != new.allergens_in_brackets;
        // Fields that only mean something on the Windows PC are kept as they were.
        g.cfg = Config {
            spreadsheet: old.spreadsheet,
            printer: old.printer,
            server_url: old.server_url,
            server_key: old.server_key,
            server_allow_self_signed: old.server_allow_self_signed,
            font_face: if new.font_face.trim().is_empty() { "Arial".into() } else { new.font_face.trim().into() },
            ..new
        };
        if let Err(e) = g.cfg.save_to(&app.settings_path()) {
            return err(StatusCode::INTERNAL_SERVER_ERROR, e);
        }
    }
    if marking_changed {
        let a = app.clone();
        let _ = tokio::task::spawn_blocking(move || a.reload()).await;
    }
    Json(json!({ "ok": true })).into_response()
}

// ---------------------------------------------------------------------------
// Bought-in products

#[derive(Deserialize)]
struct LookupReq {
    barcode: String,
}

async fn api_lookup(State(app): State<App>, Json(req): Json<LookupReq>) -> Response {
    let Some(bc) = xlsx::parse_barcode(&req.barcode).filter(|b| !b.is_empty()) else {
        return err(StatusCode::UNPROCESSABLE_ENTITY, "That doesn't look like a barcode – it should be the 8 to 14 digit number under the bars.");
    };
    let bc2 = bc.clone();
    let res = tokio::task::spawn_blocking(move || off_lookup(&bc2)).await.unwrap_or_else(|_| Err("lookup failed".into()));
    match res {
        Ok(found) => {
            if let Some(f) = &found {
                app.inner.lock().unwrap().found_cache.insert(bc.clone(), f.clone());
            }
            Json(json!({ "barcode": bc, "found": found })).into_response()
        }
        Err(e) => Json(json!({ "barcode": bc, "found": null, "error": e })).into_response(),
    }
}

fn off_lookup(barcode: &str) -> Result<Option<Found>, String> {
    let url = format!("https://{}{}", lookup::OFF_HOST, lookup::off_path(barcode));
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).user_agent(lookup::USER_AGENT).build();
    match agent.get(&url).call() {
        Ok(resp) => {
            let body = resp.into_string().map_err(|e| e.to_string())?;
            lookup::parse_off(&body)
        }
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(ureq::Error::Status(429, _)) => Err("Open Food Facts is busy – try again in a minute".into()),
        Err(ureq::Error::Status(code, _)) => Err(format!("Open Food Facts replied with error {code}")),
        Err(ureq::Error::Transport(t)) => Err(match t.kind() {
            ureq::ErrorKind::Dns => "couldn't find Open Food Facts (is the server connected to the internet?)".into(),
            ureq::ErrorKind::ConnectionFailed => "couldn't connect to Open Food Facts (is the server connected to the internet?)".into(),
            ureq::ErrorKind::Io => "the connection to Open Food Facts timed out or dropped".into(),
            k => format!("couldn't reach Open Food Facts ({k})"),
        }),
    }
}

async fn api_get_checked(State(app): State<App>, UrlPath(barcode): UrlPath<String>) -> Response {
    let store = lookup::Store::load_from(&app.checked_path());
    let g = app.inner.lock().unwrap();
    Json(json!({
        "checked": store.products.get(&barcode),
        "cached": g.found_cache.get(&barcode),
    }))
    .into_response()
}

#[derive(Deserialize)]
struct CheckReq {
    barcode: String,
    ingredients: String,
    #[serde(default)]
    may_contain: String,
    #[serde(default)]
    found_name: String,
}

async fn api_set_checked(State(app): State<App>, Json(req): Json<CheckReq>) -> Response {
    let ing = match lookup::normalise_ingredients(&req.ingredients) {
        Ok(i) => i,
        Err(e) => return err(StatusCode::UNPROCESSABLE_ENTITY, e),
    };
    let store_name = {
        let g = app.inner.lock().unwrap();
        g.products.iter().find(|p| p.barcode.as_deref() == Some(req.barcode.as_str())).and_then(|p| p.store.clone()).unwrap_or_default()
    };
    let path = app.checked_path();
    let mut store = lookup::Store::load_from(&path);
    let t = today();
    store.products.insert(
        req.barcode.clone(),
        Checked {
            store: store_name,
            found_name: req.found_name.trim().to_string(),
            ingredients: ing,
            may_contain: req.may_contain.trim().trim_end_matches('.').to_string(),
            checked_on: allergen_labels::date::add_days_formatted(t.0, t.1, t.2, 0),
        },
    );
    if let Err(e) = store.save_to(&path) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    let a = app.clone();
    let _ = tokio::task::spawn_blocking(move || a.reload()).await;
    Json(json!({ "ok": true })).into_response()
}

#[derive(Deserialize)]
struct AddReq {
    barcode: String,
    name: String,
    #[serde(default)]
    shop: String,
    #[serde(default)]
    shelf_days: u32,
}

async fn api_add(State(app): State<App>, Json(req): Json<AddReq>) -> Response {
    let Some(bc) = xlsx::parse_barcode(&req.barcode).filter(|b| !b.is_empty()) else {
        return err(StatusCode::UNPROCESSABLE_ENTITY, "Enter the barcode – the 8 to 14 digit number under the bars.");
    };
    let name = req.name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return err(StatusCode::UNPROCESSABLE_ENTITY, "Enter the product name to print on the label.");
    }
    let days = req.shelf_days.clamp(1, 10);
    {
        let g = app.inner.lock().unwrap();
        if let Some(p) = g.products.iter().find(|p| p.barcode.as_deref() == Some(bc.as_str()) || p.name.eq_ignore_ascii_case(&name)) {
            return err(StatusCode::CONFLICT, format!("\"{}\" is already in the spreadsheet with that name or barcode.", p.name));
        }
    }
    let mut values = vec![name.clone(), format!("Shelf life: {days} day{}", if days == 1 { "" } else { "s" }), format!("Barcode: {bc}")];
    if !req.shop.trim().is_empty() {
        values.push(req.shop.trim().to_string());
    }
    let path = app.spreadsheet();
    let res = tokio::task::spawn_blocking(move || xlsx::append_product_column(&path, &values)).await.unwrap_or_else(|_| Err("failed".into()));
    if let Err(e) = res {
        return err(StatusCode::UNPROCESSABLE_ENTITY, format!("Couldn't add it to the spreadsheet: {e}"));
    }
    let a = app.clone();
    let _ = tokio::task::spawn_blocking(move || a.reload()).await;
    Json(json!({ "ok": true, "name": name, "barcode": bc })).into_response()
}
