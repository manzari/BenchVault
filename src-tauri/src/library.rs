use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Short-lived membership set used by the file watcher to ignore filesystem events
/// that the app itself just produced (e.g. `add_gcode` copying a file into the library).
/// Entries auto-expire to bound memory if a watcher event never arrives to clear them.
#[derive(Default)]
pub struct IgnoreSet {
    inner: Mutex<HashMap<PathBuf, Instant>>,
}

impl IgnoreSet {
    const TTL: Duration = Duration::from_secs(5);

    pub fn mark(&self, p: &Path) {
        let mut g = self.inner.lock().unwrap();
        g.insert(p.to_path_buf(), Instant::now());
    }

    /// True if the path was marked within the last few seconds. The matching entry is consumed.
    pub fn check(&self, p: &Path) -> bool {
        let now = Instant::now();
        let mut g = self.inner.lock().unwrap();
        g.retain(|_, t| now.duration_since(*t) < Self::TTL);
        g.remove(p).is_some()
    }
}

pub struct Library {
    root: PathBuf,
    /// Derived caches kept *outside* `root` so the filesystem watcher never sees
    /// them (e.g. OpenSCAD preview meshes). Lives at `<app_data>/cache/scad_previews`.
    scad_cache: PathBuf,
    ignore: IgnoreSet,
}

impl Library {
    pub fn new(app_data_dir: PathBuf) -> Self {
        Self {
            root: app_data_dir.join("library"),
            scad_cache: app_data_dir.join("cache/scad_previews"),
            ignore: IgnoreSet::default(),
        }
    }

    pub fn root(&self) -> &Path { &self.root }

    /// Directory holding cached OpenSCAD render output (`<asset_id>.stl` + `.json`).
    pub fn scad_cache_dir(&self) -> &Path { &self.scad_cache }

    pub fn ignore_set(&self) -> &IgnoreSet { &self.ignore }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        std::fs::create_dir_all(self.root.join("thumbnails"))?;
        std::fs::create_dir_all(&self.scad_cache)?;
        Ok(())
    }

    pub fn absolute(&self, relative: &Path) -> PathBuf {
        self.root.join(relative)
    }
}

/// Sanitize a single path segment (folder name, object name, file basename).
/// Allows ASCII alphanumerics, space, dot, dash, underscore; everything else becomes `_`.
/// Trims leading/trailing dots and spaces, caps length, and never returns an empty string.
pub fn sanitize_segment(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' | ' ' => c,
            _ => '_',
        })
        .collect();
    out = out.trim_matches(|c: char| c == '.' || c == ' ').to_string();
    if out.is_empty() {
        out = "_".to_string();
    }
    out.chars().take(120).collect()
}

/// Pick a unique directory name for a folder/object given the desired display name and the set
/// of dir_names already taken by siblings. The id is used as a deterministic disambiguator.
pub fn pick_dir_name(desired: &str, taken: &HashSet<String>, id: i64) -> String {
    let base = sanitize_segment(desired);
    if !taken.contains(&base) {
        return base;
    }
    let with_id = format!("{base}__{id}");
    if !taken.contains(&with_id) {
        return with_id;
    }
    for i in 2.. {
        let cand = format!("{base}__{id}_{i}");
        if !taken.contains(&cand) {
            return cand;
        }
    }
    unreachable!()
}

/// Build a unique filename inside `target_dir`, falling back to an asset_id-suffixed name if
/// the sanitized original filename collides with an existing file on disk.
pub fn unique_filename(target_dir: &Path, original_filename: &str, asset_id: i64) -> String {
    let safe = sanitize_segment(original_filename);
    if !target_dir.join(&safe).exists() {
        return safe;
    }
    let (stem, ext) = split_extension(&safe);
    let suffixed = if ext.is_empty() {
        format!("{stem}_{asset_id}")
    } else {
        format!("{stem}_{asset_id}.{ext}")
    };
    if !target_dir.join(&suffixed).exists() {
        return suffixed;
    }
    for i in 2.. {
        let cand = if ext.is_empty() {
            format!("{stem}_{asset_id}_{i}")
        } else {
            format!("{stem}_{asset_id}_{i}.{ext}")
        };
        if !target_dir.join(&cand).exists() {
            return cand;
        }
    }
    unreachable!()
}

fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(idx) if idx > 0 && idx < name.len() - 1 => (&name[..idx], &name[idx + 1..]),
        _ => (name, ""),
    }
}

/// Best-effort recursive removal of a directory. Errors are swallowed because the on-disk tree
/// is authoritative-but-disposable; the DB is the source of truth.
pub fn remove_dir_all_quiet(path: &Path) {
    if path.exists() {
        let _ = std::fs::remove_dir_all(path);
    }
}

/// Walk up from `start` and remove each empty ancestor directory until hitting `stop_at`
/// (exclusive) or a non-empty directory.
pub fn prune_empty_ancestors(start: &Path, stop_at: &Path) {
    let mut cur = start.to_path_buf();
    while cur.starts_with(stop_at) && cur != stop_at {
        match std::fs::read_dir(&cur) {
            Ok(mut it) => {
                if it.next().is_some() {
                    return;
                }
            }
            Err(_) => return,
        }
        if std::fs::remove_dir(&cur).is_err() {
            return;
        }
        match cur.parent() {
            Some(p) => cur = p.to_path_buf(),
            None => return,
        }
    }
}
