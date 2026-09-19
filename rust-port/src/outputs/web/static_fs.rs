//! Embedded static assets — Vue SPA + favicon + bundled JS.
//!
//! We `include_bytes!` the assets at compile time so the binary is
//! self-contained: no runtime file resolution, no missing-file footguns.
//! Path resolution is `../../../assets/static/<...>` from this file
//! (this file lives at `src/outputs/web/static_fs.rs`).
//!
//! Adding a new asset is a two-step process:
//!   1. Drop the file under `assets/static/...`
//!   2. Add a `pub const NAME: StaticAsset = StaticAsset { ... };` entry
//!      in the `ASSETS` table below.
//!
//! Then it's served at `/static/<name>`.

/// A single embedded file: `(name, content_type, bytes)`.
pub struct StaticAsset {
    pub name: &'static str,
    pub content_type: &'static str,
    pub bytes: &'static [u8],
}

const FAVICON_ICO: &[u8] = include_bytes!("../../../assets/static/public/favicon.ico");
const INDEX_HTML: &[u8] = include_bytes!("../../../assets/static/templates/index.html");
const ABOUT_HTML: &[u8] = include_bytes!("../../../assets/static/templates/about.html");
const BROWSER_HTML: &[u8] = include_bytes!("../../../assets/static/templates/browser.html");

/// Master table of every static asset served by the web UI.
/// Ordering: most-frequently-hit first so a linear scan stays cheap.
pub const ASSETS: &[StaticAsset] = &[
    StaticAsset { name: "favicon.ico", content_type: "image/x-icon", bytes: FAVICON_ICO },
    StaticAsset { name: "index.html",  content_type: "text/html; charset=utf-8", bytes: INDEX_HTML },
    StaticAsset { name: "about.html",  content_type: "text/html; charset=utf-8", bytes: ABOUT_HTML },
    StaticAsset { name: "browser.html", content_type: "text/html; charset=utf-8", bytes: BROWSER_HTML },
];

/// Lookup by asset name. Returns `(content_type, bytes)` or `None`.
pub fn lookup(name: &str) -> Option<(&'static str, &'static [u8])> {
    for a in ASSETS {
        if a.name == name {
            return Some((a.content_type, a.bytes));
        }
    }
    None
}

/// Lookup by URL path (e.g. `/`, `/static/index.html`, `/favicon.ico`).
/// Returns the matched asset name + (content_type, bytes).
pub fn lookup_path(path: &str) -> Option<(&str, &'static str, &'static [u8])> {
    let stripped = path.trim_start_matches('/');
    let stripped = stripped.strip_prefix("static/").unwrap_or(stripped);
    let name = match stripped {
        "" | "/" => "index.html",
        other => other,
    };
    let (ct, bytes) = lookup(name)?;
    Some((name, ct, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn index_lookup() {
        let (name, ct, bytes) = lookup_path("/").unwrap();
        assert_eq!(name, "index.html");
        assert_eq!(ct, "text/html; charset=utf-8");
        assert!(!bytes.is_empty());
    }
    #[test]
    fn favicon_lookup() {
        let (name, ct, _) = lookup_path("/favicon.ico").unwrap();
        assert_eq!(name, "favicon.ico");
        assert_eq!(ct, "image/x-icon");
    }
    #[test]
    fn unknown_path_returns_none() {
        assert!(lookup_path("/missing.png").is_none());
    }
}
