use once_cell::sync::Lazy;
use regex::Regex;

pub fn capture_first(re: &Regex, hay: &str) -> Option<String> {
    re.captures(hay).and_then(|c| c.get(1).map(|m| m.as_str().trim().to_string()))
}

/// Sum a comma-separated list of numbers (the Prusa family emits one value per
/// extruder, e.g. `filament used [mm] = 100.5,200.3`). Returns `None` if no
/// component parses. Deleting the commas instead would yield garbage like
/// `100.5200.3`.
pub fn parse_sum_csv(s: &str) -> Option<f64> {
    let mut total = 0.0;
    let mut any = false;
    for part in s.split(',') {
        if let Ok(v) = part.trim().parse::<f64>() {
            total += v;
            any = true;
        }
    }
    any.then_some(total)
}

/// Parse `[Hh]:[Mm]:[Ss]` or `[N]h [N]m [N]s` (Prusa) or `[N]d [N]h [N]m` etc.
pub fn parse_duration_to_seconds(s: &str) -> Option<i64> {
    static SEG: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(\d+)\s*([dhms])").unwrap());
    let mut total: i64 = 0;
    let mut found = false;
    for cap in SEG.captures_iter(s) {
        found = true;
        // Saturate instead of panicking (debug) / wrapping (release) on absurd
        // values from untrusted gcode comments.
        let n: i64 = cap[1].parse().unwrap_or(0);
        let unit = &cap[2].to_ascii_lowercase();
        let seconds = match unit.as_str() {
            "d" => n.saturating_mul(86_400),
            "h" => n.saturating_mul(3600),
            "m" => n.saturating_mul(60),
            "s" => n,
            _ => 0,
        };
        total = total.saturating_add(seconds);
    }
    if found {
        return Some(total);
    }
    // Pure integer seconds (e.g. ";TIME:1234")
    s.trim().parse::<i64>().ok()
}

#[cfg(test)]
mod tests {
    use super::{parse_duration_to_seconds, parse_sum_csv};

    #[test]
    fn sums_multi_extruder_values() {
        assert_eq!(parse_sum_csv("100.5,200.3"), Some(300.8));
        assert_eq!(parse_sum_csv("100.5, 200.3"), Some(300.8));
        assert_eq!(parse_sum_csv("1234.56"), Some(1234.56));
        assert_eq!(parse_sum_csv("abc"), None);
    }

    #[test]
    fn duration_saturates_instead_of_overflowing() {
        assert_eq!(parse_duration_to_seconds("1h 30m"), Some(5400));
        // 2e14 days parses as i64 but overflows when * 86400 — must saturate, not panic.
        assert_eq!(parse_duration_to_seconds("200000000000000d"), Some(i64::MAX));
    }
}
