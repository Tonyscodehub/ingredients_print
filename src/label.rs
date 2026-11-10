//! Label layout and rendering with GDI. The same code draws the on-screen
//! preview and the printed label, so what you see is what prints.

use crate::config::Config;
use crate::xlsx::Product;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::SIZE;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Graphics::Printing::*;
use windows_sys::Win32::Storage::Xps::*;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

/// Today's date on this PC as (year, month, day).
pub fn today() -> (i32, u32, u32) {
    unsafe {
        let mut st: windows_sys::Win32::Foundation::SYSTEMTIME = std::mem::zeroed();
        GetLocalTime(&mut st);
        (st.wYear as i32, st.wMonth as u32, st.wDay as u32)
    }
}

fn font_index(s: crate::content::Style) -> usize {
    use crate::content::Style;
    match s {
        Style::Title => TITLE,
        Style::Body => BODY,
        Style::Bold => BOLD,
        Style::Footer => FOOTER,
    }
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

const TITLE: usize = 0;
const BODY: usize = 1;
const BOLD: usize = 2;
const FOOTER: usize = 3;

pub struct Fonts {
    h: [HFONT; 4],
    ascent: [i32; 4],
    height: [i32; 4],
}

fn make_font(face: &str, px: i32, bold: bool, italic: bool) -> HFONT {
    let face = wide(face);
    unsafe {
        CreateFontW(
            -px.max(1),
            0,
            0,
            0,
            if bold { 700 } else { 400 },
            italic as u32,
            0,
            0,
            1, // DEFAULT_CHARSET
            0,
            0,
            0, // DEFAULT_QUALITY
            0,
            face.as_ptr(),
        )
    }
}

impl Fonts {
    /// `dpi` is the vertical resolution of the target (printer or virtual preview dpi).
    pub fn new(hdc: HDC, cfg: &Config, dpi: f32) -> Fonts {
        let px = |pt: f32| (pt * dpi / 72.0).round() as i32;
        let face = if cfg.font_face.trim().is_empty() { "Arial" } else { cfg.font_face.trim() };
        let h = [
            make_font(face, px(cfg.title_pt), true, false),
            make_font(face, px(cfg.body_pt), false, false),
            make_font(face, px(cfg.body_pt), true, false),
            make_font(face, px(cfg.body_pt * 0.85), false, true),
        ];
        let mut ascent = [0; 4];
        let mut height = [0; 4];
        unsafe {
            let old = SelectObject(hdc, h[0] as _);
            for i in 0..4 {
                SelectObject(hdc, h[i] as _);
                let mut tm: TEXTMETRICW = std::mem::zeroed();
                GetTextMetricsW(hdc, &mut tm);
                ascent[i] = tm.tmAscent;
                height[i] = tm.tmHeight + tm.tmExternalLeading;
            }
            SelectObject(hdc, old);
        }
        Fonts { h, ascent, height }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        for h in self.h {
            unsafe { DeleteObject(h as _) };
        }
    }
}

pub struct Placed {
    x: i32,
    text: Vec<u16>,
    font: usize,
}

pub struct Line {
    pub y: i32,
    pub height: i32,
    ascent: i32,
    items: Vec<Placed>,
}

fn measure(hdc: HDC, fonts: &Fonts, s: &[u16], f: usize) -> i32 {
    if s.is_empty() {
        return 0;
    }
    unsafe {
        SelectObject(hdc, fonts.h[f] as _);
        let mut sz = SIZE { cx: 0, cy: 0 };
        GetTextExtentPoint32W(hdc, s.as_ptr(), s.len() as i32, &mut sz);
        sz.cx
    }
}

/// A word is a run of non-space text, possibly mixing fonts ("**milk**)," stays together).
struct Word {
    segs: Vec<(Vec<u16>, usize)>,
    space_before: Option<usize>,
}

fn words(runs: &[(String, usize)]) -> Vec<Word> {
    let mut out: Vec<Word> = Vec::new();
    let mut pending_space: Option<usize> = None;
    let mut in_word = false;
    for (text, f) in runs {
        for ch in text.chars() {
            if ch == ' ' {
                pending_space = Some(*f);
                in_word = false;
                continue;
            }
            if !in_word {
                out.push(Word { segs: Vec::new(), space_before: pending_space.take() });
                in_word = true;
            }
            let w = out.last_mut().unwrap();
            if w.segs.last().map_or(true, |s| s.1 != *f) {
                w.segs.push((Vec::new(), *f));
            }
            let mut buf = [0u16; 2];
            w.segs.last_mut().unwrap().0.extend_from_slice(ch.encode_utf16(&mut buf));
        }
    }
    out
}

fn finish_line(lines: &mut Vec<Line>, items: Vec<Placed>, fonts: &Fonts, width: i32, x_end: i32, center: bool, base_font: usize) {
    let mut ascent = fonts.ascent[base_font];
    let mut height = fonts.height[base_font];
    for it in &items {
        ascent = ascent.max(fonts.ascent[it.font]);
        height = height.max(fonts.height[it.font]);
    }
    let shift = if center { ((width - x_end) / 2).max(0) } else { 0 };
    let items = items.into_iter().map(|mut p| {
        p.x += shift;
        p
    });
    lines.push(Line { y: 0, height, ascent, items: items.collect() });
}

fn wrap(hdc: HDC, fonts: &Fonts, runs: &[(String, usize)], width: i32, center: bool, base_font: usize) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut items: Vec<Placed> = Vec::new();
    let mut x = 0;
    for w in words(runs) {
        let ww: i32 = w.segs.iter().map(|(s, f)| measure(hdc, fonts, s, *f)).sum();
        let sw = match w.space_before {
            Some(f) if x > 0 => measure(hdc, fonts, &[b' ' as u16], f),
            _ => 0,
        };
        if x > 0 && x + sw + ww > width {
            finish_line(&mut lines, std::mem::take(&mut items), fonts, width, x, center, base_font);
            x = 0;
        } else {
            x += sw;
        }
        if ww <= width || x > 0 {
            for (s, f) in w.segs {
                let sw = measure(hdc, fonts, &s, f);
                items.push(Placed { x, text: s, font: f });
                x += sw;
            }
        } else {
            // A single word wider than the label: break it by characters.
            for (s, f) in w.segs {
                for ch in s {
                    let cw = measure(hdc, fonts, &[ch], f);
                    if x > 0 && x + cw > width {
                        finish_line(&mut lines, std::mem::take(&mut items), fonts, width, x, center, base_font);
                        x = 0;
                    }
                    items.push(Placed { x, text: vec![ch], font: f });
                    x += cw;
                }
            }
        }
    }
    if !items.is_empty() {
        finish_line(&mut lines, items, fonts, width, x, center, base_font);
    }
    lines
}

/// Lays out a whole label. Returns the lines (with y set) and total height.
pub fn layout(hdc: HDC, fonts: &Fonts, width: i32, product: &Product, cfg: &Config) -> (Vec<Line>, i32) {
    let mut all = Vec::new();
    let mut y = 0;
    for p in crate::content::paragraphs(product, cfg, today()) {
        let runs: Vec<(String, usize)> = p.runs.iter().map(|r| (r.text.clone(), font_index(r.style))).collect();
        for mut l in wrap(hdc, fonts, &runs, width, p.center, font_index(p.base)) {
            l.y = y;
            y += l.height;
            all.push(l);
        }
        y += (fonts.height[BODY] as f32 * p.gap_after) as i32;
    }
    (all, y)
}

pub fn draw(hdc: HDC, fonts: &Fonts, lines: &[Line], x0: i32, y0: i32) {
    unsafe {
        SetBkMode(hdc, TRANSPARENT as _);
        SetTextColor(hdc, 0);
        SetTextAlign(hdc, TA_BASELINE | TA_LEFT);
        for l in lines {
            for it in &l.items {
                SelectObject(hdc, fonts.h[it.font] as _);
                TextOutW(hdc, x0 + it.x, y0 + l.y + l.ascent, it.text.as_ptr(), it.text.len() as i32);
            }
        }
        SetTextAlign(hdc, TA_TOP | TA_LEFT);
    }
}

// ---------------------------------------------------------------------------
// Printers

pub fn default_printer() -> Option<String> {
    unsafe {
        let mut n: u32 = 0;
        GetDefaultPrinterW(null_mut(), &mut n);
        if n == 0 {
            return None;
        }
        let mut buf = vec![0u16; n as usize];
        if GetDefaultPrinterW(buf.as_mut_ptr(), &mut n) == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())]))
    }
}

pub fn list_printers() -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
        let (mut needed, mut count) = (0u32, 0u32);
        EnumPrintersW(flags, null(), 4, null_mut(), 0, &mut needed, &mut count);
        if needed == 0 {
            return out;
        }
        let mut buf = vec![0u64; (needed as usize + 7) / 8]; // u64 for alignment
        if EnumPrintersW(flags, null(), 4, buf.as_mut_ptr() as *mut u8, needed, &mut needed, &mut count) == 0 {
            return out;
        }
        let infos = std::slice::from_raw_parts(buf.as_ptr() as *const PRINTER_INFO_4W, count as usize);
        for i in infos {
            let p = i.pPrinterName;
            if p.is_null() {
                continue;
            }
            let mut len = 0;
            while *p.add(len) != 0 {
                len += 1;
            }
            out.push(String::from_utf16_lossy(std::slice::from_raw_parts(p, len)));
        }
    }
    out.sort_by_key(|s| s.to_lowercase());
    out
}

pub fn print_label(product: &Product, cfg: &Config, copies: u32) -> Result<String, String> {
    let name = if cfg.printer.trim().is_empty() {
        default_printer().ok_or("no printer chosen and Windows has no default printer")?
    } else {
        cfg.printer.trim().to_string()
    };
    let wname = wide(&name);
    let driver = wide("WINSPOOL");
    unsafe {
        let hdc = CreateDCW(driver.as_ptr(), wname.as_ptr(), null(), null());
        if hdc.is_null() {
            return Err(format!("couldn't open printer \"{name}\" – check it's switched on and named correctly in Settings"));
        }
        let dpi_x = GetDeviceCaps(hdc, LOGPIXELSX as _).max(72) as f32;
        let dpi_y = GetDeviceCaps(hdc, LOGPIXELSY as _).max(72) as f32;
        let page_w = GetDeviceCaps(hdc, HORZRES as _);
        let page_h = GetDeviceCaps(hdc, VERTRES as _);
        let mx = (cfg.margin_mm.max(0.0) * dpi_x / 25.4) as i32;
        let my = (cfg.margin_mm.max(0.0) * dpi_y / 25.4) as i32;
        let fonts = Fonts::new(hdc, cfg, dpi_y);
        let (lines, _) = layout(hdc, &fonts, (page_w - 2 * mx).max(50), product, cfg);

        let doc_name = wide(&format!("Label – {}", product.name));
        let di = DOCINFOW {
            cbSize: std::mem::size_of::<DOCINFOW>() as i32,
            lpszDocName: doc_name.as_ptr(),
            lpszOutput: null(),
            lpszDatatype: null(),
            fwType: 0,
        };
        if StartDocW(hdc, &di) <= 0 {
            DeleteDC(hdc);
            return Err(format!("printer \"{name}\" refused the job"));
        }
        let usable = (page_h - 2 * my).max(1);
        let mut ok = true;
        'copies: for _ in 0..copies.max(1) {
            // Split into pages if the label is longer than the paper size set in the driver.
            let mut start = 0;
            while start < lines.len() {
                let top = lines[start].y;
                let mut end = start;
                while end < lines.len() && (lines[end].y - top + lines[end].height <= usable || end == start) {
                    end += 1;
                }
                if StartPage(hdc) <= 0 {
                    ok = false;
                    break 'copies;
                }
                draw(hdc, &fonts, &lines[start..end], mx, my - top);
                if EndPage(hdc) <= 0 {
                    ok = false;
                    break 'copies;
                }
                start = end;
            }
        }
        if ok {
            EndDoc(hdc);
        } else {
            AbortDoc(hdc);
        }
        drop(fonts);
        DeleteDC(hdc);
        if ok {
            Ok(name)
        } else {
            Err(format!("printing to \"{name}\" failed part-way"))
        }
    }
}
