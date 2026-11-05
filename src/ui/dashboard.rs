//! Overview page on a 4-column grid:
//!
//! ```text
//! [ value ][ return ][ dividends ][ tax ]      KPIs          1·1·1·1
//! [        history              ][ alloc ]     charts        3·1
//! [   top holdings   ][ income ][ market ]     details       2·1·1
//! ```

use chrono::{Local, Timelike, Utc};
use chrono_tz::America::New_York;
use eframe::egui::{self, Align, Color32, Layout, RichText, Ui};
use egui_phosphor::regular as ph;

use super::theme::{palette::*, semibold};
use super::widgets::{self, GAP, Kpi, card_header, card_row, kpi_row};
use super::{charts, format, portfolio_header};
use crate::market::hours::{self, MarketPhase};
use crate::portfolio::{PortfolioService, PortfolioState, PortfolioView};

/// Height of chart areas, shared so the history and allocation cards match.
const CHART_H: f32 = 300.0;

/// Renders the overview.
pub fn show(ui: &mut Ui, portfolio: &PortfolioService, state: &PortfolioState) {
    portfolio_header(
        ui,
        "Overview",
        "Your portfolio at a glance",
        portfolio,
        state,
    );
    ui.add_space(20.0);

    let view = state.view.as_deref();
    kpis(ui, view, state.has_credentials || state.demo);
    ui.add_space(GAP);

    card_row(ui, "charts", &[3, 1], |i, ui| match i {
        0 => history(ui, view),
        _ => allocation(ui, view),
    });
    ui.add_space(GAP);

    card_row(ui, "details", &[2, 1, 1], |i, ui| match i {
        0 => top_holdings(ui, view),
        1 => upcoming(ui, view),
        _ => market(ui),
    });
}

fn kpis(ui: &mut Ui, view: Option<&PortfolioView>, connected: bool) {
    let Some(v) = view else {
        let hint = if connected {
            "waiting for first sync…"
        } else {
            "connect Trading212"
        };
        kpi_row(
            ui,
            "kpis",
            &[
                Kpi::new(ph::WALLET, "Portfolio value", "—", hint, ACCENT),
                Kpi::new(ph::TREND_UP, "Total return", "—", hint, POSITIVE),
                Kpi::new(ph::COINS, "Dividends received", "—", hint, GOLD),
                Kpi::new(ph::PERCENT, "Avg. tax withheld", "—", hint, VIOLET),
            ],
        );
        return;
    };
    let a = &v.analytics;
    let s = &v.summary;
    let ccy = s.currency.as_str();
    let trend: Vec<f64> = v
        .snapshots
        .iter()
        .rev()
        .take(120)
        .rev()
        .map(|s| s.total_value)
        .collect();
    let cash = if a.total_return_from_deposits || s.cash.available_to_trade > 0.0 {
        format!("Cash {}", format::money(s.cash.available_to_trade, ccy))
    } else {
        "Open positions".into()
    };
    let ret_caption = match (a.total_return_pct, a.total_return_from_deposits) {
        (Some(p), true) => format!("{} vs. net deposits", format::signed_pct(p)),
        (Some(p), false) => format!("{} on open positions", format::signed_pct(p)),
        (None, _) => "realised + unrealised".into(),
    };
    let tax = &a.dividends.tax;
    let (tax_value, tax_caption) = match tax.rate {
        Some(r) => (
            format::pct(r),
            format!(
                "{}{} withheld · {} payments",
                if tax.fx_estimated { "≈" } else { "" },
                format::money(tax.withheld, ccy),
                tax.measured
            ),
        ),
        None => ("—".into(), "no measurable dividends yet".into()),
    };
    let ret_color = format::pnl_color(a.total_return);
    kpi_row(
        ui,
        "kpis",
        &[
            Kpi::new(
                ph::WALLET,
                "Portfolio value",
                format::money(s.total_value, ccy),
                cash,
                ACCENT,
            )
            .trend(&trend),
            Kpi::new(
                ph::TREND_UP,
                "Total return",
                format::signed_money(a.total_return, ccy),
                ret_caption,
                ret_color,
            )
            .value_color(ret_color),
            Kpi::new(
                ph::COINS,
                "Dividends received",
                format::money(a.dividends.total, ccy),
                format!(
                    "{} in the last 12 months",
                    format::money(a.dividends.last_12m, ccy)
                ),
                GOLD,
            ),
            Kpi::new(
                ph::PERCENT,
                "Avg. tax withheld",
                tax_value,
                tax_caption,
                VIOLET,
            ),
        ],
    );
}

fn history(ui: &mut Ui, view: Option<&PortfolioView>) {
    card_header(
        ui,
        ph::CHART_LINE,
        "Portfolio history",
        "value recorded on each sync · deposits from history",
    );
    match view {
        Some(v) if !v.snapshots.is_empty() || !v.analytics.net_deposits.is_empty() => {
            charts::value_history(
                ui,
                &v.snapshots,
                &v.analytics.net_deposits,
                &v.summary.currency,
                CHART_H,
            );
        }
        _ => placeholder(ui, CHART_H, "History appears after the first sync."),
    }
}

fn allocation(ui: &mut Ui, view: Option<&PortfolioView>) {
    card_header(ui, ph::CHART_PIE_SLICE, "Allocation", "");
    let Some(v) = view.filter(|v| !v.analytics.holdings.is_empty()) else {
        placeholder(ui, CHART_H, "No open positions.");
        return;
    };
    let holdings = &v.analytics.holdings;
    let mut slices: Vec<(String, f64, Color32)> = holdings
        .iter()
        .take(5)
        .enumerate()
        .map(|(i, h)| {
            (
                short_ticker(&h.ticker).to_string(),
                h.value,
                SERIES[i % SERIES.len()],
            )
        })
        .collect();
    let other: f64 = holdings.iter().skip(5).map(|h| h.value).sum();
    if other > 0.0 {
        slices.push(("Other".into(), other, MUTED.linear_multiply(0.6)));
    }
    widgets::donut_stacked(ui, &slices, &holdings.len().to_string(), "positions", 170.0);
}

/// One list row: colour bar, two-line label, right-aligned two-line value.
fn list_row(
    ui: &mut Ui,
    marker: impl FnOnce(&mut Ui),
    title: &str,
    subtitle: &str,
    value: RichText,
    sub_value: Option<RichText>,
) {
    ui.horizontal(|ui| {
        marker(ui);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(RichText::new(title).font(semibold(13.5)));
            ui.label(RichText::new(subtitle).size(11.5).color(MUTED));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.with_layout(Layout::top_down(Align::Max), |ui| {
                    ui.label(value);
                    if let Some(s) = sub_value {
                        ui.label(s);
                    }
                });
            });
        });
    });
    ui.add_space(6.0);
}

fn top_holdings(ui: &mut Ui, view: Option<&PortfolioView>) {
    card_header(ui, ph::BRIEFCASE, "Top holdings", "by value");
    let Some(v) = view else {
        placeholder(ui, 220.0, "Connect Trading212 to see positions.");
        return;
    };
    let ccy = v.summary.currency.as_str();
    for (i, h) in v.analytics.holdings.iter().take(6).enumerate() {
        let color = SERIES[i % SERIES.len()];
        list_row(
            ui,
            |ui| {
                let (bar, _) = ui.allocate_exact_size(egui::vec2(4.0, 30.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(bar, egui::CornerRadius::same(2), color);
            },
            short_ticker(&h.ticker),
            &truncate(&h.name, 40),
            RichText::new(format::money(h.value, ccy))
                .monospace()
                .size(13.0),
            h.pnl_pct.map(|p| {
                RichText::new(format::signed_pct(p))
                    .size(11.5)
                    .color(format::pnl_color(p))
            }),
        );
    }
}

fn upcoming(ui: &mut Ui, view: Option<&PortfolioView>) {
    card_header(ui, ph::CALENDAR_CHECK, "Upcoming income", "projected");
    let Some(v) = view else {
        placeholder(ui, 220.0, "Connect Trading212 to project income.");
        return;
    };
    if v.analytics.upcoming.is_empty() {
        placeholder(ui, 220.0, "Not enough payment history yet.");
        return;
    }
    let ccy = v.summary.currency.as_str();
    for d in v.analytics.upcoming.iter().take(6) {
        list_row(
            ui,
            |ui| date_tile(ui, d.expected_on),
            short_ticker(&d.ticker),
            d.cadence.label(),
            RichText::new(format!("≈{}", format::money(d.amount, ccy)))
                .monospace()
                .color(GOLD),
            None,
        );
    }
}

/// Calendar-style day tile: "NOV" over "13".
fn date_tile(ui: &mut Ui, date: chrono::NaiveDate) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(38.0, 38.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(
        rect,
        egui::CornerRadius::same(8),
        GOLD.linear_multiply(0.12),
    );
    p.text(
        rect.center_top() + egui::vec2(0.0, 10.0),
        egui::Align2::CENTER_CENTER,
        date.format("%b").to_string().to_uppercase(),
        egui::FontId::proportional(9.5),
        GOLD,
    );
    p.text(
        rect.center() + egui::vec2(0.0, 6.0),
        egui::Align2::CENTER_CENTER,
        date.format("%d").to_string(),
        semibold(15.0),
        TEXT,
    );
}

fn market(ui: &mut Ui) {
    card_header(ui, ph::BELL_SIMPLE, "Wall Street", "NYSE");
    let now = Utc::now();
    let clock = hours::nyse_clock(now);
    let (state, color, verb) = match clock.phase {
        MarketPhase::Open => ("Market open", POSITIVE, "Closing bell in"),
        MarketPhase::Closed => ("Market closed", MUTED, "Opening bell in"),
    };
    ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
        ui.painter().circle_filled(dot.center(), 4.5, color);
        ui.label(RichText::new(state).font(semibold(14.0)).color(color));
    });
    ui.add_space(10.0);
    ui.label(RichText::new(verb).size(12.5).color(MUTED));
    ui.label(
        RichText::new(clock.countdown())
            .font(semibold(32.0))
            .color(TEXT),
    );
    ui.add_space(12.0);

    // Session progress 09:30 → 16:00 New York time.
    let ny = now.with_timezone(&New_York);
    let minutes = ny.hour() as f32 * 60.0 + ny.minute() as f32;
    let frac = if clock.phase == MarketPhase::Open {
        ((minutes - 570.0) / 390.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 8.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, egui::CornerRadius::same(4), SURFACE_2);
    let mut fill = rect;
    fill.set_width(rect.width() * frac);
    p.rect_filled(
        fill,
        egui::CornerRadius::same(4),
        POSITIVE.linear_multiply(0.85),
    );
    ui.horizontal(|ui| {
        ui.label(RichText::new("09:30 ET").size(11.0).color(MUTED));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new("16:00 ET").size(11.0).color(MUTED));
        });
    });
    ui.add_space(10.0);
    let local = clock.next_bell.with_timezone(&Local);
    ui.label(
        RichText::new("Next bell (your time)")
            .size(11.5)
            .color(MUTED),
    );
    ui.label(RichText::new(local.format("%a %d %b, %H:%M").to_string()).font(semibold(13.5)));
}

fn placeholder(ui: &mut Ui, height: f32, text: &str) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), height),
        Layout::centered_and_justified(egui::Direction::TopDown),
        |ui| {
            ui.label(RichText::new(text).color(MUTED));
        },
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

/// Thin allocation bar.
pub fn weight_bar(ui: &mut Ui, weight: f64, color: Color32) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 4.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, egui::CornerRadius::same(2), SURFACE_2);
    let mut fill = rect;
    fill.set_width(rect.width() * weight.clamp(0.0, 1.0) as f32);
    p.rect_filled(fill, egui::CornerRadius::same(2), color);
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
