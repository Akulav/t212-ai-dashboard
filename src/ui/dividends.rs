//! Dividends page on the 4-column grid: KPIs, monthly income chart,
//! projected calendar + top payers (2·2), and the payment history table.

use eframe::egui::{self, Align, Layout, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as ph;

use super::dashboard::{short_ticker, truncate, weight_bar};
use super::holdings::right;
use super::theme::{palette::*, semibold};
use super::widgets::{self, GAP, Kpi, card, card_header, card_row, kpi_row};
use super::{charts, format, portfolio_header};
use crate::portfolio::{PortfolioService, PortfolioState, PortfolioView};

/// Renders the page.
pub fn show(ui: &mut Ui, portfolio: &PortfolioService, state: &PortfolioState) {
    portfolio_header(
        ui,
        "Dividends",
        "Income received, tax withheld and projected payments",
        portfolio,
        state,
    );
    ui.add_space(20.0);
    let Some(view) = state.view.as_deref() else {
        widgets::empty_state(
            ui,
            ph::COINS,
            "No data yet",
            "Connect Trading212 in Settings to see your dividend income.",
        );
        return;
    };
    let ccy = view.summary.currency.as_str();
    let d = &view.analytics.dividends;
    let cutoff = chrono::Utc::now().date_naive() + chrono::Duration::days(90);
    let projected_90: f64 = view
        .analytics
        .upcoming
        .iter()
        .filter(|u| u.expected_on <= cutoff)
        .map(|u| u.amount)
        .sum();
    let (rate, rate_caption) = match d.tax.rate {
        Some(r) => (
            format::pct(r),
            format!(
                "{} withheld{}",
                format::money(d.tax.withheld, ccy),
                if d.tax.fx_estimated {
                    " · FX est."
                } else {
                    ""
                }
            ),
        ),
        None => ("—".into(), "not enough data".into()),
    };

    kpi_row(
        ui,
        "dividend-kpis",
        &[
            Kpi::new(
                ph::COINS,
                "Total received",
                format::money(d.total, ccy),
                format!("{} payments", d.payments),
                GOLD,
            ),
            Kpi::new(
                ph::CALENDAR_BLANK,
                "Last 12 months",
                format::money(d.last_12m, ccy),
                format!("YTD {}", format::money(d.ytd, ccy)),
                ACCENT,
            ),
            Kpi::new(ph::PERCENT, "Avg. tax withheld", rate, rate_caption, VIOLET),
            Kpi::new(
                ph::CALENDAR_CHECK,
                "Next 90 days",
                format!("≈{}", format::money(projected_90, ccy)),
                "projected from history",
                POSITIVE,
            ),
        ],
    );
    ui.add_space(GAP);

    card(ui, |ui| {
        card_header(ui, ph::CHART_BAR, "Monthly income", "last 24 months");
        charts::monthly_dividends(ui, &d.by_month, ccy, 220.0);
    });
    ui.add_space(GAP);

    card_row(ui, "dividend-lists", &[2, 2], |i, ui| match i {
        0 => calendar(ui, view),
        _ => top_payers(ui, view),
    });
    ui.add_space(GAP);

    card(ui, |ui| history(ui, view));
}

fn calendar(ui: &mut Ui, view: &PortfolioView) {
    let ccy = view.summary.currency.as_str();
    card_header(
        ui,
        ph::CALENDAR_CHECK,
        "Projected calendar",
        "next 120 days · estimated from payment cadence",
    );
    if view.analytics.upcoming.is_empty() {
        ui.label(RichText::new("Nothing projected yet.").color(MUTED));
        return;
    }
    for u in view.analytics.upcoming.iter().take(10) {
        ui.horizontal(|ui| {
            ui.add_sized(
                [96.0, 22.0],
                egui::Label::new(
                    RichText::new(u.expected_on.format("%a %d %b").to_string())
                        .monospace()
                        .color(GOLD),
                ),
            );
            ui.add_sized(
                [64.0, 22.0],
                egui::Label::new(RichText::new(short_ticker(&u.ticker)).font(semibold(13.0))),
            );
            ui.label(RichText::new(u.cadence.label()).size(11.5).color(MUTED));
            right(ui, |ui| {
                ui.label(RichText::new(format!("≈{}", format::money(u.amount, ccy))).monospace());
            });
        });
        ui.add_space(4.0);
    }
}

fn top_payers(ui: &mut Ui, view: &PortfolioView) {
    let ccy = view.summary.currency.as_str();
    let d = &view.analytics.dividends;
    card_header(ui, ph::TROPHY, "Top payers", "lifetime");
    let top = d.by_ticker.first().map_or(0.0, |t| t.2).max(f64::EPSILON);
    for (ticker, name, total) in d.by_ticker.iter().take(8) {
        ui.horizontal(|ui| {
            ui.add_sized(
                [64.0, 20.0],
                egui::Label::new(RichText::new(short_ticker(ticker)).font(semibold(13.0))),
            );
            ui.label(RichText::new(truncate(name, 30)).size(12.5).color(MUTED));
            right(ui, |ui| {
                ui.label(RichText::new(format::money(*total, ccy)).monospace());
            });
        });
        weight_bar(ui, total / top, GOLD.linear_multiply(0.8));
        ui.add_space(6.0);
    }
}

fn history(ui: &mut Ui, view: &PortfolioView) {
    let ccy = view.summary.currency.as_str();
    card_header(
        ui,
        ph::RECEIPT,
        "Payment history",
        &format!("{} payments · latest 100 shown", view.dividend_count),
    );
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
    let head = |ui: &mut Ui, text: &str, align_right: bool| {
        let label = RichText::new(text.to_uppercase())
            .font(semibold(11.0))
            .color(MUTED);
        if align_right {
            right(ui, |ui| {
                ui.label(label);
            });
        } else {
            ui.label(label);
        }
    };
    TableBuilder::new(ui)
        .id_salt("dividend-history")
        .striped(true)
        .vscroll(false)
        .cell_layout(Layout::left_to_right(Align::Center))
        .column(Column::exact(110.0))
        .column(Column::remainder().at_least(220.0))
        .column(Column::exact(90.0))
        .column(Column::exact(120.0))
        .column(Column::exact(200.0))
        .column(Column::exact(110.0))
        .header(30.0, |mut h| {
            h.col(|ui| head(ui, "Paid", false));
            h.col(|ui| head(ui, "Instrument", false));
            h.col(|ui| head(ui, "Shares", true));
            h.col(|ui| head(ui, "Gross / share", true));
            h.col(|ui| head(ui, "Type", false));
            h.col(|ui| head(ui, "Net", true));
        })
        .body(|mut body| {
            for p in view.recent_dividends.iter().take(100) {
                body.row(34.0, |mut row| {
                    row.col(|ui| {
                        ui.label(
                            RichText::new(
                                p.paid_on
                                    .map_or_else(String::new, |t| t.format("%d %b %Y").to_string()),
                            )
                            .monospace(),
                        );
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(short_ticker(&p.ticker)).font(semibold(13.0)));
                        ui.label(
                            RichText::new(truncate(&p.instrument.name, 36))
                                .size(12.0)
                                .color(MUTED),
                        );
                    });
                    row.col(|ui| {
                        right(ui, |ui| {
                            ui.label(RichText::new(format::quantity(p.quantity)).monospace());
                        })
                    });
                    row.col(|ui| {
                        right(ui, |ui| {
                            ui.label(
                                RichText::new(format::money(
                                    p.gross_amount_per_share,
                                    &p.ticker_currency,
                                ))
                                .monospace(),
                            );
                        })
                    });
                    row.col(|ui| {
                        ui.label(
                            RichText::new(p.kind.replace('_', " ").to_lowercase())
                                .size(11.5)
                                .color(MUTED),
                        );
                    });
                    row.col(|ui| {
                        right(ui, |ui| {
                            ui.label(
                                RichText::new(format::money(p.amount, ccy))
                                    .monospace()
                                    .color(GOLD),
                            );
                        })
                    });
                });
            }
        });
}
