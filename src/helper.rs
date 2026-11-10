//! Messages between the web server and the Windows app in print helper mode.

use crate::config::Config;
use crate::xlsx::Product;
use serde::{Deserialize, Serialize};

/// A label to print, handed to the print helper.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobMsg {
    pub id: u64,
    pub product: Product,
    /// Label settings from the server (the helper keeps its own printer choice).
    pub cfg: Config,
    pub copies: u32,
}

/// "https://192.168.1.128:8443/x" -> (secure, host, port, base path without trailing /)
pub fn parse_server_url(url: &str) -> Option<(bool, String, u16, String)> {
    let url = url.trim();
    let (secure, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        (true, url)
    };
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].trim_end_matches('/').to_string()),
        None => (rest, String::new()),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() && !h.ends_with(']') || h.starts_with('[') => (h.to_string(), p.parse().ok()?),
        _ => (hostport.to_string(), if secure { 443 } else { 80 }),
    };
    if host.is_empty() {
        return None;
    }
    Some((secure, host, port, path))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn urls() {
        assert_eq!(parse_server_url("https://192.168.1.128:8443"), Some((true, "192.168.1.128".into(), 8443, "".into())));
        assert_eq!(parse_server_url("http://labels.local/"), Some((false, "labels.local".into(), 80, "".into())));
        assert_eq!(parse_server_url("192.168.1.128:8443/labels/"), Some((true, "192.168.1.128".into(), 8443, "/labels".into())));
        assert_eq!(parse_server_url("https://"), None);
    }
}
