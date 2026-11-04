//! Number formatting for money, percentages and quantities.

use eframe::egui::Color32;

use super::theme::palette::{MUTED, NEGATIVE, POSITIVE};

/// Currency symbol prefix for common ISO codes (falls back to `"XXX "`).
pub fn symbol(ccy: &str) -> String {
    match ccy {
        "GBP" => "£".into(),
        "EUR" => "€".into(),
        "USD" => "$".into(),
        "GBX" => "p".into(),
        "CHF" => "CHF ".into(),
        "" => String::new(),
        other => format!("{other} "),
    }
}

/// `1234567.891` → `"1,234,567.89"`.
pub fn grouped(value: f64, decimals: usize) -> String {
    let s = format!("{:.*}", decimals, value.abs());
    let (int, frac) = s
        .split_once('.')
        .map_or((s.as_str(), None), |(i, f)| (i, Some(f)));
    let mut out = String::with_capacity(s.len() + int.len() / 3);
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if let Some(f) = frac {
        out.push('.');
        out.push_str(f);
    }
    if value < 0.0 && value.abs() >= 0.5 * 10f64.powi(-(decimals as i32)) {
        format!("-{out}")
    } else {
        out
    }
}

/// `£1,234.56` (GBX is shown as `123.4p`).
pub fn money(value: f64, ccy: &str) -> String {
    if ccy == "GBX" {
        return format!("{}p", grouped(value, 2));
    }
    let sign = if value < 0.0 { "-" } else { "" };
    format!("{sign}{}{}", symbol(ccy), grouped(value.abs(), 2))
}

/// `+£12.30` / `-£4.00`.
pub fn signed_money(value: f64, ccy: &str) -> String {
    let sign = if value >= 0.0 { "+" } else { "-" };
    format!("{sign}{}{}", symbol(ccy), grouped(value.abs(), 2))
}

/// Compact money for axes: `£12.3k`, `£1.2M`.
pub fn compact_money(value: f64, ccy: &str) -> String {
    let (v, suffix) = match value.abs() {
        a if a >= 1e6 => (value / 1e6, "M"),
        a if a >= 1e3 => (value / 1e3, "k"),
        _ => (value, ""),
    };
    format!("{}{v:.1}{suffix}", symbol(ccy))
}

/// `0.1234` → `"12.34%"`.
pub fn pct(fraction: f64) -> String {
    format!("{:.2}%", fraction * 100.0)
}

/// `0.1234` → `"+12.34%"`.
pub fn signed_pct(fraction: f64) -> String {
    format!(
        "{}{:.2}%",
        if fraction >= 0.0 { "+" } else { "" },
        fraction * 100.0
    )
}

/// Shares: up to 6 decimals, trailing zeros trimmed.
pub fn quantity(q: f64) -> String {
    let s = format!("{q:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Green for gains, red for losses, muted for zero.
pub fn pnl_color(v: f64) -> Color32 {
    if v > 0.0 {
        POSITIVE
    } else if v < 0.0 {
        NEGATIVE
    } else {
        MUTED
    }
}

/// "3 min ago" style relative time.
pub fn ago(t: chrono::DateTime<chrono::Utc>) -> String {
    let secs = (chrono::Utc::now() - t).num_seconds().max(0);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouping() {
        assert_eq!(grouped(1_234_567.891, 2), "1,234,567.89");
        assert_eq!(grouped(999.0, 0), "999");
        assert_eq!(money(-1234.5, "GBP"), "-£1,234.50");
        assert_eq!(signed_money(12.3, "USD"), "+$12.30");
        assert_eq!(quantity(1.250000), "1.25");
        assert_eq!(compact_money(12_345.0, "EUR"), "€12.3k");
    }
}
