//! Win32 user interface: toolbar, auto-sizing product buttons, label preview, settings.

use crate::config::Config;
use crate::label::{self, wide};
use crate::xlsx::{self, Product, Run};
use std::cell::RefCell;
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Instant, SystemTime};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::UI::Controls::Dialogs::*;
use windows_sys::Win32::UI::Controls::SetScrollInfo;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const APP_TITLE: &str = "Allergen Label Printer";

const ID_REFRESH: usize = 101;
const ID_SETTINGS: usize = 102;
const ID_COPIES: usize = 103;
const ID_PRODUCT: usize = 1000;

const S_PATH: i32 = 2001;
const S_BROWSE: i32 = 2002;
const S_PRINTER: i32 = 2003;
const S_WIDTH: i32 = 2004;
const S_MARGIN: i32 = 2005;
const S_FONT: i32 = 2006;
const S_TITLE: i32 = 2007;
const S_BODY: i32 = 2008;
const S_LAYOUT: i32 = 2009;
const S_REFRESH: i32 = 2010;
const S_FOOTER: i32 = 2011;
const S_TEST: i32 = 2012;
const S_BUSINESS: i32 = 2013;
const S_CONTACT: i32 = 2014;
const S_BESTBEFORE: i32 = 2015;
const S_ADDRESS: i32 = 2016;
const S_WARNING: i32 = 2017;
const S_MARKING: i32 = 2018;

const TIMER_POLL: usize = 1;
const WM_APP_STATUS: u32 = WM_APP + 1;

// Static control styles (kept local; windows-sys files them under SystemServices).
const SS_LEFT: u32 = 0x0;
const SS_RIGHT: u32 = 0x2;
const SS_LEFTNOWORDWRAP: u32 = 0xC;
const SS_CENTERIMAGE: u32 = 0x200;
const SS_ENDELLIPSIS: u32 = 0x4000;

static DPI: AtomicI32 = AtomicI32::new(96);

fn s(px: i32) -> i32 {
    px * DPI.load(Ordering::Relaxed) / 96
}

struct App {
    cfg: Config,
    products: Vec<Product>,
    hinst: HINSTANCE,
    main: HWND,
    host: HWND,
    preview: HWND,
    status: HWND,
    copies: HWND,
    tools: Vec<HWND>,
    buttons: Vec<HWND>,
    rects: Vec<RECT>,
    content_h: i32,
    scroll: i32,
    ui_font: HFONT,
    btn_font: HFONT,
    btn_font_px: i32,
    preview_product: Option<Product>,
    last_mtime: Option<SystemTime>,
    last_load: Instant,
    settings: HWND,
    in_layout: bool,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Short, non-reentrant access to app state. Never call Win32 functions that
/// send messages to our own windows from inside the closure.
fn with<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn loword(v: usize) -> u32 {
    (v & 0xffff) as u32
}
fn hiword(v: usize) -> u32 {
    ((v >> 16) & 0xffff) as u32
}

fn set_text(h: HWND, t: &str) {
    let w = wide(t);
    unsafe { SetWindowTextW(h, w.as_ptr()) };
}

fn get_text(h: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(h);
        let mut buf = vec![0u16; n as usize + 1];
        let got = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..got.max(0) as usize])
    }
}

fn child(class: &str, text: &str, style: u32, ex: u32, parent: HWND, id: usize, font: HFONT) -> HWND {
    let c = wide(class);
    let t = wide(text);
    unsafe {
        let hinst = GetModuleHandleW(null());
        let h = CreateWindowExW(ex, c.as_ptr(), t.as_ptr(), WS_CHILD | WS_VISIBLE | style, 0, 0, 0, 0, parent, id as HMENU, hinst, null());
        if !font.is_null() {
            SendMessageW(h, WM_SETFONT, font as usize, 1);
        }
        h
    }
}

fn now_hhmm() -> String {
    unsafe {
        let mut st: SYSTEMTIME = std::mem::zeroed();
        GetLocalTime(&mut st);
        format!("{:02}:{:02}", st.wHour, st.wMinute)
    }
}

fn message_box(owner: HWND, text: &str, icon: u32) {
    let t = wide(text);
    let c = wide(APP_TITLE);
    unsafe { MessageBoxW(owner, t.as_ptr(), c.as_ptr(), MB_OK | icon) };
}

fn set_status(t: &str) {
    if let Some(h) = with(|a| a.status) {
        set_text(h, t);
    }
}

// ---------------------------------------------------------------------------

pub fn run() {
    unsafe {
        let hinst = GetModuleHandleW(null());
        let sdc = GetDC(null_mut());
        DPI.store(GetDeviceCaps(sdc, LOGPIXELSY as _).max(96), Ordering::Relaxed);
        ReleaseDC(null_mut(), sdc);

        let icon = LoadIconW(hinst, 1usize as *const u16);
        let cursor = LoadCursorW(null_mut(), IDC_ARROW);
        let bg = (COLOR_BTNFACE + 1) as usize as HBRUSH;
        for (name, proc_) in [
            ("AllergenMain", main_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT),
            ("AllergenHost", host_proc),
            ("AllergenPreview", preview_proc),
            ("AllergenSettings", settings_proc),
        ] {
            let n = wide(name);
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(proc_),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinst,
                hIcon: icon,
                hCursor: cursor,
                hbrBackground: bg,
                lpszMenuName: null(),
                lpszClassName: n.as_ptr(),
                hIconSm: icon,
            };
            RegisterClassExW(&wc);
        }

        let mut ncm: NONCLIENTMETRICSW = std::mem::zeroed();
        ncm.cbSize = std::mem::size_of::<NONCLIENTMETRICSW>() as u32;
        SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, ncm.cbSize, &mut ncm as *mut _ as *mut _, 0);
        let mut lf = ncm.lfMessageFont;
        lf.lfHeight = (lf.lfHeight as f32 * 1.15) as i32;
        let ui_font = CreateFontIndirectW(&lf);

        APP.with(|a| {
            *a.borrow_mut() = Some(App {
                cfg: Config::load(),
                products: Vec::new(),
                hinst,
                main: null_mut(),
                host: null_mut(),
                preview: null_mut(),
                status: null_mut(),
                copies: null_mut(),
                tools: Vec::new(),
                buttons: Vec::new(),
                rects: Vec::new(),
                content_h: 0,
                scroll: 0,
                ui_font,
                btn_font: null_mut(),
                btn_font_px: 0,
                preview_product: None,
                last_mtime: None,
                last_load: Instant::now(),
                settings: null_mut(),
                in_layout: false,
            })
        });

        let cls = wide("AllergenMain");
        let title = wide(APP_TITLE);
        let main = CreateWindowExW(
            0,
            cls.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            s(1100),
            s(700),
            null_mut(),
            null_mut(),
            hinst,
            null(),
        );

        let host = child("AllergenHost", "", WS_VSCROLL | WS_CLIPCHILDREN | WS_TABSTOP, WS_EX_CONTROLPARENT, main, 0, null_mut());
        let preview = child("AllergenPreview", "", 0, 0, main, 0, null_mut());
        let btn_style = WS_TABSTOP | BS_PUSHBUTTON as u32;
        let refresh = child("BUTTON", "Refresh products", btn_style, 0, main, ID_REFRESH, ui_font);
        let settings = child("BUTTON", "Settings…", btn_style, 0, main, ID_SETTINGS, ui_font);
        let lbl = child("STATIC", "Copies:", (SS_RIGHT | SS_CENTERIMAGE) as u32, 0, main, 0, ui_font);
        let copies = child("COMBOBOX", "", WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32, 0, main, ID_COPIES, ui_font);
        for i in 1..=10 {
            let t = wide(&i.to_string());
            SendMessageW(copies, CB_ADDSTRING, 0, t.as_ptr() as isize);
        }
        SendMessageW(copies, CB_SETCURSEL, 0, 0);
        let status = child("STATIC", "", (SS_LEFTNOWORDWRAP | SS_CENTERIMAGE | SS_ENDELLIPSIS) as u32, 0, main, 0, ui_font);

        with(|a| {
            a.main = main;
            a.host = host;
            a.preview = preview;
            a.status = status;
            a.copies = copies;
            a.tools = vec![refresh, settings, lbl, copies, status];
        });

        reload(false);
        SetTimer(main, TIMER_POLL, 60_000, None);
        ShowWindow(main, SW_SHOWDEFAULT);
        layout_main();
        UpdateWindow(main);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            let settings = with(|a| a.settings).unwrap_or(null_mut());
            if !settings.is_null() && IsDialogMessageW(settings, &msg) != 0 {
                continue;
            }
            if IsDialogMessageW(main, &msg) != 0 {
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

// ---------------------------------------------------------------------------
// Loading

fn reload(manual: bool) {
    let Some((path, marking)) = with(|a| (a.cfg.spreadsheet.clone(), a.cfg.marking())) else { return };
    let p = Path::new(&path);
    let mtime = std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let file = p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or(path.clone());
    match xlsx::read_products(p, marking) {
        Ok(ps) => {
            let n = ps.len();
            let changed = with(|a| {
                a.last_mtime = mtime;
                a.last_load = Instant::now();
                let changed = a.products != ps;
                let first = a.products.is_empty();
                if changed {
                    a.products = ps;
                    if let Some(pp) = &a.preview_product {
                        a.preview_product = a.products.iter().find(|p| p.name == pp.name).cloned();
                    }
                }
                (changed, first)
            })
            .unwrap_or((false, true));
            let (changed, first) = changed;
            if changed {
                rebuild_buttons();
                invalidate_preview();
            }
            set_status(&format!(
                "{n} product{} loaded from {file} at {}{}",
                if n == 1 { "" } else { "s" },
                now_hhmm(),
                if changed && !manual && !first { " – updated" } else { "" }
            ));
        }
        Err(e) => {
            // Leave last_mtime alone so a temporary problem (file mid-save) is retried next minute.
            let (has, main) = with(|a| {
                a.last_load = Instant::now();
                (!a.products.is_empty(), a.main)
            })
            .unwrap_or((false, null_mut()));
            let tail = if has { "Still using the last good list." } else { "Use Settings to choose the spreadsheet." };
            set_status(&format!("Couldn't read the spreadsheet: {e}. {tail}"));
            if manual {
                message_box(main, &format!("Couldn't read the spreadsheet:\n\n{e}\n\n{tail}"), MB_ICONWARNING);
            }
            if !has {
                if let Some(h) = with(|a| a.host) {
                    unsafe { InvalidateRect(h, null(), 1) };
                }
            }
        }
    }
}

fn poll() {
    let Some((path, last, loaded, mins)) =
        with(|a| (a.cfg.spreadsheet.clone(), a.last_mtime, a.last_load, a.cfg.refresh_minutes.max(1)))
    else {
        return;
    };
    let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    if mtime != last || loaded.elapsed().as_secs() >= mins as u64 * 60 {
        reload(false);
    }
}

// ---------------------------------------------------------------------------
// Layout

fn layout_main() {
    let Some((main, host, preview, tools)) = with(|a| (a.main, a.host, a.preview, a.tools.clone())) else { return };
    unsafe {
        let mut rc: RECT = std::mem::zeroed();
        GetClientRect(main, &mut rc);
        let (w, h) = (rc.right, rc.bottom);
        let pad = s(8);
        let bh = s(34);
        let th = bh + 2 * pad;
        let mut x = pad;
        let widths = [s(160), s(110), s(70), s(64)];
        for (i, &tw) in widths.iter().enumerate() {
            let (y, hh) = if i == 3 { (pad + s(5), s(300)) } else { (pad, bh) };
            MoveWindow(tools[i], x, y, tw, hh, 1);
            x += tw + pad;
        }
        MoveWindow(tools[4], x + pad, pad, (w - x - 2 * pad).max(0), bh, 1);
        let pw = s(340).min(w / 2);
        MoveWindow(host, 0, th, (w - pw).max(0), (h - th).max(0), 1);
        MoveWindow(preview, w - pw, th, pw, (h - th).max(0), 1);
    }
}

fn rebuild_buttons() {
    let Some((old, host, names, font)) = with(|a| {
        (std::mem::take(&mut a.buttons), a.host, a.products.iter().map(|p| p.name.clone()).collect::<Vec<_>>(), a.btn_font)
    }) else {
        return;
    };
    unsafe {
        for b in old {
            DestroyWindow(b);
        }
    }
    let style = WS_TABSTOP | (BS_PUSHBUTTON | BS_MULTILINE | BS_CENTER | BS_VCENTER) as u32;
    let new: Vec<HWND> =
        names.iter().enumerate().map(|(i, n)| child("BUTTON", &n.replace('&', "&&"), style, 0, host, ID_PRODUCT + i, font)).collect();
    with(|a| {
        a.buttons = new;
        a.scroll = 0;
    });
    layout_buttons();
    unsafe { InvalidateRect(host, null(), 1) };
}

struct Plan {
    cols: i32,
    bw: i32,
    bh: i32,
    content_h: i32,
}

/// Picks a grid that makes the buttons as large as possible without scrolling;
/// if they can't fit at a sensible minimum size, falls back to a scrolling grid.
fn plan(n: i32, w: i32, h: i32) -> Plan {
    let gap = s(12);
    let (minw, minh, maxh) = (s(140), s(70), s(220));
    let mut best: Option<(f32, Plan)> = None;
    for cols in 1..=n.max(1) {
        let bw = (w - gap * (cols + 1)) / cols;
        if bw < minw && cols > 1 {
            break;
        }
        let rows = (n + cols - 1) / cols;
        let bh = (h - gap * (rows + 1)) / rows.max(1);
        if bh < minh {
            continue;
        }
        let bh = bh.min(maxh);
        let score = (bw as f32 / 2.2).min(bh as f32);
        if best.as_ref().map_or(true, |(b, _)| score > *b) {
            best = Some((score, Plan { cols, bw, bh, content_h: gap + rows * (bh + gap) }));
        }
    }
    if let Some((_, p)) = best {
        return p;
    }
    let cols = ((w - gap) / (minw + gap)).clamp(1, n.max(1));
    let bw = ((w - gap * (cols + 1)) / cols).max(s(60));
    let bh = minh + s(10);
    let rows = (n + cols - 1) / cols;
    Plan { cols, bw, bh, content_h: gap + rows * (bh + gap) }
}

fn layout_buttons() {
    let Some(Some((host, buttons, longest, cur_px))) = with(|a| {
        if a.in_layout {
            return None;
        }
        a.in_layout = true;
        let longest = a.products.iter().map(|p| p.name.chars().count()).max().unwrap_or(1).max(1);
        Some((a.host, a.buttons.clone(), longest, a.btn_font_px))
    }) else {
        return;
    };
    unsafe {
        let n = buttons.len() as i32;
        let mut rc: RECT = std::mem::zeroed();
        GetClientRect(host, &mut rc);
        let has_sb = (GetWindowLongPtrW(host, GWL_STYLE) as u32 & WS_VSCROLL) != 0;
        let sbw = GetSystemMetrics(SM_CXVSCROLL);
        let full_w = rc.right + if has_sb { sbw } else { 0 };
        let h = rc.bottom;
        let mut p = plan(n, full_w, h);
        if p.content_h > h {
            p = plan(n, full_w - sbw, h);
        }
        let gap = s(12);
        let rects: Vec<RECT> = (0..n)
            .map(|i| {
                let (r, c) = (i / p.cols, i % p.cols);
                let x = gap + c * (p.bw + gap);
                let y = gap + r * (p.bh + gap);
                RECT { left: x, top: y, right: x + p.bw, bottom: y + p.bh }
            })
            .collect();

        // Font size follows the button size; long names get a smaller font (up to 2 lines).
        let by_height = p.bh as f32 * 0.24;
        let by_width = p.bw as f32 * 2.0 / (longest as f32 * 0.55);
        let px = (by_height.min(by_width) as i32).clamp(s(12), s(34));
        let mut new_font = null_mut();
        if px != cur_px {
            let face = wide("Segoe UI");
            new_font = CreateFontW(-px, 0, 0, 0, 600, 0, 0, 0, 1, 0, 0, 5, 0, face.as_ptr());
        }

        let max_scroll = (p.content_h - h).max(0);
        let (scroll, old_font, font) = with(|a| {
            a.rects = rects;
            a.content_h = p.content_h;
            a.scroll = a.scroll.clamp(0, max_scroll);
            let old = if !new_font.is_null() {
                a.btn_font_px = px;
                std::mem::replace(&mut a.btn_font, new_font)
            } else {
                null_mut()
            };
            (a.scroll, old, a.btn_font)
        })
        .unwrap();
        if !new_font.is_null() {
            for &b in &buttons {
                SendMessageW(b, WM_SETFONT, font as usize, 0);
            }
            if !old_font.is_null() {
                DeleteObject(old_font as _);
            }
        }
        let si = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: p.content_h - 1,
            nPage: h.max(0) as u32,
            nPos: scroll,
            nTrackPos: 0,
        };
        SetScrollInfo(host, SB_VERT, &si, 1);
    }
    with(|a| a.in_layout = false);
    apply_positions();
}

fn apply_positions() {
    let Some((buttons, rects, scroll)) = with(|a| (a.buttons.clone(), a.rects.clone(), a.scroll)) else { return };
    unsafe {
        for (b, r) in buttons.iter().zip(rects.iter()) {
            MoveWindow(*b, r.left, r.top - scroll, r.right - r.left, r.bottom - r.top, 1);
        }
    }
}

fn scroll_to(pos: i32) {
    let Some((host, content_h)) = with(|a| (a.host, a.content_h)) else { return };
    unsafe {
        let mut rc: RECT = std::mem::zeroed();
        GetClientRect(host, &mut rc);
        let pos = pos.clamp(0, (content_h - rc.bottom).max(0));
        with(|a| a.scroll = pos);
        let si = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_POS,
            nMin: 0,
            nMax: 0,
            nPage: 0,
            nPos: pos,
            nTrackPos: 0,
        };
        SetScrollInfo(host, SB_VERT, &si, 1);
    }
    apply_positions();
}

fn invalidate_preview() {
    if let Some(h) = with(|a| a.preview) {
        unsafe { InvalidateRect(h, null(), 0) };
    }
}

// ---------------------------------------------------------------------------
// Printing

fn spawn_print(product: Product, cfg: Config, copies: u32) {
    let Some(main) = with(|a| a.main) else { return };
    set_status(&format!("Printing {}…", product.name));
    let main = main as usize;
    std::thread::spawn(move || {
        let (ok, msg) = match label::print_label(&product, &cfg, copies) {
            Ok(printer) => (
                true,
                format!(
                    "Printed {} ({} label{}) on {} at {}",
                    product.name,
                    copies,
                    if copies == 1 { "" } else { "s" },
                    printer,
                    now_hhmm()
                ),
            ),
            Err(e) => (false, format!("Couldn't print {}: {e}", product.name)),
        };
        let b = Box::into_raw(Box::new(msg));
        unsafe {
            if PostMessageW(main as HWND, WM_APP_STATUS, (!ok) as usize, b as isize) == 0 {
                drop(Box::from_raw(b));
            }
        }
    });
}

fn on_product(idx: usize) {
    let Some((p, cfg, copies_h)) = with(|a| (a.products.get(idx).cloned(), a.cfg.clone(), a.copies)) else { return };
    let Some(p) = p else { return };
    let copies = unsafe { SendMessageW(copies_h, CB_GETCURSEL, 0, 0) }.max(0) as u32 + 1;
    with(|a| a.preview_product = Some(p.clone()));
    invalidate_preview();
    spawn_print(p, cfg, copies);
}

// ---------------------------------------------------------------------------
// Window procedures

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_SIZE => {
            layout_main();
            0
        }
        WM_GETMINMAXINFO => {
            let mmi = lp as *mut MINMAXINFO;
            (*mmi).ptMinTrackSize = POINT { x: s(720), y: s(420) };
            0
        }
        WM_COMMAND => {
            match loword(wp) as usize {
                ID_REFRESH => reload(true),
                ID_SETTINGS => open_settings(),
                _ => return DefWindowProcW(hwnd, msg, wp, lp),
            }
            0
        }
        WM_TIMER if wp == TIMER_POLL => {
            poll();
            0
        }
        WM_APP_STATUS => {
            let b = Box::from_raw(lp as *mut String);
            set_status(&b);
            if wp != 0 {
                message_box(hwnd, &b, MB_ICONERROR);
            }
            0
        }
        WM_MOUSEWHEEL => {
            if let Some(h) = with(|a| a.host) {
                SendMessageW(h, msg, wp, lp);
            }
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe extern "system" fn host_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_SIZE => {
            layout_buttons();
            0
        }
        WM_COMMAND if hiword(wp) == BN_CLICKED && loword(wp) as usize >= ID_PRODUCT => {
            on_product(loword(wp) as usize - ID_PRODUCT);
            0
        }
        WM_VSCROLL => {
            let Some(cur) = with(|a| a.scroll) else { return 0 };
            let mut si: SCROLLINFO = std::mem::zeroed();
            si.cbSize = std::mem::size_of::<SCROLLINFO>() as u32;
            si.fMask = SIF_ALL;
            GetScrollInfo(hwnd, SB_VERT, &mut si);
            let line = s(40);
            let page = si.nPage as i32;
            let pos = match loword(wp) as i32 {
                x if x == SB_LINEUP as i32 => cur - line,
                x if x == SB_LINEDOWN as i32 => cur + line,
                x if x == SB_PAGEUP as i32 => cur - page,
                x if x == SB_PAGEDOWN as i32 => cur + page,
                x if x == SB_THUMBTRACK as i32 || x == SB_THUMBPOSITION as i32 => si.nTrackPos,
                x if x == SB_TOP as i32 => 0,
                x if x == SB_BOTTOM as i32 => i32::MAX / 2,
                _ => cur,
            };
            scroll_to(pos);
            0
        }
        WM_MOUSEWHEEL => {
            let delta = hiword(wp) as u16 as i16 as i32;
            if let Some(cur) = with(|a| a.scroll) {
                scroll_to(cur - delta * s(60) / 120);
            }
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            if let Some((empty, font)) = with(|a| (a.buttons.is_empty(), a.ui_font)) {
                if empty {
                    let mut rc: RECT = std::mem::zeroed();
                    GetClientRect(hwnd, &mut rc);
                    rc.left += s(24);
                    rc.right -= s(24);
                    rc.top += s(40);
                    SelectObject(hdc, font as _);
                    SetBkMode(hdc, TRANSPARENT as _);
                    let mut t = wide("No products loaded yet.\n\nPress Settings… to choose your products spreadsheet, then Refresh products.");
                    DrawTextW(hdc, t.as_mut_ptr(), -1, &mut rc, DT_CENTER | DT_WORDBREAK);
                }
            }
            EndPaint(hwnd, &ps);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe extern "system" fn preview_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,
        WM_SIZE => {
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rc: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rc);
            // Double-buffer to avoid flicker.
            let mem = CreateCompatibleDC(hdc);
            let bmp = CreateCompatibleBitmap(hdc, rc.right.max(1), rc.bottom.max(1));
            let old_bmp = SelectObject(mem, bmp as _);
            FillRect(mem, &rc, GetSysColorBrush(COLOR_BTNFACE));
            if let Some((product, cfg, font)) = with(|a| (a.preview_product.clone(), a.cfg.clone(), a.ui_font)) {
                paint_preview(mem, &rc, product.as_ref(), &cfg, font);
            }
            BitBlt(hdc, 0, 0, rc.right, rc.bottom, mem, 0, 0, SRCCOPY);
            SelectObject(mem, old_bmp);
            DeleteObject(bmp as _);
            DeleteDC(mem);
            EndPaint(hwnd, &ps);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe fn paint_preview(hdc: HDC, rc: &RECT, product: Option<&Product>, cfg: &Config, ui_font: HFONT) {
    let pad = s(16);
    SetBkMode(hdc, TRANSPARENT as _);
    SelectObject(hdc, ui_font as _);
    let mut cap = RECT { left: pad, top: s(4), right: rc.right - pad, bottom: s(30) };
    let mut t = wide(if product.is_some() { "Last label printed" } else { "Label preview" });
    SetTextColor(hdc, 0x505050);
    DrawTextW(hdc, t.as_mut_ptr(), -1, &mut cap, DT_LEFT | DT_SINGLELINE | DT_VCENTER);

    let lw = (rc.right - 2 * pad).max(s(80));
    let x0 = pad;
    let y0 = s(34);
    let Some(p) = product else {
        let mut r = RECT { left: x0, top: y0 + s(10), right: x0 + lw, bottom: rc.bottom };
        let mut t = wide("Press a product button to print its label. The label will be shown here.");
        DrawTextW(hdc, t.as_mut_ptr(), -1, &mut r, DT_CENTER | DT_WORDBREAK);
        return;
    };
    // Scale the label so its real width (mm) fills the panel.
    let dpi = lw as f32 / (cfg.label_width_mm.max(20.0) / 25.4);
    let m = (cfg.margin_mm.max(0.0) * dpi / 25.4) as i32 + s(4);
    let fonts = label::Fonts::new(hdc, cfg, dpi);
    let (lines, h) = label::layout(hdc, &fonts, lw - 2 * m, p, cfg);
    let paper = RECT { left: x0, top: y0, right: x0 + lw, bottom: y0 + h + 2 * m };
    FillRect(hdc, &paper, GetStockObject(WHITE_BRUSH) as HBRUSH);
    let border = CreateSolidBrush(0x00B0B0B0);
    FrameRect(hdc, &paper, border);
    DeleteObject(border as _);
    label::draw(hdc, &fonts, &lines, x0 + m, y0 + m);
}

// ---------------------------------------------------------------------------
// Settings window

fn open_settings() {
    let Some((existing, main, cfg, font, hinst)) = with(|a| (a.settings, a.main, a.cfg.clone(), a.ui_font, a.hinst)) else { return };
    unsafe {
        if !existing.is_null() {
            SetForegroundWindow(existing);
            return;
        }
        let cls = wide("AllergenSettings");
        let title = wide("Settings");
        let (w, h) = (s(660), s(710));
        let mut mr: RECT = std::mem::zeroed();
        GetWindowRect(main, &mut mr);
        let x = mr.left + ((mr.right - mr.left) - w) / 2;
        let y = mr.top + ((mr.bottom - mr.top) - h) / 2;
        let wnd = CreateWindowExW(
            WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT,
            cls.as_ptr(),
            title.as_ptr(),
            WS_POPUP | WS_CAPTION | WS_SYSMENU,
            x.max(0),
            y.max(0),
            w,
            h,
            main,
            null_mut(),
            hinst,
            null(),
        );
        with(|a| a.settings = wnd);

        let lx = s(16);
        let lw = s(200);
        let cx = lx + lw + s(8);
        let rh = s(28);
        let step = s(40);
        let mut y = s(16);
        let edit = WS_TABSTOP | ES_AUTOHSCROLL as u32;
        let label = |text: &str, x: i32, y: i32, w: i32| {
            let h = child("STATIC", text, (SS_LEFT | SS_CENTERIMAGE) as u32, 0, wnd, 0, font);
            MoveWindow(h, x, y, w, rh, 0);
        };
        let place = |id: i32, class: &str, text: &str, style: u32, ex: u32, x: i32, y: i32, w: i32, h: i32| {
            // Single-line edits don't centre text vertically, so keep them snug.
            let (y, h) = if class == "EDIT" { (y + (h - s(25)) / 2, s(25)) } else { (y, h) };
            let c = child(class, text, style, ex, wnd, id as usize, font);
            MoveWindow(c, x, y, w, h, 0);
            c
        };

        label("Spreadsheet file:", lx, y, lw);
        place(S_PATH, "EDIT", &cfg.spreadsheet, edit, WS_EX_CLIENTEDGE, cx, y, s(310), rh);
        place(S_BROWSE, "BUTTON", "Browse…", WS_TABSTOP, 0, cx + s(318), y, s(96), rh);
        y += step;

        label("Printer:", lx, y, lw);
        let pc = place(S_PRINTER, "COMBOBOX", "", WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32, 0, cx, y + s(2), s(414), s(300));
        let mut printers = vec!["(Windows default printer)".to_string()];
        printers.extend(label::list_printers());
        if !cfg.printer.is_empty() && !printers.iter().any(|p| p == &cfg.printer) {
            printers.push(cfg.printer.clone());
        }
        for (i, p) in printers.iter().enumerate() {
            let t = wide(p);
            SendMessageW(pc, CB_ADDSTRING, 0, t.as_ptr() as isize);
            if (i == 0 && cfg.printer.is_empty()) || (i > 0 && *p == cfg.printer) {
                SendMessageW(pc, CB_SETCURSEL, i, 0);
            }
        }
        y += step;

        label("Label width (mm):", lx, y, lw);
        place(S_WIDTH, "EDIT", &fmt_num(cfg.label_width_mm), edit, WS_EX_CLIENTEDGE, cx, y, s(70), rh);
        label("Extra side margin (mm):", cx + s(90), y, s(180));
        place(S_MARGIN, "EDIT", &fmt_num(cfg.margin_mm), edit, WS_EX_CLIENTEDGE, cx + s(280), y, s(70), rh);
        y += step;

        label("Font:", lx, y, lw);
        place(S_FONT, "EDIT", &cfg.font_face, edit, WS_EX_CLIENTEDGE, cx, y, s(200), rh);
        y += step;

        label("Product name size (pt):", lx, y, lw);
        place(S_TITLE, "EDIT", &fmt_num(cfg.title_pt), edit, WS_EX_CLIENTEDGE, cx, y, s(70), rh);
        label("Ingredient text size (pt):", cx + s(90), y, s(180));
        place(S_BODY, "EDIT", &fmt_num(cfg.body_pt), edit, WS_EX_CLIENTEDGE, cx + s(280), y, s(70), rh);
        y += step;

        label("Ingredient layout:", lx, y, lw);
        let lc = place(S_LAYOUT, "COMBOBOX", "", WS_TABSTOP | CBS_DROPDOWNLIST as u32, 0, cx, y + s(2), s(414), s(200));
        for t in ["Comma-separated, like a UK food label", "One ingredient per line"] {
            let t = wide(t);
            SendMessageW(lc, CB_ADDSTRING, 0, t.as_ptr() as isize);
        }
        SendMessageW(lc, CB_SETCURSEL, cfg.one_per_line as usize, 0);
        y += step;

        label("Allergens are:", lx, y, lw);
        let mc = place(S_MARKING, "COMBOBOX", "", WS_TABSTOP | CBS_DROPDOWNLIST as u32, 0, cx, y + s(2), s(414), s(200));
        for t in ["Text in (brackets) – only the bracketed text prints bold", "Bold text in the spreadsheet"] {
            let t = wide(t);
            SendMessageW(mc, CB_ADDSTRING, 0, t.as_ptr() as isize);
        }
        SendMessageW(mc, CB_SETCURSEL, (!cfg.allergens_in_brackets) as usize, 0);
        y += step;

        label("Re-read every (minutes):", lx, y, lw);
        place(S_REFRESH, "EDIT", &cfg.refresh_minutes.to_string(), edit | ES_NUMBER as u32, WS_EX_CLIENTEDGE, cx, y, s(70), rh);
        label("Saved changes are also picked up within a minute.", cx + s(80), y, s(340));
        y += step;

        label("Footer text (optional):", lx, y, lw);
        place(S_FOOTER, "EDIT", &cfg.footer, edit, WS_EX_CLIENTEDGE, cx, y, s(414), rh);
        y += step;

        label("Business name:", lx, y, lw);
        place(S_BUSINESS, "EDIT", &cfg.business_name, edit, WS_EX_CLIENTEDGE, cx, y, s(200), rh);
        y += step;

        label("Address:", lx, y, lw);
        place(S_ADDRESS, "EDIT", &cfg.address, edit, WS_EX_CLIENTEDGE, cx, y, s(414), rh);
        y += step;

        label("Contact:", lx, y, lw);
        place(S_CONTACT, "EDIT", &cfg.contact, edit, WS_EX_CLIENTEDGE, cx, y, s(414), rh);
        y += step;

        label("Default best before:", lx, y, lw);
        let bb = place(S_BESTBEFORE, "COMBOBOX", "", WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32, 0, cx, y + s(2), s(110), s(300));
        for d in 1..=10 {
            let t = wide(&format!("{d} day{}", if d == 1 { "" } else { "s" }));
            SendMessageW(bb, CB_ADDSTRING, 0, t.as_ptr() as isize);
        }
        SendMessageW(bb, CB_SETCURSEL, (cfg.best_before_days.clamp(1, 10) - 1) as usize, 0);
        label("Used when the spreadsheet gives no shelf life.", cx + s(120), y, s(300));
        y += step;

        label("Warning (bottom of label):", lx, y, lw);
        place(S_WARNING, "EDIT", &cfg.warning, edit, WS_EX_CLIENTEDGE, cx, y, s(414), rh);

        let by = h - s(100);
        place(S_TEST, "BUTTON", "Print test label", WS_TABSTOP, 0, lx, by, s(150), s(34));
        place(IDOK, "BUTTON", "Save", WS_TABSTOP | BS_DEFPUSHBUTTON as u32, 0, w - s(250), by, s(100), s(34));
        place(IDCANCEL, "BUTTON", "Cancel", WS_TABSTOP, 0, w - s(140), by, s(100), s(34));

        EnableWindow(main, 0);
        ShowWindow(wnd, SW_SHOW);
        SetFocus(GetDlgItem(wnd, IDOK));
    }
}

fn fmt_num(v: f32) -> String {
    if (v - v.round()).abs() < 0.001 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

fn close_settings() {
    let Some((wnd, main)) = with(|a| (std::mem::replace(&mut a.settings, null_mut()), a.main)) else { return };
    unsafe {
        EnableWindow(main, 1);
        if !wnd.is_null() {
            DestroyWindow(wnd);
        }
        SetForegroundWindow(main);
    }
}

fn read_form(wnd: HWND) -> Result<Config, String> {
    let mut cfg = with(|a| a.cfg.clone()).unwrap_or_default();
    let item = |id: i32| unsafe { GetDlgItem(wnd, id) };
    let num = |id: i32, what: &str, lo: f32, hi: f32| -> Result<f32, String> {
        let t = get_text(item(id));
        match t.trim().parse::<f32>() {
            Ok(v) if v >= lo && v <= hi => Ok(v),
            _ => Err(format!("{what} should be a number between {lo} and {hi}.")),
        }
    };
    cfg.spreadsheet = get_text(item(S_PATH)).trim().trim_matches('"').to_string();
    let sel = unsafe { SendMessageW(item(S_PRINTER), CB_GETCURSEL, 0, 0) };
    cfg.printer = if sel <= 0 {
        String::new()
    } else {
        unsafe {
            let n = SendMessageW(item(S_PRINTER), CB_GETLBTEXTLEN, sel as usize, 0).max(0) as usize;
            let mut buf = vec![0u16; n + 1];
            SendMessageW(item(S_PRINTER), CB_GETLBTEXT, sel as usize, buf.as_mut_ptr() as isize);
            String::from_utf16_lossy(&buf[..n])
        }
    };
    cfg.label_width_mm = num(S_WIDTH, "Label width", 20.0, 120.0)?;
    cfg.margin_mm = num(S_MARGIN, "Side margin", 0.0, 20.0)?;
    cfg.font_face = get_text(item(S_FONT)).trim().to_string();
    if cfg.font_face.is_empty() {
        cfg.font_face = "Arial".into();
    }
    cfg.title_pt = num(S_TITLE, "Product name size", 6.0, 48.0)?;
    cfg.body_pt = num(S_BODY, "Ingredient text size", 5.0, 30.0)?;
    cfg.one_per_line = unsafe { SendMessageW(item(S_LAYOUT), CB_GETCURSEL, 0, 0) } == 1;
    cfg.refresh_minutes = num(S_REFRESH, "Refresh interval", 1.0, 1440.0)? as u32;
    cfg.footer = get_text(item(S_FOOTER)).trim().to_string();
    cfg.business_name = get_text(item(S_BUSINESS)).trim().to_string();
    cfg.allergens_in_brackets = unsafe { SendMessageW(item(S_MARKING), CB_GETCURSEL, 0, 0) } != 1;
    cfg.address = get_text(item(S_ADDRESS)).trim().to_string();
    cfg.contact = get_text(item(S_CONTACT)).trim().to_string();
    cfg.warning = get_text(item(S_WARNING)).trim().to_string();
    cfg.best_before_days = unsafe { SendMessageW(item(S_BESTBEFORE), CB_GETCURSEL, 0, 0) }.clamp(0, 9) as u32 + 1;
    Ok(cfg)
}

fn browse(wnd: HWND) {
    unsafe {
        let cur = get_text(GetDlgItem(wnd, S_PATH));
        let mut buf = vec![0u16; 2048];
        for (i, c) in cur.encode_utf16().take(2000).enumerate() {
            buf[i] = c;
        }
        let filter: Vec<u16> = "Excel workbooks (*.xlsx)\0*.xlsx\0All files (*.*)\0*.*\0\0".encode_utf16().collect();
        let title = wide("Choose the products spreadsheet");
        let mut ofn: OPENFILENAMEW = std::mem::zeroed();
        ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        ofn.hwndOwner = wnd;
        ofn.lpstrFilter = filter.as_ptr();
        ofn.lpstrFile = buf.as_mut_ptr();
        ofn.nMaxFile = buf.len() as u32;
        ofn.lpstrTitle = title.as_ptr();
        ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_HIDEREADONLY;
        if GetOpenFileNameW(&mut ofn) != 0 {
            let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
            set_text(GetDlgItem(wnd, S_PATH), &String::from_utf16_lossy(&buf[..n]));
        }
    }
}

fn sample_product() -> Product {
    let r = |t: &str, b: bool| vec![Run { text: t.into(), bold: b }];
    Product {
        name: "Test Label".into(),
        ingredients: vec![r("Wheat flour", true), r("Sugar", false), r("Butter (milk)", true), r("Free range eggs", true), r("Vanilla", false)],
        shelf_life_days: None,
    }
}

unsafe extern "system" fn settings_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = loword(wp) as i32;
            match id {
                S_BROWSE => browse(hwnd),
                S_TEST => match read_form(hwnd) {
                    Ok(cfg) => {
                        let p = with(|a| a.products.first().cloned()).flatten().unwrap_or_else(sample_product);
                        spawn_print(p, cfg, 1);
                    }
                    Err(e) => message_box(hwnd, &e, MB_ICONWARNING),
                },
                IDOK => match read_form(hwnd) {
                    Ok(cfg) => {
                        if let Err(e) = cfg.save() {
                            message_box(hwnd, &e, MB_ICONWARNING);
                        }
                        let path_changed = with(|a| {
                            let changed = a.cfg.spreadsheet != cfg.spreadsheet
                                || a.cfg.allergens_in_brackets != cfg.allergens_in_brackets;
                            a.cfg = cfg;
                            changed
                        })
                        .unwrap_or(false);
                        close_settings();
                        if path_changed {
                            with(|a| {
                                a.products.clear();
                                a.last_mtime = None;
                            });
                            reload(true);
                            rebuild_buttons();
                        }
                        invalidate_preview();
                    }
                    Err(e) => message_box(hwnd, &e, MB_ICONWARNING),
                },
                IDCANCEL => close_settings(),
                _ => return DefWindowProcW(hwnd, msg, wp, lp),
            }
            0
        }
        WM_CLOSE => {
            close_settings();
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
