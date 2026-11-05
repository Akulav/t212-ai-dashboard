//! Overview page: KPIs, value history, allocation, top holdings, projected
//! dividends and the Wall Street session.

use chrono::{Local, Timelike, Utc};
use chrono_tz::America::New_York;
use eframe::egui::{self, Align, Color32, Layout, RichText, Ui};
use egui_phosphor::regular as ph;

use super::theme::{palette::*, semibold};
use super::widgets::{self, card, kpi, kpi_with};
use super::{charts, format, portfolio_header};
use crate::market::hours::{self, MarketPhase};
use crate::portfolio::{PortfolioService, PortfolioState, PortfolioView};

const GAP: f32 = 14.0;

/// Renders the overview.
pub fn show(ui: &mut Ui, portfolio: &PortfolioService, state: &PortfolioState) {
    portfolio_header(
        ui,
        "Overview",
        "Your portfolio at a glance",
        portfolio,
        state,
    );
    ui.add_space(18.0);

    let view = state.view.as_deref();
    kpi_row(ui, view, state.has_credentials || state.demo);
    ui.add_space(GAP);

    // Row 2: history (≈ 2/3) + allocation (≈ 1/3).
    split(
        ui,
        0.64,
        340.0,
        |ui| history_card(ui, view),
        |ui| allocation_card(ui, view),
    );
    ui.add_space(GAP);

    // Row 3: holdings · dividends · market.
    ui.columns(3, |c| {
        top_holdings_card(&mut c[0], view);
        upcoming_card(&mut c[1], view);
        market_card(&mut c[2]);
    });
}

fn kpi_row(ui: &mut Ui, view: Option<&PortfolioView>, connected: bool) {
    let dash = "—";
    ui.columns(4, |c| match view {
        Some(v) => {
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
            kpi_with(
                &mut c[0],
                ph::WALLET,
                "Portfolio value",
                &format::money(s.total_value, ccy),
                &cash,
                ACCENT,
                TEXT,
                Some(&trend),
            );

            let caption = match (a.total_return_pct, a.total_return_from_deposits) {
                (Some(p), true) => format!("{} vs. net deposits", format::signed_pct(p)),
                (Some(p), false) => format!("{} on open positions", format::signed_pct(p)),
                (None, _) => "realised + unrealised".into(),
            };
            kpi_with(
                &mut c[1],
                ph::TREND_UP,
                "Total return",
                &format::signed_money(a.total_return, ccy),
                &caption,
                format::pnl_color(a.total_return),
                format::pnl_color(a.total_return),
                None,
            );
            kpi(
                &mut c[2],
                ph::COINS,
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
                None => (dash.into(), "no measurable dividends yet".into()),
            };
            kpi(
                &mut c[3],
                ph::PERCENT,
                "Avg. tax withheld",
                &value,
                &caption,
                VIOLET,
            );
        }
        None => {
            let hint = if connected {
                "waiting for first sync…"
            } else {
                "connect Trading212"
            };
            kpi(&mut c[0], ph::WALLET, "Portfolio value", dash, hint, ACCENT);
            kpi(
                &mut c[1],
                ph::TREND_UP,
                "Total return",
                dash,
                hint,
                POSITIVE,
            );
            kpi(&mut c[2], ph::COINS, "Dividends received", dash, hint, GOLD);
            kpi(
                &mut c[3],
                ph::PERCENT,
                "Avg. tax withheld",
                dash,
                hint,
                VIOLET,
            );
        }
    });
}

/// Two side-by-side regions; `ratio` is the left share of the width.
fn split(
    ui: &mut Ui,
    ratio: f32,
    height: f32,
    left: impl FnOnce(&mut Ui),
    right: impl FnOnce(&mut Ui),
) {
    let total = ui.available_width();
    let lw = (total - GAP) * ratio;
    let rw = total - GAP - lw;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        ui.allocate_ui_with_layout(egui::vec2(lw, height), Layout::top_down(Align::Min), |ui| {
            ui.set_width(lw);
            left(ui)
        });
        ui.allocate_ui_with_layout(egui::vec2(rw, height), Layout::top_down(Align::Min), |ui| {
            ui.set_width(rw);
            right(ui)
        });
    });
}

fn card_header(ui: &mut Ui, icon: &str, title: &str, note: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).size(16.0).color(MUTED));
        widgets::section_title(ui, title);
        if !note.is_empty() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(note).size(11.5).color(MUTED));
            });
        }
    });
    ui.add_space(8.0);
}

fn history_card(ui: &mut Ui, view: Option<&PortfolioView>) {
    card(ui, |ui| {
        ui.set_min_height(300.0);
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
                    270.0,
                );
            }
            _ => placeholder(ui, 270.0, "History appears after the first sync."),
        }
    });
}

fn allocation_card(ui: &mut Ui, view: Option<&PortfolioView>) {
    card(ui, |ui| {
        ui.set_min_height(300.0);
        ui.set_min_width(ui.available_width());
        card_header(ui, ph::CHART_PIE_SLICE, "Allocation", "");
        let Some(v) = view.filter(|v| !v.analytics.holdings.is_empty()) else {
            placeholder(ui, 270.0, "No open positions.");
            return;
        };
        let holdings = &v.analytics.holdings;
        let mut slices: Vec<(String, f64, Color32)> = holdings
            .iter()
            .take(6)
            .enumerate()
            .map(|(i, h)| {
                (
                    short_ticker(&h.ticker).to_string(),
                    h.value,
                    SERIES[i % SERIES.len()],
                )
            })
            .collect();
        let other: f64 = holdings.iter().skip(6).map(|h| h.value).sum();
        if other > 0.0 {
            slices.push((
                format!("{} others", holdings.len() - 6),
                other,
                MUTED.linear_multiply(0.6),
            ));
        }
        ui.add_space(14.0);
        widgets::donut(ui, &slices, &holdings.len().to_string(), "positions", 190.0);
    });
}

fn top_holdings_card(ui: &mut Ui, view: Option<&PortfolioView>) {
    card(ui, |ui| {
        ui.set_min_height(300.0);
        card_header(ui, ph::BRIEFCASE, "Top holdings", "");
        let Some(v) = view else {
            placeholder(ui, 240.0, "Connect Trading212 to see positions.");
            return;
        };
        let ccy = v.summary.currency.as_str();
        for (i, h) in v.analytics.holdings.iter().take(6).enumerate() {
            let color = SERIES[i % SERIES.len()];
            ui.horizontal(|ui| {
                let (bar, _) = ui.allocate_exact_size(egui::vec2(4.0, 30.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(bar, egui::CornerRadius::same(2), color);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(RichText::new(short_ticker(&h.ticker)).font(semibold(13.5)));
                    ui.label(RichText::new(truncate(&h.name, 24)).size(11.5).color(MUTED));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.with_layout(Layout::top_down(Align::Max), |ui| {
                            ui.label(
                                RichText::new(format::money(h.value, ccy))
                                    .monospace()
                                    .size(13.0),
                            );
                            if let Some(p) = h.pnl_pct {
                                ui.label(
                                    RichText::new(format::signed_pct(p))
                                        .size(11.5)
                                        .color(format::pnl_color(p)),
                                );
                            }
                        });
                    });
                });
            });
            ui.add_space(4.0);
        }
    });
}

fn upcoming_card(ui: &mut Ui, view: Option<&PortfolioView>) {
    card(ui, |ui| {
        ui.set_min_height(300.0);
        card_header(ui, ph::CALENDAR_CHECK, "Upcoming dividends", "projected");
        let Some(v) = view else {
            placeholder(ui, 240.0, "Connect Trading212 to project income.");
            return;
        };
        let ccy = v.summary.currency.as_str();
        if v.analytics.upcoming.is_empty() {
            placeholder(ui, 240.0, "Not enough payment history yet.");
            return;
        }
        for d in v.analytics.upcoming.iter().take(6) {
            ui.horizontal(|ui| {
                date_tile(ui, d.expected_on);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(RichText::new(short_ticker(&d.ticker)).font(semibold(13.5)));
                    ui.label(RichText::new(d.cadence.label()).size(11.5).color(MUTED));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("≈{}", format::money(d.amount, ccy)))
                            .monospace()
                            .color(GOLD),
                    );
                });
            });
            ui.add_space(4.0);
        }
    });
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

fn market_card(ui: &mut Ui) {
    card(ui, |ui| {
        ui.set_min_height(300.0);
        card_header(ui, ph::BELL_SIMPLE, "Wall Street", "NYSE regular session");
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
        ui.add_space(8.0);
        ui.label(RichText::new(verb).size(12.5).color(MUTED));
        ui.label(
            RichText::new(clock.countdown())
                .font(semibold(34.0))
                .color(TEXT),
        );
        ui.add_space(10.0);

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
        ui.add_space(8.0);
        let local = clock.next_bell.with_timezone(&Local);
        ui.label(
            RichText::new(format!(
                "Next bell: {} your time",
                local.format("%a %d %b, %H:%M")
            ))
            .size(12.0)
            .color(MUTED),
        );
    });
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
