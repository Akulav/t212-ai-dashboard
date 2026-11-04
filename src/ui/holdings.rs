//! Holdings page: sortable, searchable positions table.

use eframe::egui::{self, RichText, Ui};

use super::dashboard::{short_ticker, truncate, weight_bar};
use super::theme::palette::*;
use super::widgets::{self, card, chip};
use super::{format, portfolio_header};
use crate::portfolio::analytics::HoldingRow;
use crate::portfolio::{PortfolioService, PortfolioState};

/// Sortable columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortBy {
    /// Instrument name.
    Name,
    /// Market value.
    Value,
    /// Unrealised P/L in money.
    Pnl,
    /// Unrealised P/L in percent.
    PnlPct,
    /// Lifetime dividends.
    Dividends,
}

/// Persistent page state.
pub struct HoldingsPage {
    search: String,
    sort: SortBy,
    descending: bool,
}

impl Default for HoldingsPage {
    fn default() -> Self {
        Self {
            search: String::new(),
            sort: SortBy::Value,
            descending: true,
        }
    }
}

impl HoldingsPage {
    /// Renders the page.
    pub fn show(&mut self, ui: &mut Ui, portfolio: &PortfolioService, state: &PortfolioState) {
        portfolio_header(ui, "Holdings", portfolio, state);
        ui.add_space(12.0);
        let Some(view) = state.view.as_deref() else {
            widgets::empty_state(
                ui,
                "💼",
                "No data yet",
                "Connect Trading212 in Settings to see your positions.",
            );
            return;
        };
        let a = &view.analytics;
        let ccy = view.summary.currency.as_str();

        let inv = &view.summary.investments;
        ui.columns(4, |c| {
            widgets::kpi(
                &mut c[0],
                "📦",
                "Invested value",
                &format::money(inv.current_value, ccy),
                &format!("{} positions", a.holdings.len()),
                ACCENT,
            );
            widgets::kpi(
                &mut c[1],
                "🧮",
                "Cost basis",
                &format::money(inv.total_cost, ccy),
                "open positions",
                MUTED,
            );
            let upct = (inv.total_cost > 0.0).then(|| inv.unrealized_profit_loss / inv.total_cost);
            widgets::kpi(
                &mut c[2],
                "📈",
                "Unrealised P/L",
                &format::signed_money(inv.unrealized_profit_loss, ccy),
                &upct.map_or_else(String::new, format::signed_pct),
                format::pnl_color(inv.unrealized_profit_loss),
            );
            widgets::kpi(
                &mut c[3],
                "✅",
                "Realised P/L",
                &format::signed_money(inv.realized_profit_loss, ccy),
                "lifetime, closed trades",
                format::pnl_color(inv.realized_profit_loss),
            );
        });
        ui.add_space(12.0);

        card(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::section_title(ui, "Positions");
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("🔍 Search ticker, name or ISIN")
                        .desired_width(260.0),
                );
            });
            ui.add_space(6.0);

            let needle = self.search.to_lowercase();
            let mut rows: Vec<&HoldingRow> = a
                .holdings
                .iter()
                .filter(|h| {
                    needle.is_empty()
                        || h.ticker.to_lowercase().contains(&needle)
                        || h.name.to_lowercase().contains(&needle)
                        || h.isin.to_lowercase().contains(&needle)
                })
                .collect();
            rows.sort_by(|x, y| {
                let ord = match self.sort {
                    SortBy::Name => x.name.cmp(&y.name),
                    SortBy::Value => x.value.total_cmp(&y.value),
                    SortBy::Pnl => x.pnl.total_cmp(&y.pnl),
                    SortBy::PnlPct => x
                        .pnl_pct
                        .unwrap_or(0.0)
                        .total_cmp(&y.pnl_pct.unwrap_or(0.0)),
                    SortBy::Dividends => x.dividends_received.total_cmp(&y.dividends_received),
                };
                if self.descending { ord.reverse() } else { ord }
            });

            egui::Grid::new("holdings_grid")
                .num_columns(8)
                .spacing([18.0, 8.0])
                .striped(true)
                .show(ui, |ui| {
                    self.header(ui, "Instrument", Some(SortBy::Name));
                    self.header(ui, "Shares", None);
                    self.header(ui, "Avg / Last", None);
                    self.header(ui, "Value", Some(SortBy::Value));
                    self.header(ui, "P/L", Some(SortBy::Pnl));
                    self.header(ui, "P/L %", Some(SortBy::PnlPct));
                    self.header(ui, "Dividends", Some(SortBy::Dividends));
                    self.header(ui, "Weight", None);
                    ui.end_row();

                    for h in rows {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(short_ticker(&h.ticker)).strong().monospace(),
                                );
                                if h.quote_currency != ccy && !h.quote_currency.is_empty() {
                                    chip(ui, &h.quote_currency, VIOLET);
                                }
                            });
                            ui.label(RichText::new(truncate(&h.name, 34)).size(12.0).color(MUTED));
                        });
                        ui.label(RichText::new(format::quantity(h.quantity)).monospace());
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(format::money(h.avg_price, &h.quote_currency))
                                    .size(12.0)
                                    .color(MUTED)
                                    .monospace(),
                            );
                            ui.label(
                                RichText::new(format::money(h.current_price, &h.quote_currency))
                                    .monospace(),
                            );
                        });
                        ui.label(
                            RichText::new(format::money(h.value, ccy))
                                .monospace()
                                .strong(),
                        );
                        let pnl_label = ui.label(
                            RichText::new(format::signed_money(h.pnl, ccy))
                                .monospace()
                                .color(format::pnl_color(h.pnl)),
                        );
                        if let Some(fx) = h.fx_impact {
                            pnl_label.on_hover_text(format!(
                                "of which FX: {}",
                                format::signed_money(fx, ccy)
                            ));
                        }
                        match h.pnl_pct {
                            Some(p) => ui.label(
                                RichText::new(format::signed_pct(p))
                                    .monospace()
                                    .color(format::pnl_color(p)),
                            ),
                            None => ui.label("—"),
                        };
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(format::money(h.dividends_received, ccy)).monospace(),
                            );
                            if let Some(y) = h.yield_on_cost {
                                ui.label(
                                    RichText::new(format!("YoC {}", format::pct(y)))
                                        .size(11.5)
                                        .color(GOLD),
                                );
                            }
                        });
                        ui.vertical(|ui| {
                            ui.set_width(90.0);
                            ui.label(RichText::new(format::pct(h.weight)).size(12.0).monospace());
                            weight_bar(ui, h.weight);
                        });
                        ui.end_row();
                    }
                });
        });
    }

    fn header(&mut self, ui: &mut Ui, text: &str, sort: Option<SortBy>) {
        let Some(sort) = sort else {
            ui.label(RichText::new(text).size(12.0).color(MUTED).strong());
            return;
        };
        let arrow = if self.sort == sort {
            if self.descending { " ⏷" } else { " ⏶" }
        } else {
            ""
        };
        let color = if self.sort == sort { TEXT } else { MUTED };
        if ui
            .add(
                egui::Button::new(
                    RichText::new(format!("{text}{arrow}"))
                        .size(12.0)
                        .color(color)
                        .strong(),
                )
                .frame(false),
            )
            .clicked()
        {
            if self.sort == sort {
                self.descending = !self.descending;
            } else {
                self.sort = sort;
                self.descending = sort != SortBy::Name;
            }
        }
    }
}
