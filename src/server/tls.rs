//! HTTPS, set up the same way as Blockplan: a certificate from the certs
//! folder if there is one, otherwise a self-signed one made there on first run.
//!
//! HTTPS = self-signed (default) | auto | on | off

use axum_server::tls_rustls::RustlsConfig;
use std::path::{Path, PathBuf};

fn find_pair(dir: &Path) -> Option<(PathBuf, PathBuf)> {
    if let (Ok(c), Ok(k)) = (std::env::var("TLS_CERT"), std::env::var("TLS_KEY")) {
        return Some((c.into(), k.into()));
    }
    for (c, k) in [("cert.pem", "key.pem"), ("fullchain.pem", "privkey.pem")] {
        if dir.join(c).exists() && dir.join(k).exists() {
            return Some((dir.join(c), dir.join(k)));
        }
    }
    // name.crt / name.cer with name.key
    let entries = std::fs::read_dir(dir).ok()?;
    for e in entries.flatten() {
        let p = e.path();
        let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
        if ext == "crt" || ext == "cer" {
            let key = p.with_extension("key");
            if key.exists() {
                return Some((p, key));
            }
        }
    }
    None
}

fn hostnames() -> Vec<String> {
    let mut names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    if let Ok(h) = std::fs::read_to_string("/etc/hostname") {
        let h = h.trim();
        if !h.is_empty() {
            names.push(h.to_string());
        }
    }
    if let Ok(extra) = std::env::var("HTTPS_HOSTNAMES") {
        names.extend(extra.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
    }
    names.dedup();
    names
}

pub async fn setup() -> Result<Option<(RustlsConfig, String)>, String> {
    let mode = std::env::var("HTTPS").unwrap_or_else(|_| "self-signed".into()).to_lowercase();
    if mode == "off" {
        return Ok(None);
    }
    let dir = PathBuf::from(std::env::var("CERT_DIR").unwrap_or_else(|_| "certs".into()));
    let pair = find_pair(&dir);
    let (cert, key, about) = match (pair, mode.as_str()) {
        (Some((c, k)), _) => {
            let about = format!("Using the certificate in {}", c.display());
            (c, k, about)
        }
        (None, "auto") => return Ok(None),
        (None, "on") => return Err(format!("HTTPS=on but there's no certificate in {}", dir.display())),
        (None, _) => {
            let c = dir.join("self-signed-cert.pem");
            let k = dir.join("self-signed-key.pem");
            if !(c.exists() && k.exists()) {
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let names = hostnames();
                let made = rcgen::generate_simple_self_signed(names.clone()).map_err(|e| e.to_string())?;
                std::fs::write(&c, made.cert.pem()).map_err(|e| e.to_string())?;
                std::fs::write(&k, made.key_pair.serialize_pem()).map_err(|e| e.to_string())?;
                println!("Made a self-signed certificate for: {}", names.join(", "));
            }
            let about = format!(
                "Using the self-signed certificate in {} – browsers warn until it's trusted (see the README).",
                dir.display()
            );
            (c, k, about)
        }
    };
    let cfg = RustlsConfig::from_pem_file(&cert, &key)
        .await
        .map_err(|e| format!("the certificate or key in {} couldn't be used ({e})", dir.display()))?;
    Ok(Some((cfg, about)))
}
