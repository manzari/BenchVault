//! Importers for fetching 3D models from external sources by URL.
//!
//! Currently supports Thingiverse `thing:NNN` URLs. The flow is:
//!   1. Fetch the public HTML page and parse the embedded JSON-LD `Product` block for
//!      title/author/license/cover-image.
//!   2. If a Thingiverse API token is configured (settings key `thingiverse.api_token`),
//!      call `api.thingiverse.com` to list and download the thing's files.
//!   3. Without a token, the object is still created with metadata + cover image; the
//!      caller is told no files were downloaded so the UI can prompt for a token.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use sqlx::SqlitePool;

use crate::domain::{CreateObjectInput, FolderId, Object};
use crate::library::Library;
use crate::repository;
use crate::service;

use super::{ServiceError, ServiceResult};

const USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) BenchVault/0.1";
const THINGIVERSE_TOKEN_KEY: &str = "thingiverse.api_token";

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct UrlImportReport {
    pub object: Object,
    pub source: ImportSource,
    /// Number of model files (.stl / .3mf / .obj) downloaded into the object.
    pub model_files_imported: u32,
    /// Number of files that were listed by the source but skipped (unsupported extension).
    pub files_skipped: u32,
    /// True if a cover image was set from the source.
    pub cover_imported: bool,
    /// Set when the source has files we could fetch but the user has not configured the
    /// required credentials (e.g. a Thingiverse API token).
    pub auth_required: bool,
    /// Human-readable warning to surface in the UI, if any.
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ImportSource {
    Thingiverse,
}

/// Dispatch on URL host. Currently only Thingiverse is recognized.
pub async fn import_from_url(
    pool: &SqlitePool,
    library: &Library,
    url: &str,
    folder_id: Option<FolderId>,
) -> ServiceResult<UrlImportReport> {
    if let Some(thing_id) = parse_thingiverse_thing_id(url) {
        return import_thingiverse(pool, library, thing_id, folder_id).await;
    }
    Err(ServiceError::Other(format!(
        "unsupported URL: {url} (only Thingiverse `thing:NNN` URLs are supported right now)"
    )))
}

pub async fn get_thingiverse_token(pool: &SqlitePool) -> ServiceResult<Option<String>> {
    Ok(repository::settings::get(pool, THINGIVERSE_TOKEN_KEY).await?)
}

pub async fn set_thingiverse_token(pool: &SqlitePool, token: Option<&str>) -> ServiceResult<()> {
    match token.map(str::trim).filter(|s| !s.is_empty()) {
        Some(t) => repository::settings::put(pool, THINGIVERSE_TOKEN_KEY, t).await?,
        None => repository::settings::delete(pool, THINGIVERSE_TOKEN_KEY).await?,
    }
    Ok(())
}

// --- Thingiverse ---

pub fn parse_thingiverse_thing_id(url: &str) -> Option<u64> {
    // Accept thing:NNN, /thing:NNN/..., /things/NNN, plain "NNN" if numeric and looks like an id.
    // Be conservative — only return Some when we're confident.
    let lower = url.trim();
    if let Some(re_match) = regex::Regex::new(r"thing:(\d+)").ok().and_then(|re| {
        re.captures(lower).and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
    }) {
        return re_match.parse().ok();
    }
    if let Some(re_match) = regex::Regex::new(r"/things/(\d+)").ok().and_then(|re| {
        re.captures(lower).and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
    }) {
        return re_match.parse().ok();
    }
    None
}

async fn import_thingiverse(
    pool: &SqlitePool,
    library: &Library,
    thing_id: u64,
    folder_id: Option<FolderId>,
) -> ServiceResult<UrlImportReport> {
    let client = http_client()?;
    let meta = fetch_thingiverse_metadata(&client, thing_id).await?;
    let token = get_thingiverse_token(pool).await?;

    // Stage downloads in a temp dir; tempdir is dropped (and removed) at the end of this fn.
    let staging = tempfile::Builder::new()
        .prefix("gcf-thingiverse-")
        .tempdir()
        .map_err(ServiceError::Io)?;

    // Cover image first so the object has a thumbnail even if file downloads fail.
    let cover_local = if let Some(cover_url) = meta.cover_image_url.as_deref() {
        match download_to(&client, cover_url, staging.path(), "cover.jpg").await {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("[thingiverse] cover download failed: {e}");
                None
            }
        }
    } else {
        None
    };

    // Fetch file list if we have a token. Without one, we still create the object with
    // metadata + cover, and tell the caller a token is required for file downloads.
    let mut model_paths: Vec<PathBuf> = Vec::new();
    let mut skipped: u32 = 0;
    let mut warning: Option<String> = None;
    let auth_required;

    match token.as_deref() {
        Some(tok) => {
            auth_required = false;
            match fetch_thingiverse_files(&client, tok, thing_id).await {
                Ok(files) => {
                    for file in files {
                        let dl = file.download_url.as_deref().unwrap_or(&file.public_url);
                        if dl.is_empty() {
                            continue;
                        }
                        if !is_supported_model_file(&file.name) {
                            skipped += 1;
                            continue;
                        }
                        match download_to(&client, dl, staging.path(), &file.name).await {
                            Ok(p) => model_paths.push(p),
                            Err(e) => {
                                eprintln!(
                                    "[thingiverse] file '{}' download failed: {e}",
                                    file.name
                                );
                                skipped += 1;
                            }
                        }
                    }
                }
                Err(e) => {
                    warning = Some(format!("Could not list Thingiverse files: {e}"));
                }
            }
        }
        None => {
            auth_required = true;
            warning = Some(
                "No Thingiverse API token configured — imported metadata + cover only. \
                 Add a token in Settings to download files."
                    .to_string(),
            );
        }
    }

    let stl_paths: Vec<String> = model_paths
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let cover_image_path = cover_local.as_ref().map(|p| p.to_string_lossy().to_string());

    let object = service::objects::create_with_files(
        pool,
        library,
        CreateObjectInput {
            name: meta.title.clone(),
            folder_id,
            notes: None,
            source_url: Some(meta.source_url.clone()),
            author: meta.author.clone(),
            license: meta.license.clone(),
            stl_paths,
            cover_image_path,
        },
    )
    .await?;

    let report = UrlImportReport {
        model_files_imported: model_paths.len() as u32,
        cover_imported: cover_local.is_some(),
        files_skipped: skipped,
        auth_required,
        warning,
        source: ImportSource::Thingiverse,
        object,
    };
    Ok(report)
}

#[derive(Debug, Clone)]
struct ThingMeta {
    #[allow(dead_code)]
    thing_id: u64,
    title: String,
    author: Option<String>,
    license: Option<String>,
    cover_image_url: Option<String>,
    source_url: String,
}

async fn fetch_thingiverse_metadata(
    client: &reqwest::Client,
    thing_id: u64,
) -> ServiceResult<ThingMeta> {
    let url = format!("https://www.thingiverse.com/thing:{thing_id}");
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(http_err)?
        .error_for_status()
        .map_err(http_err)?;
    let html = resp.text().await.map_err(http_err)?;

    let json_ld = extract_product_json_ld(&html);

    let title = json_ld
        .as_ref()
        .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .or_else(|| extract_meta_property(&html, "og:title"))
        .unwrap_or_else(|| format!("thing:{thing_id}"));

    let author = json_ld
        .as_ref()
        .and_then(|v| v.get("brand").and_then(|b| b.get("name")).and_then(|n| n.as_str()).map(str::to_string));

    let license = json_ld
        .as_ref()
        .and_then(|v| v.pointer("/mainEntityOfPage/license").and_then(|l| l.as_str()).map(str::to_string));

    let cover_image_url = json_ld
        .as_ref()
        .and_then(|v| v.get("thumbnailUrl").and_then(|t| t.as_str()).map(str::to_string))
        .or_else(|| extract_meta_property(&html, "og:image"))
        .map(|s| decode_html_entities(&s));

    Ok(ThingMeta {
        thing_id,
        title: decode_html_entities(&title),
        author: author.map(|s| decode_html_entities(&s)),
        license,
        cover_image_url,
        source_url: url,
    })
}

#[derive(Debug, Deserialize)]
struct ThingiverseFile {
    name: String,
    /// CDN URL the public web frontend embeds.
    #[serde(default)]
    public_url: String,
    /// Authenticated download URL — only present when the API call is authorized.
    #[serde(default)]
    download_url: Option<String>,
}

async fn fetch_thingiverse_files(
    client: &reqwest::Client,
    token: &str,
    thing_id: u64,
) -> Result<Vec<ThingiverseFile>, String> {
    let url = format!("https://api.thingiverse.com/things/{thing_id}/files");
    let resp = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("API {status}: {}", body.chars().take(200).collect::<String>()));
    }
    resp.json().await.map_err(|e| e.to_string())
}

async fn download_to(
    client: &reqwest::Client,
    url: &str,
    dir: &Path,
    suggested_name: &str,
) -> ServiceResult<PathBuf> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(http_err)?
        .error_for_status()
        .map_err(http_err)?;
    let bytes = resp.bytes().await.map_err(http_err)?;
    let safe = crate::library::sanitize_segment(suggested_name);
    let path = unique_path(dir, &safe);
    std::fs::write(&path, &bytes)?;
    Ok(path)
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 && i < name.len() - 1 => (&name[..i], &name[i + 1..]),
        _ => (name, ""),
    };
    for i in 1.. {
        let candidate = if ext.is_empty() {
            dir.join(format!("{stem}_{i}"))
        } else {
            dir.join(format!("{stem}_{i}.{ext}"))
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

fn http_client() -> ServiceResult<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(http_err)
}

fn http_err(e: reqwest::Error) -> ServiceError {
    ServiceError::Other(format!("http: {e}"))
}

fn is_supported_model_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    // Treat any of these as STL parts. The repository stores them all as kind=stl; that's a known
    // simplification documented in CLAUDE.md (we'll grow per-format kinds later).
    lower.ends_with(".stl") || lower.ends_with(".3mf") || lower.ends_with(".obj")
}

/// Walk through every `<script type="application/ld+json">` block and return the first JSON object
/// whose `@type` is `Product`. Thingiverse pages have two such blocks; only one is the thing.
fn extract_product_json_ld(html: &str) -> Option<serde_json::Value> {
    let re = regex::Regex::new(
        r#"(?is)<script[^>]*type=['"]application/ld\+json['"][^>]*>(.*?)</script>"#,
    )
    .ok()?;
    for cap in re.captures_iter(html) {
        let body = cap.get(1)?.as_str().trim();
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
            if v.get("@type").and_then(|t| t.as_str()) == Some("Product") {
                return Some(v);
            }
        }
    }
    None
}

fn extract_meta_property(html: &str, property: &str) -> Option<String> {
    // Match either property="x" content="y" or content="y" property="x".
    let pat1 = regex::Regex::new(&format!(
        r#"(?i)<meta[^>]*property=['"]{}['"][^>]*content=['"]([^'"]+)['"]"#,
        regex::escape(property)
    ))
    .ok()?;
    if let Some(c) = pat1.captures(html) {
        return c.get(1).map(|m| m.as_str().to_string());
    }
    let pat2 = regex::Regex::new(&format!(
        r#"(?i)<meta[^>]*content=['"]([^'"]+)['"][^>]*property=['"]{}['"]"#,
        regex::escape(property)
    ))
    .ok()?;
    pat2.captures(html).and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
}

fn decode_html_entities(s: &str) -> String {
    // Decode &amp; LAST: doing it first turns "&amp;lt;" into "&lt;" and then
    // into "<", double-decoding content that was only meant to be "&lt;".
    s.replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_thing_id_from_canonical_url() {
        assert_eq!(
            parse_thingiverse_thing_id("https://www.thingiverse.com/thing:763622"),
            Some(763622)
        );
        assert_eq!(
            parse_thingiverse_thing_id("https://www.thingiverse.com/thing:763622/files"),
            Some(763622)
        );
        assert_eq!(
            parse_thingiverse_thing_id("https://example.com/things/123"),
            Some(123)
        );
        assert_eq!(parse_thingiverse_thing_id("not a url"), None);
    }

    #[test]
    fn extracts_product_block() {
        let html = r#"
            <script type="application/ld+json">{"@context":"x","@type":"WebSite"}</script>
            <script type="application/ld+json">
                {"@context":"https://schema.org/","@type":"Product","name":"hello",
                 "brand":{"name":"alice"},"mainEntityOfPage":{"license":"CC0"},
                 "thumbnailUrl":"https://img/x.jpg"}
            </script>
        "#;
        let v = extract_product_json_ld(html).unwrap();
        assert_eq!(v.get("name").and_then(|n| n.as_str()), Some("hello"));
    }

    #[test]
    fn detects_model_files() {
        assert!(is_supported_model_file("part.stl"));
        assert!(is_supported_model_file("PART.STL"));
        assert!(is_supported_model_file("model.3mf"));
        assert!(is_supported_model_file("thing.obj"));
        assert!(!is_supported_model_file("readme.txt"));
        assert!(!is_supported_model_file("photo.jpg"));
    }
}
