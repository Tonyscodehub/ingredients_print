#![cfg_attr(windows, windows_subsystem = "windows")]

#[allow(unused_imports)]
pub(crate) use allergen_labels::{config, content, date, helper, lookup, xlsx};

#[cfg(windows)]
mod http;
#[cfg(windows)]
mod label;
#[cfg(windows)]
mod ui;

#[cfg(windows)]
fn main() {
    ui::run();
}

/// Non-Windows builds are only for testing the spreadsheet reader.
#[cfg(not(windows))]
fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| config::Config::default().spreadsheet);
    match xlsx::read_products(std::path::Path::new(&path), xlsx::Marking::Brackets) {
        Ok(ps) => {
            for p in ps {
                println!("{}", p.name);
                for i in p.ingredients {
                    let s: String =
                        i.iter().map(|r| if r.bold { format!("**{}**", r.text) } else { r.text.clone() }).collect();
                    println!("  {s}");
                }
            }
        }
        Err(e) => eprintln!("error: {e}"),
    }
}
