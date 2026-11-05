//! Prompts for the local LLM and tolerant parsing of its JSON replies.
//!
//! Design rules for small local models:
//! * Pre-compute every number in Rust; the model only interprets. The
//!   prompt states the figures explicitly and forbids inventing data.
//! * Ask for a small, fixed JSON shape and request JSON mode
//!   (`LlmManager::chat_json`), then parse defensively: strip `<think>`
//!   blocks, take the outermost `{…}`, default missing fields.

use std::fmt::Write as _;

use anyhow::{Context, Result, anyhow};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::{HoldingInsight, InsightsReport};
use crate::llm::ChatMessage;

/// Per-holding commentary.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiHoldingNote {
    /// `Add`, `Hold`, `Trim` or `Watch`.
    pub verdict: String,
    /// One or two sentences.
    pub thesis: String,
    /// Up to three positives.
    pub strengths: Vec<String>,
    /// Up to three risks.
    pub risks: Vec<String>,
}

/// Portfolio-level commentary.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiPortfolioNote {
    /// One-line headline.
    pub headline: String,
    /// Short paragraph.
    pub overview: String,
    /// Concrete, prioritised actions.
    pub recommendations: Vec<String>,
    /// Buy / add opportunities with a reason each.
    pub opportunities: Vec<String>,
    /// Key risks.
    pub risks: Vec<String>,
}

/// System prompt shared by all calls.
pub const SYSTEM: &str = "You are a disciplined equity analyst writing for a private investor. \
Use ONLY the figures provided; never invent prices, ratios, news or events. \
Be specific and concise, cite the metrics you rely on, and acknowledge missing data. \
This is analysis, not personalised financial advice. Reply with a single JSON object only.";

fn fmt_opt(v: Option<f64>, f: impl Fn(f64) -> String) -> String {
    v.map_or_else(|| "n/a".into(), f)
}

fn pct(v: f64) -> String {
    format!("{:.1}%", v * 100.0)
}

/// Compact fact sheet for one holding.
pub fn holding_facts(h: &HoldingInsight, ccy: &str) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "Holding: {} ({})", h.name, h.ticker);
    let _ = writeln!(
        s,
        "Portfolio weight: {}; value {:.0} {ccy}; unrealised P/L: {}",
        pct(h.weight),
        h.value,
        fmt_opt(h.pnl_pct, pct)
    );
    match &h.fundamentals {
        None => {
            let _ = writeln!(s, "Fundamentals: unavailable");
        }
        Some(f) => {
            let _ = writeln!(
                s,
                "Type: {}; sector: {}",
                f.kind,
                f.sector.as_deref().unwrap_or("n/a")
            );
            let _ = writeln!(
                s,
                "Valuation: P/E ttm {}, P/E fwd {}, PEG {}, FCF yield {}, P/B {}",
                fmt_opt(f.pe_ttm, |v| format!("{v:.1}")),
                fmt_opt(f.pe_forward, |v| format!("{v:.1}")),
                fmt_opt(f.peg, |v| format!("{v:.2}")),
                fmt_opt(f.fcf_yield, pct),
                fmt_opt(f.price_to_book, |v| format!("{v:.1}")),
            );
            let _ = writeln!(
                s,
                "Growth: revenue {}, earnings {}. Quality: net margin {}, operating margin {}, ROE {}, debt/equity {}",
                fmt_opt(f.revenue_growth, pct),
                fmt_opt(f.earnings_growth, pct),
                fmt_opt(f.profit_margin, pct),
                fmt_opt(f.operating_margin, pct),
                fmt_opt(f.roe, pct),
                fmt_opt(f.debt_to_equity, |v| format!("{v:.2}")),
            );
            let _ = writeln!(
                s,
                "Income: dividend yield {}, payout {}{}",
                fmt_opt(f.dividend_yield, pct),
                fmt_opt(f.payout_ratio, pct),
                f.expense_ratio.map_or_else(String::new, |t| format!(
                    ", ETF ongoing charge {:.2}%",
                    t * 100.0
                )),
            );
            if !f.quarters.is_empty() {
                let q: Vec<String> = f
                    .quarters
                    .iter()
                    .map(|q| {
                        format!(
                            "{}: revenue {}",
                            q.label,
                            fmt_opt(q.revenue, |v| format!("{:.2}B", v / 1e9))
                        )
                    })
                    .collect();
                let _ = writeln!(s, "Recent quarters: {}", q.join("; "));
            }
        }
    }
    let sc = &h.scores;
    let _ = writeln!(
        s,
        "Model scores (0-100): overall {}, valuation {}, growth {}, quality {}, income {}. Rule-based signal: {}.",
        fmt_opt(sc.overall, |v| format!("{v:.0}")),
        fmt_opt(sc.valuation, |v| format!("{v:.0}")),
        fmt_opt(sc.growth, |v| format!("{v:.0}")),
        fmt_opt(sc.quality, |v| format!("{v:.0}")),
        fmt_opt(sc.income, |v| format!("{v:.0}")),
        h.signal.label(),
    );
    s
}

/// Prompt for one holding's note.
pub fn holding_prompt(h: &HoldingInsight, ccy: &str) -> Vec<ChatMessage> {
    vec![
        ChatMessage::system(SYSTEM),
        ChatMessage::user(format!(
            "{}\nReturn JSON: {{\"verdict\": \"Add\"|\"Hold\"|\"Trim\"|\"Watch\", \"thesis\": string (max 2 sentences), \
             \"strengths\": [max 3 short strings], \"risks\": [max 3 short strings]}}",
            holding_facts(h, ccy)
        )),
    ]
}

/// Prompt for the portfolio summary.
pub fn portfolio_prompt(r: &InsightsReport) -> Vec<ChatMessage> {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "Portfolio of {} positions, value {:.0} {}.",
        r.holdings.len(),
        r.total_value,
        r.currency
    );
    if let Some(score) = r.portfolio_score {
        let _ = writeln!(s, "Weighted fundamental score: {score:.0}/100.");
    }
    let sectors: Vec<String> = r
        .sectors
        .iter()
        .take(6)
        .map(|(n, w)| format!("{n} {}", pct(*w)))
        .collect();
    let _ = writeln!(s, "Sector mix: {}.", sectors.join(", "));
    if !r.flags.is_empty() {
        let _ = writeln!(s, "Observations: {}.", r.flags.join("; "));
    }
    let _ = writeln!(s, "Holdings (largest first):");
    for h in r.holdings.iter().take(15) {
        let f = h.fundamentals.as_ref();
        let _ = writeln!(
            s,
            "- {} {}: weight {}, P/L {}, P/E {}, rev growth {}, signal {}{}",
            h.ticker,
            h.name,
            pct(h.weight),
            fmt_opt(h.pnl_pct, pct),
            fmt_opt(f.and_then(|f| f.pe_ttm), |v| format!("{v:.1}")),
            fmt_opt(f.and_then(|f| f.revenue_growth), pct),
            h.signal.label(),
            h.ai.as_ref().map_or_else(String::new, |a| format!(
                ", analyst view: {} — {}",
                a.verdict, a.thesis
            )),
        );
    }
    vec![
        ChatMessage::system(SYSTEM),
        ChatMessage::user(format!(
            "{s}\nReturn JSON: {{\"headline\": string (max 12 words), \"overview\": string (3-4 sentences), \
             \"recommendations\": [3-5 concrete, prioritised actions], \
             \"opportunities\": [up to 4 'TICKER: reason' buy/add ideas from the holdings above, justified by valuation metrics], \
             \"risks\": [up to 4 key risks]}}"
        )),
    ]
}

/// Extracts and parses the JSON object from a model reply.
pub fn parse<T: DeserializeOwned>(raw: &str) -> Result<T> {
    let text = match (raw.find("<think>"), raw.find("</think>")) {
        (Some(_), Some(end)) => &raw[end + "</think>".len()..],
        _ => raw,
    };
    let start = text
        .find('{')
        .ok_or_else(|| anyhow!("no JSON object in model reply"))?;
    let end = text
        .rfind('}')
        .ok_or_else(|| anyhow!("unterminated JSON in model reply"))?;
    serde_json::from_str(&text[start..=end]).context("model reply is not the expected JSON")
}

/// Normalises a verdict to one of the four allowed values.
pub fn normalise_verdict(v: &str) -> String {
    let l = v.trim().to_lowercase();
    let out = if l.starts_with("add") || l.starts_with("buy") {
        "Add"
    } else if l.starts_with("trim") || l.starts_with("sell") || l.starts_with("reduce") {
        "Trim"
    } else if l.starts_with("watch") {
        "Watch"
    } else {
        "Hold"
    };
    out.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_wrapped_in_noise() {
        let raw = "<think>hmm</think>\nSure! ```json\n{\"verdict\":\"buy more\",\"thesis\":\"Cheap.\",\"risks\":[\"FX\"]}\n```";
        let n: AiHoldingNote = parse(raw).unwrap();
        assert_eq!(normalise_verdict(&n.verdict), "Add");
        assert_eq!(n.thesis, "Cheap.");
        assert!(n.strengths.is_empty()); // missing field defaults
    }

    #[test]
    fn rejects_non_json() {
        assert!(parse::<AiHoldingNote>("I cannot help with that").is_err());
    }
}
