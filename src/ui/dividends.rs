//! Dividends page: income KPIs, monthly chart, projected calendar, top
//! payers and payment history.

use eframe::egui::{self, Align, Layout, RichText, Ui};

use super::dashboard::{short_ticker, truncate};
use super::theme::palette::*;
use super::widgets::{self, card, kpi};
use super::{charts, format, portfolio_header};
use crate::portfolio::{PortfolioService, PortfolioState};

/// Renders the page.
pub fn show(ui: &mut Ui, portfolio: &PortfolioService, state: &PortfolioState) {
    portfolio_header(ui, "Dividends", portfolio, state);
    ui.add_space(12.0);
    let Some(view) = state.view.as_deref() else {
        widgets::empty_state(
            ui,
            "💰",
            "No data yet",
            "Connect Trading212 in Settings to see your dividend income.",
        );
        return;
    };
    let ccy = view.summary.currency.as_str();
    let d = &view.analytics.dividends;
    let projected_90: f64 = {
        let cutoff = chrono::Utc::now().date_naive() + chrono::Duration::days(90);
        view.analytics
            .upcoming
            .iter()
            .filter(|u| u.expected_on <= cutoff)
            .map(|u| u.amount)
            .sum()
    };

    ui.columns(4, |c| {
        kpi(
            &mut c[0],
            "💰",
            "Total received",
            &format::money(d.total, ccy),
            &format!("{} payments", d.payments),
            GOLD,
        );
        kpi(
            &mut c[1],
            "📅",
            "Last 12 months",
            &format::money(d.last_12m, ccy),
            &format!("YTD {}", format::money(d.ytd, ccy)),
            ACCENT,
        );
        let (rate, caption) = match d.tax.rate {
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
        kpi(
            &mut c[2],
            "🧾",
            "Avg. tax withheld",
            &rate,
            &caption,
            VIOLET,
        );
        kpi(
            &mut c[3],
            "🔮",
            "Next 90 days",
            &format!("≈{}", format::money(projected_90, ccy)),
            "projected from history",
            POSITIVE,
        );
    });
    ui.add_space(12.0);

    card(ui, |ui| {
        widgets::section_title(ui, "Monthly income · last 24 months");
        ui.add_space(6.0);
        charts::monthly_dividends(ui, &d.by_month, ccy, 220.0);
    });
    ui.add_space(12.0);

    ui.columns(2, |c| {
        card(&mut c[0], |ui| {
            widgets::section_title(ui, "Projected calendar (next 120 days)");
            ui.label(
                RichText::new("Trading212 doesn't publish upcoming dividends; these are estimated from each holding's payment cadence.")
                    .size(11.5)
                    .color(MUTED),
            );
            ui.add_space(6.0);
            if view.analytics.upcoming.is_empty() {
                ui.label(RichText::new("Nothing projected yet.").color(MUTED));
            }
            egui::Grid::new("upcoming_grid").num_columns(4).spacing([14.0, 6.0]).striped(true).show(ui, |ui| {
                for u in &view.analytics.upcoming {
                    ui.label(RichText::new(u.expected_on.format("%a %d %b").to_string()).monospace().color(GOLD));
                    ui.label(RichText::new(short_ticker(&u.ticker)).strong().monospace());
                    ui.label(RichText::new(u.cadence.label()).size(11.5).color(MUTED));
                    ui.label(RichText::new(format!("≈{}", format::money(u.amount, ccy))).monospace());
                    ui.end_row();
                }
            });
        });
        card(&mut c[1], |ui| {
            widgets::section_title(ui, "Top payers (lifetime)");
            ui.add_space(6.0);
            let top = d.by_ticker.first().map_or(0.0, |t| t.2).max(f64::EPSILON);
            for (ticker, name, total) in d.by_ticker.iter().take(10) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(short_ticker(ticker)).strong().monospace());
                    ui.label(RichText::new(truncate(name, 26)).color(MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format::money(*total, ccy)).monospace());
                    });
                });
                let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 4.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, egui::CornerRadius::same(2), SURFACE_2);
                let mut fill = rect;
                fill.set_width(rect.width() * (*total / top).clamp(0.0, 1.0) as f32);
                ui.painter().rect_filled(fill, egui::CornerRadius::same(2), GOLD.linear_multiply(0.8));
            }
        });
    });
    ui.add_space(12.0);

    card(ui, |ui| {
        widgets::section_title(
            ui,
            &format!("Payment history · {} total", view.dividend_count),
        );
        ui.add_space(6.0);
        egui::Grid::new("dividend_history")
            .num_columns(6)
            .spacing([18.0, 6.0])
            .striped(true)
            .show(ui, |ui| {
                for h in [
                    "Paid",
                    "Instrument",
                    "Shares",
                    "Gross / share",
                    "Type",
                    "Net",
                ] {
                    ui.label(RichText::new(h).size(12.0).color(MUTED).strong());
                }
                ui.end_row();
                for p in view.recent_dividends.iter().take(100) {
                    ui.label(
                        RichText::new(
                            p.paid_on
                                .map_or_else(String::new, |t| t.format("%d %b %Y").to_string()),
                        )
                        .monospace(),
                    );
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(short_ticker(&p.ticker)).strong().monospace());
                        ui.label(
                            RichText::new(truncate(&p.instrument.name, 24))
                                .size(12.0)
                                .color(MUTED),
                        );
                    });
                    ui.label(RichText::new(format::quantity(p.quantity)).monospace());
                    ui.label(
                        RichText::new(format::money(p.gross_amount_per_share, &p.ticker_currency))
                            .monospace(),
                    );
                    ui.label(
                        RichText::new(p.kind.replace('_', " ").to_lowercase())
                            .size(11.5)
                            .color(MUTED),
                    );
                    ui.label(
                        RichText::new(format::money(p.amount, ccy))
                            .monospace()
                            .color(GOLD),
                    );
                    ui.end_row();
                }
            });
    });
}
