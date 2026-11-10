//! What goes on a label, independent of how it's drawn. Used by the Windows
//! printer/preview (GDI) and by the web app's preview, so both say the same thing.

use crate::config::Config;
use crate::xlsx::{Product, Run};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    Title,
    Body,
    Bold,
    Footer,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Para {
    pub runs: Vec<Span>,
    pub center: bool,
    /// Font whose line height is used for empty space and the minimum line height.
    pub base: Style,
    /// Space after the paragraph, in body-text lines.
    pub gap_after: f32,
}

fn sp(text: impl Into<String>, style: Style) -> Span {
    Span { text: text.into(), style }
}

/// "Best before: 11 Nov 2026" – `today` is (year, month, day) where the label is printed.
pub fn best_before_text(cfg: &Config, product: &Product, today: (i32, u32, u32)) -> Option<String> {
    let days = product.shelf_life_days.unwrap_or(cfg.best_before_days);
    if days == 0 {
        return None;
    }
    let d = crate::date::add_days_formatted(today.0, today.1, today.2, days as i64);
    Some(format!("Best before: {d}"))
}

pub fn paragraphs(product: &Product, cfg: &Config, today: (i32, u32, u32)) -> Vec<Para> {
    let mut paras: Vec<Para> = Vec::new();
    let push = |paras: &mut Vec<Para>, runs: Vec<Span>, center: bool, base: Style, gap_before: bool| {
        if gap_before {
            if let Some(last) = paras.last_mut() {
                last.gap_after = 0.35;
            }
        }
        paras.push(Para { runs, center, base, gap_after: 0.0 });
    };
    push(&mut paras, vec![sp(product.name.clone(), Style::Title)], true, Style::Title, false);
    if let Some(p) = paras.last_mut() {
        p.gap_after = 0.35;
    }

    let ing = |cell: &Vec<Run>| -> Vec<Span> {
        cell.iter().map(|r| sp(r.text.clone(), if r.bold { Style::Bold } else { Style::Body })).collect()
    };
    if product.ingredients.is_empty() {
        push(&mut paras, vec![sp("No ingredients listed.", Style::Body)], false, Style::Body, false);
    } else if cfg.one_per_line {
        push(&mut paras, vec![sp("Ingredients:", Style::Bold)], false, Style::Body, false);
        for cell in &product.ingredients {
            push(&mut paras, ing(cell), false, Style::Body, false);
        }
    } else {
        let mut runs = vec![sp("Ingredients: ", Style::Bold)];
        let n = product.ingredients.len();
        for (i, cell) in product.ingredients.iter().enumerate() {
            runs.extend(ing(cell));
            runs.push(sp(if i + 1 < n { ", " } else { "." }, Style::Body));
        }
        push(&mut paras, runs, false, Style::Body, false);
    }
    if let Some(m) = product.may_contain.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
        push(
            &mut paras,
            vec![sp("May contain: ", Style::Body), sp(m.trim_end_matches('.').to_string() + ".", Style::Bold)],
            false,
            Style::Body,
            false,
        );
    }
    if !cfg.footer.trim().is_empty() {
        push(&mut paras, vec![sp(cfg.footer.trim(), Style::Footer)], false, Style::Footer, true);
    }
    if let Some(bb) = best_before_text(cfg, product, today) {
        push(&mut paras, vec![sp(bb, Style::Bold)], false, Style::Body, true);
    }
    // Business block, centred: name (bold), address, contact.
    let business = [(cfg.business_name.trim(), Style::Bold), (cfg.address.trim(), Style::Body), (cfg.contact.trim(), Style::Body)];
    let mut first = true;
    for (text, style) in business {
        if text.is_empty() {
            continue;
        }
        push(&mut paras, vec![sp(text, style)], true, style, first);
        first = false;
    }
    if !cfg.warning.trim().is_empty() {
        push(&mut paras, vec![sp(cfg.warning.trim(), Style::Footer)], false, Style::Footer, true);
    }
    paras
}
