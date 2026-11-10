//! Bought-in products: barcode lookups on Open Food Facts, and the store of
//! lookups that someone has checked against the pack and accepted.
//!
//! Ingredient text uses *asterisks* around allergens (bold on the label).
//! The shop and the manufacturer/brand are kept for checking only and are
//! never printed.

use crate::xlsx::Run;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const USER_AGENT: &str = "AllergenLabels/1.2 (https://github.com/Tonyscodehub/ingredients_print)";
pub const OFF_HOST: &str = "world.openfoodfacts.org";

pub fn off_path(barcode: &str) -> String {
    format!(
        "/api/v2/product/{barcode}.json?fields=product_name,product_name_en,brands,ingredients_text,ingredients_text_en,allergens_tags,traces_tags"
    )
}

/// What Open Food Facts returned for a barcode.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Found {
    pub name: String,
    pub brand: String,
    /// Ingredients with *allergens* marked.
    pub ingredients: String,
    pub may_contain: String,
}

/// A lookup someone has checked against the pack and accepted.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Checked {
    pub store: String,
    /// Product name/brand found online, kept only to show on the checking screen.
    pub found_name: String,
    pub ingredients: String,
    pub may_contain: String,
    pub checked_on: String,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Store {
    pub products: BTreeMap<String, Checked>,
}

fn store_path() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(crate::config::exe_dir);
    base.join("AllergenLabels").join("checked_products.json")
}

impl Store {
    pub fn load() -> Store {
        Self::load_from(&store_path())
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_to(&store_path())
    }

    pub fn load_from(p: &std::path::Path) -> Store {
        std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save_to(&self, p: &std::path::Path) -> Result<(), String> {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let s = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(p, s).map_err(|e| format!("couldn't save to {}: {e}", p.display()))
    }
}

/// Parses an Open Food Facts v2 product response. Ok(None) = barcode not in the database.
pub fn parse_off(json: &str) -> Result<Option<Found>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| "Open Food Facts sent an unreadable reply".to_string())?;
    if v.get("status").and_then(|s| s.as_i64()) != Some(1) {
        return Ok(None);
    }
    let p = &v["product"];
    let s = |k: &str| p.get(k).and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    let tags = |k: &str| -> Vec<String> {
        p.get(k)
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect())
            .unwrap_or_default()
    };
    let name = Some(s("product_name_en")).filter(|x| !x.is_empty()).unwrap_or_else(|| s("product_name"));
    let raw = Some(s("ingredients_text_en")).filter(|x| !x.is_empty()).unwrap_or_else(|| s("ingredients_text"));
    let allergens = tags("allergens_tags");
    let traces: Vec<String> = tags("traces_tags").iter().map(|t| tag_name(t)).collect();
    Ok(Some(Found { name, brand: s("brands"), ingredients: mark_allergens(&raw, &allergens), may_contain: traces.join(", ") }))
}

/// "en:sesame-seeds" -> "sesame seeds"
fn tag_name(t: &str) -> String {
    t.split_once(':').map(|(_, n)| n).unwrap_or(t).replace('-', " ")
}

/// Words to bold for each Open Food Facts allergen tag (UK's 14 allergens).
fn keywords(tag: &str) -> &'static [&'static str] {
    match tag {
        "en:gluten" => &["wheat", "barley", "rye", "oat", "oats", "spelt", "kamut", "gluten"],
        "en:milk" => &["milk", "butter", "buttermilk", "cream", "cheese", "whey", "lactose", "yoghurt", "yogurt", "casein"],
        "en:eggs" => &["egg", "eggs"],
        "en:nuts" => &[
            "nut", "nuts", "almond", "almonds", "hazelnut", "hazelnuts", "walnut", "walnuts", "cashew", "cashews", "pecan",
            "pecans", "pistachio", "pistachios", "macadamia",
        ],
        "en:peanuts" => &["peanut", "peanuts"],
        "en:soybeans" => &["soya", "soy", "soybean", "soybeans"],
        "en:sesame-seeds" => &["sesame"],
        "en:celery" => &["celery", "celeriac"],
        "en:mustard" => &["mustard"],
        "en:fish" => &["fish"],
        "en:crustaceans" => &["crustacean", "crustaceans", "prawn", "prawns", "shrimp", "crab", "lobster"],
        "en:molluscs" => &["mollusc", "molluscs", "mussel", "mussels", "oyster", "oysters", "squid"],
        "en:lupin" => &["lupin"],
        "en:sulphur-dioxide-and-sulphites" => &["sulphite", "sulphites", "metabisulphite", "sulphur"],
        _ => &[],
    }
}

/// Words that look like an allergen but aren't after these (cocoa butter, coconut milk…).
fn false_friend(prev: &str, word: &str) -> bool {
    match word {
        "butter" => matches!(prev, "cocoa" | "shea" | "peanut" | "nut" | "almond" | "cashew"),
        "milk" => matches!(prev, "coconut" | "oat" | "almond" | "soya" | "soy" | "rice"),
        "cream" => matches!(prev, "coconut" | "soya" | "soy" | "oat"),
        "nut" | "nuts" => matches!(prev, "coconut" | "nutmeg"),
        _ => false,
    }
}

/// Turns Open Food Facts ingredient text into *marked* text: its own `_allergen_`
/// markers become asterisks, and allergen words for the product's allergen tags
/// are marked too.
pub fn mark_allergens(raw: &str, allergen_tags: &[String]) -> String {
    let mut t = raw.trim().to_string();
    for prefix in ["ingredients:", "ingredients :", "ingredients"] {
        if t.to_lowercase().starts_with(prefix) {
            t = t[prefix.len()..].trim_start().to_string();
            break;
        }
    }
    let mut t = t.trim_end_matches(['.', ' ']).replace('_', "*").replace('\n', " ");
    if t.matches('*').count() % 2 == 1 {
        t = t.replace('*', "");
    }
    let words: Vec<&str> = allergen_tags.iter().flat_map(|tag| keywords(tag).iter().copied()).collect();
    if words.is_empty() {
        return tidy(&t);
    }
    // Walk the text, marking whole words outside existing *…* spans.
    let mut out = String::new();
    let mut inside = false;
    let mut prev_word = String::new();
    let chars: Vec<char> = t.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '*' {
            inside = !inside;
            out.push(c);
            i += 1;
            continue;
        }
        if c.is_alphabetic() {
            let start = i;
            while i < chars.len() && (chars[i].is_alphabetic() || chars[i] == '\'') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let lower = word.to_lowercase();
            if !inside && words.contains(&lower.as_str()) && !false_friend(&prev_word, &lower) {
                out.push('*');
                out.push_str(&word);
                out.push('*');
            } else {
                out.push_str(&word);
            }
            prev_word = lower;
            continue;
        }
        out.push(c);
        i += 1;
    }
    tidy(&out)
}

/// Merges adjacent marks ("*wheat* *flour*" stays, "**" disappears) and squeezes spaces.
fn tidy(s: &str) -> String {
    let s = s.replace("**", "");
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// *marked* text -> label runs.
pub fn marked_to_runs(s: &str) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for (i, part) in s.split('*').enumerate() {
        if part.is_empty() {
            continue;
        }
        let bold = i % 2 == 1;
        match out.last_mut() {
            Some(l) if l.bold == bold => l.text.push_str(part),
            _ => out.push(Run { text: part.to_string(), bold }),
        }
    }
    out
}

pub fn marks_balanced(s: &str) -> bool {
    s.matches('*').count() % 2 == 0
}

/// Tidies checked ingredients: one per line is fine (lines are joined with
/// commas), spaces are squeezed, and a trailing full stop is dropped.
pub fn normalise_ingredients(text: &str) -> Result<String, String> {
    let mut ing = String::new();
    for line in text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")) {
        let line = line.trim().trim_end_matches('.').to_string();
        if line.is_empty() {
            continue;
        }
        if !ing.is_empty() {
            ing.push_str(if ing.ends_with(',') { " " } else { ", " });
        }
        ing.push_str(&line);
    }
    if ing.is_empty() {
        return Err("Type the ingredients from the pack first.".into());
    }
    if !marks_balanced(&ing) {
        return Err("There's an odd number of * marks in the ingredients. Each allergen needs one before and one after, like *milk*.".into());
    }
    Ok(ing)
}

/// Fills in checked details for bought-in products. Returns how many still need checking.
pub fn apply_checked(ps: &mut [crate::xlsx::Product], store: &Store) -> usize {
    let mut unchecked = 0;
    for p in ps.iter_mut() {
        let Some(bc) = p.barcode.clone() else { continue };
        match store.products.get(&bc) {
            Some(c) if !c.ingredients.trim().is_empty() => {
                p.ingredients = vec![marked_to_runs(&c.ingredients)];
                p.may_contain = Some(c.may_contain.clone()).filter(|m| !m.trim().is_empty());
                p.needs_check = false;
            }
            _ => unchecked += 1,
        }
    }
    unchecked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_off_reply() {
        let json = r#"{"code":"5010251703129","product":{"product_name":"All Butter Shortbread Fingers","brands":"Morrisons",
            "ingredients_text_en":"Wheat Flour (_Wheat_ Flour, Calcium Carbonate, Iron, Niacin, Thiamin), _Butter_ (_Milk_) (32%), Sugar, Salt.",
            "allergens_tags":["en:gluten","en:milk"],"traces_tags":["en:nuts","en:sesame-seeds"]},
            "status":1,"status_verbose":"product found"}"#;
        let f = parse_off(json).unwrap().unwrap();
        assert_eq!(f.name, "All Butter Shortbread Fingers");
        assert_eq!(f.brand, "Morrisons");
        assert_eq!(f.ingredients, "*Wheat* Flour (*Wheat* Flour, Calcium Carbonate, Iron, Niacin, Thiamin), *Butter* (*Milk*) (32%), Sugar, Salt");
        assert_eq!(f.may_contain, "nuts, sesame seeds");
    }

    #[test]
    fn not_found() {
        assert_eq!(parse_off(r#"{"code":"123","status":0,"status_verbose":"product not found"}"#).unwrap(), None);
    }

    #[test]
    fn marks_words_without_underscores() {
        let tags = vec!["en:milk".to_string(), "en:soybeans".to_string(), "en:gluten".to_string()];
        let m = mark_allergens("INGREDIENTS: Sugar, cocoa butter, whole milk powder, emulsifier (soya lecithin), oats.", &tags);
        assert_eq!(m, "Sugar, cocoa butter, whole *milk* powder, emulsifier (*soya* lecithin), *oats*");
        let m = mark_allergens("Coconut milk, butter", &tags);
        assert_eq!(m, "Coconut milk, *butter*");
    }

    #[test]
    fn runs() {
        let r = marked_to_runs("Flour (*wheat*), *butter*");
        assert_eq!(r.len(), 4);
        assert!(r[1].bold && r[1].text == "wheat");
        assert!(!r[2].bold && r[2].text == "), ");
    }
}
