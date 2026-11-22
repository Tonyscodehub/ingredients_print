//! The public allergen page on the website (e.g. poffles.co.uk/allergens/shed):
//! builds the data it shows from the products, so the web server can send it
//! whenever the spreadsheet changes.

use crate::config::Config;
use crate::lookup;
use crate::xlsx::Product;
use serde::Serialize;

/// The UK's 14 allergens: key (as used by Open Food Facts) and the name shown.
pub const ALLERGENS: [(&str, &str); 14] = [
    ("en:gluten", "Gluten"),
    ("en:milk", "Milk"),
    ("en:eggs", "Eggs"),
    ("en:nuts", "Tree nuts"),
    ("en:peanuts", "Peanuts"),
    ("en:soybeans", "Soya"),
    ("en:sesame-seeds", "Sesame"),
    ("en:celery", "Celery"),
    ("en:mustard", "Mustard"),
    ("en:fish", "Fish"),
    ("en:crustaceans", "Crustaceans"),
    ("en:molluscs", "Molluscs"),
    ("en:lupin", "Lupin"),
    ("en:sulphur-dioxide-and-sulphites", "Sulphites"),
];

/// Short key for the page ("gluten", "sesame-seeds"…).
fn short(tag: &str) -> &str {
    tag.trim_start_matches("en:")
}

/// Which of the 14 allergens a piece of text names (e.g. "wheat flour" → gluten).
pub fn allergens_in(text: &str) -> Vec<&'static str> {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphabetic()).filter(|w| !w.is_empty()).collect();
    let mut found = Vec::new();
    for (tag, _) in ALLERGENS {
        let kws = lookup::keywords(tag);
        let hit = words.iter().enumerate().any(|(i, w)| {
            let prev = if i > 0 { words[i - 1] } else { "" };
            kws.contains(w) && !lookup::false_friend(prev, w)
        });
        // Deliberately cautious: anything marked as an allergen that names one counts.
        if hit {
            found.push(short(tag));
        }
    }
    found
}

#[derive(Serialize, Debug, PartialEq)]
pub struct SiteRun {
    /// Text.
    pub t: String,
    /// Allergen (printed bold).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub b: bool,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct SiteProduct {
    pub name: String,
    /// One entry per ingredient, each a list of runs.
    pub ingredients: Vec<Vec<SiteRun>>,
    pub may_contain: String,
    /// Allergen keys from the bold (allergen) text.
    pub contains: Vec<&'static str>,
    /// Allergen keys from the "may contain" text that aren't already in `contains`.
    pub may: Vec<&'static str>,
    /// Bought-in product whose details haven't been checked against the pack yet.
    pub pending: bool,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct SiteAllergen {
    pub key: &'static str,
    pub name: &'static str,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct SiteData {
    pub format: u32,
    pub products: Vec<SiteProduct>,
    pub allergens: Vec<SiteAllergen>,
    pub warning: String,
    pub business_name: String,
    pub address: String,
    pub contact: String,
}

pub fn site_data(products: &[Product], cfg: &Config) -> SiteData {
    let mut list: Vec<SiteProduct> = products
        .iter()
        .map(|p| {
            if p.needs_check {
                return SiteProduct {
                    name: p.name.clone(),
                    ingredients: Vec::new(),
                    may_contain: String::new(),
                    contains: Vec::new(),
                    may: Vec::new(),
                    pending: true,
                };
            }
            let ingredients: Vec<Vec<SiteRun>> = p
                .ingredients
                .iter()
                .map(|cell| cell.iter().filter(|r| !r.text.is_empty()).map(|r| SiteRun { t: r.text.clone(), b: r.bold }).collect::<Vec<_>>())
                .filter(|c: &Vec<SiteRun>| c.iter().any(|r| !r.t.trim().is_empty()))
                .collect();
            let bold: String = p.ingredients.iter().flatten().filter(|r| r.bold).map(|r| r.text.as_str()).collect::<Vec<_>>().join(" , ");
            let contains = allergens_in(&bold);
            let may_contain = p.may_contain.clone().unwrap_or_default().trim().to_string();
            let may = allergens_in(&may_contain).into_iter().filter(|k| !contains.contains(k)).collect();
            SiteProduct { name: p.name.clone(), ingredients, may_contain, contains, may, pending: false }
        })
        .collect();
    list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    SiteData {
        format: 1,
        products: list,
        allergens: ALLERGENS.iter().map(|(t, n)| SiteAllergen { key: short(t), name: n }).collect(),
        warning: cfg.warning.clone(),
        business_name: cfg.business_name.clone(),
        address: cfg.address.clone(),
        contact: cfg.contact.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_allergens() {
        assert_eq!(allergens_in("wheat flour, butter (milk)"), vec!["gluten", "milk"]);
        assert_eq!(allergens_in("cocoa butter, coconut milk"), Vec::<&str>::new());
        assert_eq!(allergens_in("Eggs"), vec!["eggs"]);
        assert_eq!(allergens_in("nuts, sesame and soya"), vec!["nuts", "soybeans", "sesame-seeds"]);
        assert_eq!(allergens_in("gluten-free oats"), vec!["gluten"]);
    }
}
