//! Minimal .xlsx reader that keeps bold formatting.
//!
//! Reads the first worksheet. Row 1 (the first non-empty row) holds product
//! names, one per column; the cells below hold ingredients. Bold text, either a
//! whole bold cell or bold words inside a cell, is treated as an allergen.

use quick_xml::events::{BytesStart, Event};
use serde::{Deserialize, Serialize};
use quick_xml::Reader;
use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub text: String,
    pub bold: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Product {
    pub name: String,
    /// One entry per ingredient cell; each cell is a list of runs.
    pub ingredients: Vec<Vec<Run>>,
    /// From a "Shelf life: N days" cell in the column, if there is one.
    pub shelf_life_days: Option<u32>,
    /// Bought-in product: barcode from a "Barcode: …" cell.
    pub barcode: Option<String>,
    /// Shop named in the cell below the barcode. Never printed.
    pub store: Option<String>,
    /// "May contain" text from the checked pack details.
    pub may_contain: Option<String>,
    /// Bought-in product whose looked-up details haven't been checked yet.
    pub needs_check: bool,
}

/// "Barcode: 5010251703129", or a cell that is just an 8–14 digit number.
pub fn parse_barcode(text: &str) -> Option<String> {
    let t = text.trim();
    let lower = t.to_lowercase();
    let (rest, labelled) = match lower.strip_prefix("barcode") {
        Some(_) => (&t["barcode".len()..], true),
        None => (t, false),
    };
    let rest = rest.trim_start_matches([':', ' ', '=', '-']);
    let mut digits: String = rest.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    // A number Excel wrote in scientific notation (e.g. 5.01025170313E+12).
    if digits.contains(['E', 'e']) {
        if let Ok(f) = digits.parse::<f64>() {
            digits = format!("{f:.0}");
        }
    }
    let ok = !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit());
    if labelled {
        return ok.then_some(digits).or(Some(String::new()));
    }
    (ok && (8..=14).contains(&digits.len())).then_some(digits)
}

fn strip_store_label(t: &str) -> String {
    let lower = t.to_lowercase();
    for p in ["store", "shop", "from", "supplier"] {
        if let Some(rest) = lower.strip_prefix(p) {
            if rest.starts_with([':', ' ', '=']) {
                return t[p.len()..].trim_start_matches([':', ' ', '=']).trim().to_string();
            }
        }
    }
    t.trim().to_string()
}

/// Recognises "Shelf life: 3 days", "shelf life 3", "Shelf-life = 10 days" etc.
/// Returns Some(None) for a shelf-life cell with no usable number (so it's
/// still kept off the label), None if the cell isn't a shelf-life cell.
pub fn parse_shelf_life(text: &str) -> Option<Option<u32>> {
    let t = text.trim().to_lowercase().replace('-', " ");
    let rest = t.strip_prefix("shelf life")?;
    let digits: String = rest.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
    Some(digits.parse().ok())
}

/// A run as stored in the file: `bold == None` means "use the cell's font".
#[derive(Clone, Debug)]
struct RawRun {
    text: String,
    bold: Option<bool>,
}

type Zip = zip::ZipArchive<Cursor<Vec<u8>>>;

/// How allergens are marked in the spreadsheet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Marking {
    /// Text inside (round brackets) is the allergen; spreadsheet bold is ignored.
    Brackets,
    /// Bold text in the spreadsheet is the allergen.
    Bold,
}

pub fn read_products(path: &Path, marking: Marking) -> Result<Vec<Product>, String> {
    // Read the whole file up front so we hold it for as short a time as possible
    // (it may be open in Excel on the same PC or a shared drive).
    let bytes = std::fs::read(path).map_err(|e| format!("can't open {}: {e}", path.display()))?;
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| "this doesn't look like an .xlsx file (old .xls files need saving as .xlsx)".to_string())?;

    let sheet_path = first_sheet_path(&mut zip)?;
    let xf_bold = bold_by_style(&mut zip);
    let shared = match entry(&mut zip, "xl/sharedStrings.xml") {
        Some(s) => parse_shared_strings(&s)?,
        None => Vec::new(),
    };
    let sheet = entry(&mut zip, &sheet_path).ok_or_else(|| format!("worksheet {sheet_path} is missing"))?;
    let cells = parse_sheet(&sheet, &shared, &xf_bold)?;
    Ok(build_products(cells, marking))
}

/// Re-marks a cell so only text inside brackets is bold (the brackets
/// themselves stay regular). Handles nesting and unclosed brackets.
pub fn bracket_runs(runs: &[Run]) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    let mut depth = 0u32;
    for ch in runs.iter().flat_map(|r| r.text.chars()) {
        let bold = match ch {
            '(' => {
                depth += 1;
                false
            }
            ')' => {
                depth = depth.saturating_sub(1);
                false
            }
            _ => depth > 0,
        };
        match out.last_mut() {
            Some(last) if last.bold == bold => last.text.push(ch),
            _ => out.push(Run { text: ch.to_string(), bold }),
        }
    }
    out
}

// ---------------------------------------------------------------------------

fn entry(zip: &mut Zip, name: &str) -> Option<String> {
    let idx = zip.index_for_name(name).or_else(|| {
        (0..zip.len()).find(|&i| zip.name_for_index(i).map_or(false, |n| n.eq_ignore_ascii_case(name)))
    })?;
    let mut f = zip.by_index(idx).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

fn attr(e: &BytesStart, local: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == local)
        .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()))
}

/// `<b/>` = bold, `<b val="0"/>` or `<b val="false"/>` = not bold.
fn bool_val(e: &BytesStart) -> bool {
    !matches!(attr(e, b"val").as_deref(), Some("0") | Some("false"))
}

fn xml_err(e: quick_xml::Error) -> String {
    format!("spreadsheet XML error: {e}")
}

fn first_sheet_path(zip: &mut Zip) -> Result<String, String> {
    let wb = entry(zip, "xl/workbook.xml").ok_or("workbook.xml is missing – is this an .xlsx file?")?;
    let mut rid = None;
    let mut r = Reader::from_str(&wb);
    loop {
        match r.read_event().map_err(xml_err)? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"sheet" => {
                rid = e
                    .attributes()
                    .flatten()
                    .find(|a| a.key.local_name().as_ref() == b"id" && a.key.prefix().is_some())
                    .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()));
                break;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let fallback = "xl/worksheets/sheet1.xml".to_string();
    let Some(rid) = rid else { return Ok(fallback) };
    let Some(rels) = entry(zip, "xl/_rels/workbook.xml.rels") else { return Ok(fallback) };
    let mut r = Reader::from_str(&rels);
    loop {
        match r.read_event().map_err(xml_err)? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"Relationship" => {
                if attr(&e, b"Id").as_deref() == Some(rid.as_str()) {
                    if let Some(t) = attr(&e, b"Target") {
                        return Ok(if let Some(abs) = t.strip_prefix('/') {
                            abs.to_string()
                        } else {
                            format!("xl/{}", t.trim_start_matches("./"))
                        });
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(fallback)
}

/// For each cell style index (the `s` attribute), whether its font is bold.
fn bold_by_style(zip: &mut Zip) -> Vec<bool> {
    let Some(s) = entry(zip, "xl/styles.xml") else { return Vec::new() };
    let mut fonts: Vec<bool> = Vec::new();
    let mut xf_font: Vec<usize> = Vec::new();
    let (mut in_fonts, mut in_font, mut in_xfs) = (false, false, false);
    let mut r = Reader::from_str(&s);
    loop {
        let ev = match r.read_event() {
            Ok(ev) => ev,
            Err(_) => break,
        };
        match ev {
            Event::Start(e) => match e.local_name().as_ref() {
                b"fonts" => in_fonts = true,
                b"font" if in_fonts => {
                    fonts.push(false);
                    in_font = true;
                }
                b"b" if in_font => *fonts.last_mut().unwrap() = bool_val(&e),
                b"cellXfs" => in_xfs = true,
                b"xf" if in_xfs => xf_font.push(attr(&e, b"fontId").and_then(|v| v.parse().ok()).unwrap_or(0)),
                _ => {}
            },
            Event::Empty(e) => match e.local_name().as_ref() {
                b"font" if in_fonts => fonts.push(false),
                b"b" if in_font => *fonts.last_mut().unwrap() = bool_val(&e),
                b"xf" if in_xfs => xf_font.push(attr(&e, b"fontId").and_then(|v| v.parse().ok()).unwrap_or(0)),
                _ => {}
            },
            Event::End(e) => match e.local_name().as_ref() {
                b"fonts" => in_fonts = false,
                b"font" => in_font = false,
                b"cellXfs" => in_xfs = false,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    xf_font.iter().map(|&f| fonts.get(f).copied().unwrap_or(false)).collect()
}

/// Parses the body of an `<si>` (shared string) or `<is>` (inline string).
#[derive(Default)]
struct RichParser {
    runs: Vec<RawRun>,
    in_r: bool,
    in_rpr: bool,
    in_rph: u32,
    in_t: bool,
    run_text: String,
    run_bold: Option<bool>,
}

impl RichParser {
    /// Feed one event. Returns the finished runs when `container` closes.
    fn feed(&mut self, ev: &Event, container: &[u8]) -> Option<Vec<RawRun>> {
        match ev {
            Event::Start(e) => match e.local_name().as_ref() {
                b"r" => {
                    self.in_r = true;
                    self.run_text.clear();
                    self.run_bold = None;
                }
                // A run with its own properties fully defines its font, so it
                // is bold only if <b/> appears inside them.
                b"rPr" if self.in_r => {
                    self.in_rpr = true;
                    self.run_bold = Some(false);
                }
                b"b" if self.in_rpr => self.run_bold = Some(bool_val(e)),
                b"rPh" => self.in_rph += 1,
                b"t" if self.in_rph == 0 => self.in_t = true,
                _ => {}
            },
            Event::Empty(e) => match e.local_name().as_ref() {
                b"rPr" if self.in_r => self.run_bold = Some(false),
                b"b" if self.in_rpr => self.run_bold = Some(bool_val(e)),
                _ => {}
            },
            Event::Text(t) if self.in_t => {
                let s = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
                self.push_text(s);
            }
            Event::CData(c) if self.in_t => {
                let s = String::from_utf8_lossy(c).into_owned();
                self.push_text(s);
            }
            Event::End(e) => match e.local_name().as_ref() {
                b"t" => self.in_t = false,
                b"rPr" => self.in_rpr = false,
                b"rPh" => self.in_rph = self.in_rph.saturating_sub(1),
                b"r" => {
                    self.in_r = false;
                    let text = std::mem::take(&mut self.run_text);
                    self.runs.push(RawRun { text, bold: self.run_bold });
                }
                n if n == container => return Some(std::mem::take(&mut self.runs)),
                _ => {}
            },
            _ => {}
        }
        None
    }

    fn push_text(&mut self, s: String) {
        if self.in_r {
            self.run_text.push_str(&s);
        } else {
            self.runs.push(RawRun { text: s, bold: None });
        }
    }
}

fn parse_shared_strings(s: &str) -> Result<Vec<Vec<RawRun>>, String> {
    let mut out = Vec::new();
    let mut r = Reader::from_str(s);
    let mut cur: Option<RichParser> = None;
    loop {
        let ev = r.read_event().map_err(xml_err)?;
        if let Event::Eof = ev {
            break;
        }
        if let Some(p) = cur.as_mut() {
            if let Some(runs) = p.feed(&ev, b"si") {
                out.push(runs);
                cur = None;
            }
            continue;
        }
        match &ev {
            Event::Start(e) if e.local_name().as_ref() == b"si" => cur = Some(RichParser::default()),
            Event::Empty(e) if e.local_name().as_ref() == b"si" => out.push(Vec::new()),
            _ => {}
        }
    }
    Ok(out)
}

/// "AB12" -> (col 28, row 12), both 1-based.
fn cell_ref(r: &str) -> Option<(u32, u32)> {
    let mut col = 0u32;
    let mut digits = String::new();
    for ch in r.chars() {
        if ch.is_ascii_alphabetic() && digits.is_empty() {
            col = col * 26 + (ch.to_ascii_uppercase() as u32 - 'A' as u32 + 1);
        } else if ch.is_ascii_digit() {
            digits.push(ch);
        } else if ch != '$' {
            return None;
        }
    }
    Some((col, digits.parse().ok()?))
}

struct Cell {
    row: u32,
    col: u32,
    runs: Vec<Run>,
}

fn parse_sheet(s: &str, shared: &[Vec<RawRun>], xf_bold: &[bool]) -> Result<Vec<Cell>, String> {
    let mut cells = Vec::new();
    let mut r = Reader::from_str(s);
    let (mut row, mut col) = (0u32, 0u32);
    let mut in_c = false;
    let mut in_v = false;
    let mut c_type = String::new();
    let mut c_bold = false;
    let mut v_text = String::new();
    let mut inline: Option<Vec<RawRun>> = None;
    let mut rich: Option<RichParser> = None;

    loop {
        let ev = r.read_event().map_err(xml_err)?;
        if let Event::Eof = ev {
            break;
        }
        if let Some(p) = rich.as_mut() {
            if let Some(runs) = p.feed(&ev, b"is") {
                inline = Some(runs);
                rich = None;
            }
            continue;
        }
        match &ev {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"row" => {
                row = attr(e, b"r").and_then(|v| v.parse().ok()).unwrap_or(row + 1);
                col = 0;
            }
            Event::Start(e) if e.local_name().as_ref() == b"c" => {
                match attr(e, b"r").as_deref().and_then(cell_ref) {
                    Some((c, rr)) => {
                        col = c;
                        row = rr;
                    }
                    None => col += 1,
                }
                in_c = true;
                c_type = attr(e, b"t").unwrap_or_default();
                let style: usize = attr(e, b"s").and_then(|v| v.parse().ok()).unwrap_or(0);
                c_bold = xf_bold.get(style).copied().unwrap_or(false);
                v_text.clear();
                inline = None;
            }
            Event::Empty(e) if e.local_name().as_ref() == b"c" => {
                col = attr(e, b"r").as_deref().and_then(cell_ref).map(|(c, _)| c).unwrap_or(col + 1);
            }
            Event::Start(e) if in_c && e.local_name().as_ref() == b"v" => in_v = true,
            Event::Start(e) if in_c && e.local_name().as_ref() == b"is" => rich = Some(RichParser::default()),
            Event::Text(t) if in_v => v_text.push_str(&t.unescape().map(|c| c.into_owned()).unwrap_or_default()),
            Event::End(e) => match e.local_name().as_ref() {
                b"v" => in_v = false,
                b"c" if in_c => {
                    in_c = false;
                    let raw: Vec<RawRun> = match c_type.as_str() {
                        "s" => v_text.trim().parse::<usize>().ok().and_then(|i| shared.get(i)).cloned().unwrap_or_default(),
                        "inlineStr" => inline.take().unwrap_or_default(),
                        "e" => Vec::new(),
                        "b" => vec![RawRun { text: if v_text.trim() == "1" { "TRUE" } else { "FALSE" }.into(), bold: None }],
                        _ => vec![RawRun { text: v_text.clone(), bold: None }],
                    };
                    let runs: Vec<Run> =
                        raw.into_iter().map(|r| Run { text: r.text, bold: r.bold.unwrap_or(c_bold) }).collect();
                    let runs = clean(runs);
                    if !runs.is_empty() {
                        cells.push(Cell { row, col, runs });
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok(cells)
}

/// Normalise whitespace (newlines, tabs, doubled spaces), trim, merge runs.
fn clean(runs: Vec<Run>) -> Vec<Run> {
    let mut chars: Vec<(char, bool)> = Vec::new();
    for r in runs {
        let t = r.text.replace("_x000D_", "").replace("_x000A_", " ").replace("_x0009_", " ");
        for ch in t.chars() {
            let ch = if ch.is_whitespace() || ch.is_control() { ' ' } else { ch };
            if ch == ' ' && chars.last().map_or(true, |&(c, _)| c == ' ') {
                continue;
            }
            chars.push((ch, r.bold));
        }
    }
    while chars.last().map_or(false, |&(c, _)| c == ' ') {
        chars.pop();
    }
    let mut out: Vec<Run> = Vec::new();
    for (ch, bold) in chars {
        match out.last_mut() {
            Some(last) if last.bold == bold => last.text.push(ch),
            _ => out.push(Run { text: ch.to_string(), bold }),
        }
    }
    out
}

fn plain(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

fn build_products(cells: Vec<Cell>, marking: Marking) -> Vec<Product> {
    let Some(header_row) = cells.iter().map(|c| c.row).min() else { return Vec::new() };
    let mut cols: BTreeMap<u32, Product> = BTreeMap::new();
    for c in cells.iter().filter(|c| c.row == header_row) {
        cols.insert(c.col, Product { name: plain(&c.runs), ..Default::default() });
    }
    let mut body: Vec<&Cell> = cells.iter().filter(|c| c.row > header_row).collect();
    body.sort_by_key(|c| (c.col, c.row));
    let mut store_at: Option<(u32, u32)> = None; // (col, row) of the cell under a barcode
    for c in body {
        if let Some(p) = cols.get_mut(&c.col) {
            let text = plain(&c.runs);
            if store_at == Some((c.col, c.row)) {
                p.store = Some(strip_store_label(&text));
                continue;
            }
            if let Some(bc) = parse_barcode(&text) {
                if p.barcode.is_none() && !bc.is_empty() {
                    p.barcode = Some(bc);
                    store_at = Some((c.col, c.row + 1));
                }
                continue;
            }
            match parse_shelf_life(&text) {
                Some(days) => {
                    if p.shelf_life_days.is_none() {
                        p.shelf_life_days = days;
                    }
                }
                None => p.ingredients.push(match marking {
                    Marking::Brackets => bracket_runs(&c.runs),
                    Marking::Bold => c.runs.clone(),
                }),
            }
        }
    }
    let mut out: Vec<Product> = cols.into_values().collect();
    for p in out.iter_mut().filter(|p| p.barcode.is_some()) {
        p.ingredients.clear();
        p.needs_check = true;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelf_life() {
        assert_eq!(parse_shelf_life("Shelf life: 3 days"), Some(Some(3)));
        assert_eq!(parse_shelf_life("shelf-life 10"), Some(Some(10)));
        assert_eq!(parse_shelf_life("Shelf life: tbc"), Some(None));
        assert_eq!(parse_shelf_life("Wheat flour"), None);
    }

    #[test]
    fn brackets() {
        let r = |t: &str, b: bool| Run { text: t.into(), bold: b };
        let out = bracket_runs(&[r("Butter (milk)", true)]);
        assert_eq!(out, vec![r("Butter (", false), r("milk", true), r(")", false)]);
        let out = bracket_runs(&[r("Chips (milk, soya (lecithin))", false)]);
        assert_eq!(out, vec![r("Chips (", false), r("milk, soya ", true), r("(", false), r("lecithin", true), r("))", false)]);
        assert_eq!(bracket_runs(&[r("Sugar", true)]), vec![r("Sugar", false)]);
    }

    #[test]
    fn barcodes() {
        assert_eq!(parse_barcode("Barcode: 5010251703129").as_deref(), Some("5010251703129"));
        assert_eq!(parse_barcode("barcode 5010 2517 03129").as_deref(), Some("5010251703129"));
        assert_eq!(parse_barcode("5010251703129").as_deref(), Some("5010251703129"));
        assert_eq!(parse_barcode("5.010251703129E+12").as_deref(), Some("5010251703129"));
        assert_eq!(parse_barcode("Sugar"), None);
        assert_eq!(parse_barcode("250"), None);
        assert_eq!(strip_store_label("Store: Tesco"), "Tesco");
        assert_eq!(strip_store_label("Sainsbury's"), "Sainsbury's");
    }

    #[test]
    fn refs() {
        assert_eq!(cell_ref("A1"), Some((1, 1)));
        assert_eq!(cell_ref("AB12"), Some((28, 12)));
    }

    #[test]
    fn sample_sheet() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../out/Products.xlsx");
        if !p.exists() {
            return;
        }
        let products = read_products(&p, Marking::Brackets).unwrap();
        for pr in &products {
            println!("== {}", pr.name);
            for ing in &pr.ingredients {
                let s: String =
                    ing.iter().map(|r| if r.bold { format!("**{}**", r.text) } else { r.text.clone() }).collect();
                println!("   {s}");
            }
        }
        assert_eq!(products.len(), 7);
        let bought = &products[6];
        assert_eq!(bought.barcode.as_deref(), Some("5010251703129"));
        assert_eq!(bought.store.as_deref(), Some("Morrisons"));
        assert!(bought.ingredients.is_empty());
        assert_eq!(products[0].shelf_life_days, Some(2));
        assert_eq!(products[4].shelf_life_days, Some(7));
        assert_eq!(products[1].name, "Chocolate Brownie");
        // "Flour (wheat)": only "wheat" bold
        let flour = &products[0].ingredients[0];
        assert!(flour.iter().any(|r| r.bold && r.text == "wheat"));
        assert!(flour.iter().any(|r| !r.bold && r.text.starts_with("Flour")));
        assert!(products[0].ingredients[2].iter().all(|r| !r.bold)); // Caster sugar
        let chips = &products[1].ingredients[6];
        assert!(chips.iter().any(|r| r.bold && r.text == "milk, soya"));
    }
}

// ---------------------------------------------------------------------------
// Writing: append a product column to the first sheet, leaving everything else
// in the workbook untouched.

fn col_letters(mut col: u32) -> String {
    let mut s = Vec::new();
    while col > 0 {
        let r = (col - 1) % 26;
        s.push((b'A' + r as u8) as char);
        col = (col - 1) / 26;
    }
    s.iter().rev().collect()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Finds `attr="value"` inside a tag's text.
fn tag_attr(tag: &str, name: &str) -> Option<String> {
    let pat = format!(" {name}=\"");
    let i = tag.find(&pat)? + pat.len();
    let j = tag[i..].find('"')? + i;
    Some(tag[i..j].to_string())
}

/// Byte ranges of each `<row …>…</row>` (or `<row …/>`) in sheetData, with its row number.
fn find_rows(xml: &str) -> Vec<(u32, usize, usize)> {
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(i) = xml[pos..].find("<row").map(|i| i + pos) {
        let after = xml.as_bytes().get(i + 4).copied();
        if !matches!(after, Some(b' ') | Some(b'>') | Some(b'/')) {
            pos = i + 4;
            continue;
        }
        let Some(tag_end) = xml[i..].find('>').map(|j| j + i) else { break };
        let tag = &xml[i..=tag_end];
        let end = if tag.ends_with("/>") {
            tag_end + 1
        } else {
            match xml[tag_end..].find("</row>") {
                Some(j) => tag_end + j + "</row>".len(),
                None => break,
            }
        };
        let r = tag_attr(tag, "r").and_then(|v| v.parse().ok()).unwrap_or(out.last().map_or(1, |l: &(u32, usize, usize)| l.0 + 1));
        out.push((r, i, end));
        pos = end;
    }
    out
}

/// Cells inside a row's text: (column, start, end, style).
fn find_cells(row: &str) -> Vec<(u32, usize, usize, Option<String>)> {
    let mut out = Vec::new();
    let mut pos = 0;
    let mut col = 0;
    while let Some(i) = row[pos..].find("<c").map(|i| i + pos) {
        if !matches!(row.as_bytes().get(i + 2), Some(b' ') | Some(b'>') | Some(b'/')) {
            pos = i + 2;
            continue;
        }
        let Some(tag_end) = row[i..].find('>').map(|j| j + i) else { break };
        let tag = &row[i..=tag_end];
        let end = if tag.ends_with("/>") {
            tag_end + 1
        } else {
            match row[tag_end..].find("</c>") {
                Some(j) => tag_end + j + 4,
                None => break,
            }
        };
        col = tag_attr(tag, "r").as_deref().and_then(cell_ref).map(|(c, _)| c).unwrap_or(col + 1);
        out.push((col, i, end, tag_attr(tag, "s")));
        pos = end;
    }
    out
}

/// Adds `values` (top to bottom, starting at the header row) as a new column
/// after the last used one. Each new cell copies the style of the cell to its
/// left. The original is kept as "<name> (backup).xlsx".
pub fn append_product_column(path: &Path, values: &[String]) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("can't open {}: {e}", path.display()))?;
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes.clone())).map_err(|_| "this doesn't look like an .xlsx file".to_string())?;
    let sheet_path = first_sheet_path(&mut zip)?;
    let xf_bold = bold_by_style(&mut zip);
    let shared = match entry(&mut zip, "xl/sharedStrings.xml") {
        Some(s) => parse_shared_strings(&s)?,
        None => Vec::new(),
    };
    let mut xml = entry(&mut zip, &sheet_path).ok_or("worksheet is missing")?;
    let cells = parse_sheet(&xml, &shared, &xf_bold)?;
    let header_row = cells.iter().map(|c| c.row).min().unwrap_or(1);
    let new_col = cells.iter().map(|c| c.col).max().unwrap_or(0) + 1;
    let letters = col_letters(new_col);

    if !xml.contains("<sheetData") {
        return Err("the first sheet has an unusual layout, so the app can't add to it".into());
    }
    if let Some(i) = xml.find("<sheetData/>") {
        xml.replace_range(i..i + "<sheetData/>".len(), "<sheetData></sheetData>");
    }

    for (k, value) in values.iter().enumerate() {
        if value.is_empty() {
            continue;
        }
        let row_no = header_row + k as u32;
        let rows = find_rows(&xml);
        let new_cell = |style: Option<String>| {
            format!(
                "<c r=\"{letters}{row_no}\"{} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
                style.map(|s| format!(" s=\"{s}\"")).unwrap_or_default(),
                xml_escape(value)
            )
        };
        match rows.iter().find(|r| r.0 == row_no) {
            Some(&(_, start, end)) => {
                let row = xml[start..end].to_string();
                let tag_end = row.find('>').unwrap();
                // Drop the "spans" hint; it would no longer be accurate.
                let mut open = row[..=tag_end].to_string();
                if let Some(sp) = tag_attr(&open, "spans") {
                    open = open.replace(&format!(" spans=\"{sp}\""), "");
                }
                let self_closing = open.ends_with("/>");
                if self_closing {
                    open = format!("{}>", open.trim_end_matches("/>"));
                }
                let body = if self_closing { String::new() } else { row[tag_end + 1..row.len() - "</row>".len()].to_string() };
                let found = find_cells(&body);
                let style = found.iter().filter(|c| c.0 < new_col).last().and_then(|c| c.3.clone());
                let cell = new_cell(style);
                // Keep cells in column order; replace an empty formatted cell already in this column.
                let new_body = match found.iter().find(|c| c.0 >= new_col) {
                    Some(c) if c.0 == new_col => format!("{}{}{}", &body[..c.1], cell, &body[c.2..]),
                    Some(c) => format!("{}{}{}", &body[..c.1], cell, &body[c.1..]),
                    None => format!("{body}{cell}"),
                };
                xml.replace_range(start..end, &format!("{open}{new_body}</row>"));
            }
            None => {
                let style = None;
                let row = format!("<row r=\"{row_no}\">{}</row>", new_cell(style));
                let at = rows.iter().find(|r| r.0 > row_no).map(|r| r.1).unwrap_or_else(|| xml.find("</sheetData>").unwrap());
                xml.insert_str(at, &row);
            }
        }
    }

    // Widen the recorded used range if needed.
    if let Some(i) = xml.find("<dimension ref=\"") {
        let s = i + "<dimension ref=\"".len();
        if let Some(e) = xml[s..].find('"').map(|e| e + s) {
            let r = xml[s..e].to_string();
            let last = r.split(':').last().unwrap_or("A1");
            if let Some((c, rw)) = cell_ref(last) {
                let first = r.split(':').next().unwrap_or("A1");
                let rw = rw.max(header_row + values.len() as u32 - 1);
                let new_ref = format!("{first}:{}{rw}", col_letters(c.max(new_col)));
                xml.replace_range(s..e, &new_ref);
            }
        }
    }

    // Rebuild the zip, copying every other part unchanged.
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let f = zip.by_index_raw(i).map_err(|e| e.to_string())?;
        if f.name() == sheet_path {
            drop(f);
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            out.start_file(sheet_path.as_str(), opts).map_err(|e| e.to_string())?;
            std::io::Write::write_all(&mut out, xml.as_bytes()).map_err(|e| e.to_string())?;
        } else {
            out.raw_copy_file(f).map_err(|e| e.to_string())?;
        }
    }
    let new_bytes = out.finish().map_err(|e| e.to_string())?.into_inner();

    // Sanity check before touching the real file.
    let check = read_products_from_bytes(new_bytes.clone(), Marking::Bold)?;
    if !check.iter().any(|p| p.name == values[0]) {
        return Err("the new column didn't read back correctly, so the spreadsheet was left unchanged".into());
    }

    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Products".into());
    let backup = path.with_file_name(format!("{stem} (backup).xlsx"));
    let _ = std::fs::write(&backup, &bytes);
    let tmp = path.with_file_name(format!("~{stem}.tmp"));
    std::fs::write(&tmp, &new_bytes).map_err(|e| format!("couldn't write next to the spreadsheet: {e}"))?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "couldn't save the spreadsheet ({e}). If it's open in Excel, close it and try again"
        ));
    }
    Ok(())
}

fn read_products_from_bytes(bytes: Vec<u8>, marking: Marking) -> Result<Vec<Product>, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let sheet_path = first_sheet_path(&mut zip)?;
    let xf_bold = bold_by_style(&mut zip);
    let shared = match entry(&mut zip, "xl/sharedStrings.xml") {
        Some(s) => parse_shared_strings(&s)?,
        None => Vec::new(),
    };
    let sheet = entry(&mut zip, &sheet_path).ok_or("worksheet is missing")?;
    Ok(build_products(parse_sheet(&sheet, &shared, &xf_bold)?, marking))
}

#[cfg(test)]
mod write_tests {
    use super::*;

    #[test]
    fn appends_a_column() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/Products.xlsx");
        let dir = std::env::temp_dir().join("allergen_append_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("Products.xlsx");
        std::fs::copy(&src, &p).unwrap();
        let vals: Vec<String> =
            ["Oat & Raisin <Cookies>", "Shelf life: 7 days", "Barcode: 5000168001234", "Tesco"].iter().map(|s| s.to_string()).collect();
        append_product_column(&p, &vals).unwrap();
        let ps = read_products(&p, Marking::Brackets).unwrap();
        assert_eq!(ps.len(), 8);
        let last = ps.last().unwrap();
        assert_eq!(last.name, "Oat & Raisin <Cookies>");
        assert_eq!(last.barcode.as_deref(), Some("5000168001234"));
        assert_eq!(last.store.as_deref(), Some("Tesco"));
        assert_eq!(last.shelf_life_days, Some(7));
        assert!(dir.join("Products (backup).xlsx").exists());
        // Earlier products untouched.
        assert_eq!(ps[0], read_products(&src, Marking::Brackets).unwrap()[0]);
        println!("{}", p.display());
    }
}
