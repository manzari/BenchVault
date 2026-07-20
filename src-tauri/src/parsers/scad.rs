//! Static OpenSCAD source analysis — no geometry, no external binary. Extracts
//! the customizer parameters, `include`/`use` dependencies, and module/function
//! names by a line-oriented scan (the same shape OpenSCAD's own customizer uses).
//! Heuristic by design: `//` and `/* */` inside string literals are not modelled.

use std::fs;
use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ScadParameter {
    pub name: String,
    pub default: String,
    pub group: Option<String>,
    /// Raw customizer hint from a trailing `// [ ... ]` comment (range or options).
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ScadMetadata {
    pub parameters: Vec<ScadParameter>,
    pub includes: Vec<String>,
    pub uses: Vec<String>,
    pub modules: Vec<String>,
    pub functions: Vec<String>,
}

#[cfg(test)]
impl ScadMetadata {
    fn is_empty(&self) -> bool {
        self.parameters.is_empty()
            && self.includes.is_empty()
            && self.uses.is_empty()
            && self.modules.is_empty()
            && self.functions.is_empty()
    }
}

pub fn parse_path(path: &Path) -> std::io::Result<ScadMetadata> {
    Ok(parse_str(&fs::read_to_string(path)?))
}

pub fn parse_str(src: &str) -> ScadMetadata {
    let mut m = ScadMetadata::default();
    let mut depth: i32 = 0; // net brace nesting before the current line
    let mut group: Option<String> = None;

    for raw in src.lines() {
        // A group header `/* [Name] */` on its own; also resets to no group for `[Hidden]`.
        if let Some(g) = group_header(raw) {
            group = if g.eq_ignore_ascii_case("Hidden") { None } else { Some(g) };
            continue;
        }

        let (code, comment) = split_line_comment(raw);
        let trimmed = code.trim();

        collect_dep(trimmed, "include", &mut m.includes);
        collect_dep(trimmed, "use", &mut m.uses);
        collect_decl(trimmed, "module", &mut m.modules);
        collect_decl(trimmed, "function", &mut m.functions);

        if depth == 0 {
            if let Some((name, default)) = top_level_assignment(&code) {
                m.parameters.push(ScadParameter {
                    name,
                    default,
                    group: group.clone(),
                    hint: comment.and_then(bracket_hint),
                });
            }
        }

        depth += brace_delta(&code);
        if depth < 0 {
            depth = 0;
        }
    }
    m
}

/// `/* [Name] */` → `Some("Name")`. Requires the line to be only that comment.
fn group_header(line: &str) -> Option<String> {
    let t = line.trim();
    let inner = t.strip_prefix("/*")?.strip_suffix("*/")?.trim();
    let name = inner.strip_prefix('[')?.strip_suffix(']')?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Split off a trailing `//` line comment, returning (code, comment_without_slashes).
fn split_line_comment(line: &str) -> (String, Option<String>) {
    match line.find("//") {
        Some(i) => (line[..i].to_string(), Some(line[i + 2..].trim().to_string())),
        None => (line.to_string(), None),
    }
}

/// A customizer hint is the bracketed part of a trailing comment: `[0:100]` or `[a,b]`.
fn bracket_hint(comment: String) -> Option<String> {
    let start = comment.find('[')?;
    let end = comment[start..].find(']')? + start;
    let inner = comment[start + 1..end].trim();
    (!inner.is_empty()).then(|| inner.to_string())
}

fn collect_dep(code: &str, keyword: &str, out: &mut Vec<String>) {
    let rest = match code.strip_prefix(keyword) {
        Some(r) if r.starts_with(|c: char| c.is_whitespace() || c == '<') => r,
        _ => return,
    };
    if let (Some(a), Some(b)) = (rest.find('<'), rest.find('>')) {
        if b > a {
            let dep = rest[a + 1..b].trim();
            if !dep.is_empty() && !out.iter().any(|d| d == dep) {
                out.push(dep.to_string());
            }
        }
    }
}

fn collect_decl(code: &str, keyword: &str, out: &mut Vec<String>) {
    let rest = match code.strip_prefix(keyword) {
        Some(r) if r.starts_with(char::is_whitespace) => r.trim_start(),
        _ => return,
    };
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .collect();
    if !name.is_empty() && !out.iter().any(|d| d == &name) {
        out.push(name);
    }
}

const RESERVED: &[&str] = &["module", "function", "if", "for", "let", "else", "include", "use"];

/// A single-line top-level `name = value;` assignment → (name, value literal).
fn top_level_assignment(code: &str) -> Option<(String, String)> {
    let t = code.trim();
    let t = t.strip_suffix(';')?.trim_end();
    let eq = find_assign_eq(t)?;
    let (lhs, rhs) = (t[..eq].trim(), t[eq + 1..].trim());
    if rhs.is_empty() || !is_identifier(lhs) || RESERVED.contains(&lhs) {
        return None;
    }
    Some((lhs.to_string(), rhs.to_string()))
}

/// Index of a plain `=` that is not part of `==`, `<=`, `>=`, `!=`.
fn find_assign_eq(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    for i in 0..b.len() {
        if b[i] == b'=' {
            let prev = (i > 0).then(|| b[i - 1]);
            let next = b.get(i + 1).copied();
            if next == Some(b'=') || matches!(prev, Some(b'=' | b'<' | b'>' | b'!')) {
                continue;
            }
            return Some(i);
        }
    }
    None
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

fn brace_delta(code: &str) -> i32 {
    code.bytes().fold(0, |acc, b| match b {
        b'{' => acc + 1,
        b'}' => acc - 1,
        _ => acc,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "\
// A parametric widget
include <MCAD/gears.scad>
use <helpers.scad>

/* [Dimensions] */
height = 10;      // [1:100]
width = 20;       // width in mm
style = \"round\"; // [round, square]

/* [Hidden] */
internal = 3;

module widget(h = 5) {
    inner = h * 2;   // not a top-level param
    function unused() = 1;
}

function pitch(d) = d / 2;
";

    #[test]
    fn extracts_params_deps_and_decls() {
        let m = parse_str(SRC);

        assert_eq!(m.includes, vec!["MCAD/gears.scad"]);
        assert_eq!(m.uses, vec!["helpers.scad"]);
        assert_eq!(m.modules, vec!["widget"]);
        assert!(m.functions.contains(&"pitch".to_string()));
        assert!(m.functions.contains(&"unused".to_string()));

        let names: Vec<&str> = m.parameters.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["height", "width", "style", "internal"]);

        let height = &m.parameters[0];
        assert_eq!(height.default, "10");
        assert_eq!(height.group.as_deref(), Some("Dimensions"));
        assert_eq!(height.hint.as_deref(), Some("1:100"));

        let width = &m.parameters[1];
        assert_eq!(width.hint, None); // trailing comment without brackets

        let style = &m.parameters[2];
        assert_eq!(style.default, "\"round\"");
        assert_eq!(style.hint.as_deref(), Some("round, square"));

        // `internal` is under a Hidden group header → no group.
        assert_eq!(m.parameters[3].group, None);
    }

    #[test]
    fn ignores_operators_that_look_like_assignments() {
        let m = parse_str("a = 1;\nif (a == 2) { b = 3; }\nc = a >= 1;\n");
        let names: Vec<&str> = m.parameters.iter().map(|p| p.name.as_str()).collect();
        // `b` is nested (depth 1); `c` is top-level with a `>=` in its value.
        assert_eq!(names, vec!["a", "c"]);
        assert_eq!(m.parameters[1].default, "a >= 1");
    }

    #[test]
    fn empty_for_geometry_only_source() {
        let m = parse_str("cube([1,2,3]);\nsphere(r=4);\n");
        assert!(m.is_empty());
    }
}
