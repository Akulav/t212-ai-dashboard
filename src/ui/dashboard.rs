//! Dashboard page: portfolio KPIs, market context, value history, top
//! holdings and projected dividends.

use eframe::egui::{self, Align, Layout, RichText, Ui};

use super::theme::palette::*;
use super::widgets::{self, card, kpi};
use super::{charts, format, portfolio_header};
use crate::market::hours::{self, MarketPhase};
use crate::portfolio::{PortfolioService, PortfolioState};

/// Renders the dashboard.
pub fn show(ui: &mut Ui, portfolio: &PortfolioService, state: &PortfolioState) {
    portfolio_header(ui, "Portfolio overview", portfolio, state);
    ui.add_space(12.0);

    let view = state.view.as_deref();
    let ccy = view.map_or("", |v| v.summary.currency.as_str());
    let dash = "—".to_string();

    ui.columns(4, |c| match view {
        Some(v) => {
            let a = &v.analytics;
            let s = &v.summary;
            kpi(
                &mut c[0],
                "💼",
                "Portfolio value",
                &format::money(s.total_value, ccy),
                &format!("Cash {}", format::money(s.cash.available_to_trade, ccy)),
                ACCENT,
            );
            let caption = match (a.total_return_pct, a.total_return_from_deposits) {
                (Some(p), true) => format!("{} vs. net deposits", format::signed_pct(p)),
                (Some(p), false) => format!("{} on cost (P/L)", format::signed_pct(p)),
                (None, _) => "realised + unrealised".into(),
            };
            kpi(
                &mut c[1],
                "📈",
                "Total return",
                &format::signed_money(a.total_return, ccy),
                &caption,
                format::pnl_color(a.total_return),
            );
            kpi(
                &mut c[2],
                "💰",
                "Dividends received",
                &format::money(a.dividends.total, ccy),
                &format!(
                    "{} in the last 12 months",
                    format::money(a.dividends.last_12m, ccy)
                ),
                GOLD,
            );
            let tax = &a.dividends.tax;
            let (value, caption) = match tax.rate {
                Some(r) => (
                    format::pct(r),
                    format!(
                        "{}{} withheld · {} payments",
                        if tax.fx_estimated { "≈" } else { "" },
                        format::money(tax.withheld, ccy),
                        tax.measured
                    ),
                ),
                None => (dash.clone(), "no measurable dividends yet".into()),
            };
            kpi(
                &mut c[3],
                "🧾",
                "Avg. tax withheld",
                &value,
                &caption,
                VIOLET,
            );
        }
        None => {
            let hint = if state.has_credentials {
                "waiting for first sync…"
            } else {
                "connect Trading212"
            };
            kpi(&mut c[0], "💼", "Portfolio value", &dash, hint, ACCENT);
            kpi(&mut c[1], "📈", "Total return", &dash, hint, POSITIVE);
            kpi(&mut c[2], "💰", "Dividends received", &dash, hint, GOLD);
            kpi(&mut c[3], "🧾", "Avg. tax withheld", &dash, hint, VIOLET);
        }
    });
    ui.add_space(14.0);

    widgets::section_title(ui, "Market context");
    ui.add_space(4.0);
    let clock = hours::nyse_clock(chrono::Utc::now());
    let (bell_label, bell_color) = match clock.phase {
        MarketPhase::Open => ("Closing bell in", POSITIVE),
        MarketPhase::Closed => ("Opening bell in", ACCENT),
    };
    ui.columns(4, |c| {
        kpi(
            &mut c[0],
            "🔔",
            "Wall Street",
            &clock.countdown(),
            bell_label,
            bell_color,
        );
        kpi(
            &mut c[1],
            "🎈",
            "Inflation (CPI YoY)",
            &dash,
            "FRED · phase 3",
            WARNING,
        );
        kpi(
            &mut c[2],
            "🏦",
            "Money supply (M2)",
            &dash,
            "FRED · phase 3",
            ACCENT,
        );
        kpi(
            &mut c[3],
            "🥇",
            "Gold (XAU/USD)",
            &dash,
            "spot · phase 3",
            GOLD,
        );
    });
    ui.add_space(14.0);

    card(ui, |ui| {
        ui.horizontal(|ui| {
            widgets::section_title(ui, "Portfolio history");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new("Account value is recorded locally on each sync; deposits come from your full history.")
                        .size(11.5)
                        .color(MUTED),
                );
            });
        });
        ui.add_space(6.0);
        match view {
            Some(v) if !v.snapshots.is_empty() || !v.analytics.net_deposits.is_empty() => {
                charts::value_history(ui, &v.snapshots, &v.analytics.net_deposits, ccy, 260.0);
            }
            _ => {
                ui.add_space(110.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("History appears after the first sync.").color(MUTED))
                });
                ui.add_space(110.0);
            }
        }
    });

    let Some(v) = view else { return };
    ui.add_space(14.0);
    ui.columns(2, |c| {
        card(&mut c[0], |ui| {
            widgets::section_title(ui, "Top holdings");
            ui.add_space(6.0);
            for h in v.analytics.holdings.iter().take(8) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(short_ticker(&h.ticker)).strong().monospace());
                    ui.label(RichText::new(truncate(&h.name, 28)).color(MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(p) = h.pnl_pct {
                            ui.label(
                                RichText::new(format::signed_pct(p))
                                    .size(12.0)
                                    .color(format::pnl_color(p)),
                            );
                        }
                        ui.label(RichText::new(format::money(h.value, ccy)).monospace());
                    });
                });
                weight_bar(ui, h.weight);
            }
            if v.analytics.holdings.is_empty() {
                ui.label(RichText::new("No open positions.").color(MUTED));
            }
        });
        card(&mut c[1], |ui| {
            widgets::section_title(ui, "Upcoming dividends (projected)");
            ui.add_space(6.0);
            if v.analytics.upcoming.is_empty() {
                ui.label(RichText::new("Not enough payment history to project yet.").color(MUTED));
            }
            for d in v.analytics.upcoming.iter().take(8) {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(d.expected_on.format("%d %b").to_string())
                            .monospace()
                            .color(GOLD),
                    );
                    ui.label(RichText::new(short_ticker(&d.ticker)).strong().monospace());
                    ui.label(RichText::new(d.cadence.label()).size(11.5).color(MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("≈{}", format::money(d.amount, ccy))).monospace(),
                        );
                    });
                });
            }
        });
    });
}

/// Thin allocation bar.
pub fn weight_bar(ui: &mut Ui, weight: f64) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 4.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, egui::CornerRadius::same(2), SURFACE_2);
    let mut fill = rect;
    fill.set_width(rect.width() * weight.clamp(0.0, 1.0) as f32);
    p.rect_filled(
        fill,
        egui::CornerRadius::same(2),
        ACCENT.linear_multiply(0.8),
    );
}

/// `AAPL_US_EQ` → `AAPL`; `VUSAl_EQ` → `VUSA`.
pub fn short_ticker(t: &str) -> &str {
    let base = t.split('_').next().unwrap_or(t);
    // London tickers carry a trailing lowercase 'l' (e.g. "VUSAl").
    base.strip_suffix('l')
        .filter(|s| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        })
        .unwrap_or(base)
}

/// Truncates with an ellipsis.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::short_ticker;

    #[test]
    fn tickers() {
        assert_eq!(short_ticker("AAPL_US_EQ"), "AAPL");
        assert_eq!(short_ticker("VUSAl_EQ"), "VUSA");
        assert_eq!(short_ticker("BRK.B_US_EQ"), "BRK.B");
    }
}
