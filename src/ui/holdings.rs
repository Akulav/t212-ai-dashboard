//! Holdings page: KPI row + a sortable, searchable positions table with
//! fixed columns, right-aligned numbers and uniform row height.

use eframe::egui::{self, Align, Layout, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as ph;

use super::dashboard::{short_ticker, truncate, weight_bar};
use super::theme::{palette::*, semibold};
use super::widgets::{self, GAP, Kpi, card, card_header, chip, kpi_row};
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
    /// Portfolio weight.
    Weight,
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

/// Right-aligned cell content.
pub fn right(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.with_layout(Layout::right_to_left(Align::Center), add);
}

impl HoldingsPage {
    /// Renders the page.
    pub fn show(&mut self, ui: &mut Ui, portfolio: &PortfolioService, state: &PortfolioState) {
        portfolio_header(
            ui,
            "Holdings",
            "Positions, cost basis, P/L and income per holding",
            portfolio,
            state,
        );
        ui.add_space(20.0);
        let Some(view) = state.view.as_deref() else {
            widgets::empty_state(
                ui,
                ph::BRIEFCASE,
                "No data yet",
                "Connect Trading212 in Settings to see your positions.",
            );
            return;
        };
        let a = &view.analytics;
        let ccy = view.summary.currency.as_str();
        let inv = &view.summary.investments;
        let upct = (inv.total_cost > 0.0).then(|| inv.unrealized_profit_loss / inv.total_cost);
        kpi_row(
            ui,
            "holdings-kpis",
            &[
                Kpi::new(
                    ph::STACK,
                    "Invested value",
                    format::money(inv.current_value, ccy),
                    format!("{} positions", a.holdings.len()),
                    ACCENT,
                ),
                Kpi::new(
                    ph::TAG,
                    "Cost basis",
                    format::money(inv.total_cost, ccy),
                    "open positions",
                    SKY,
                ),
                Kpi::new(
                    ph::TREND_UP,
                    "Unrealised P/L",
                    format::signed_money(inv.unrealized_profit_loss, ccy),
                    upct.map_or_else(String::new, format::signed_pct),
                    format::pnl_color(inv.unrealized_profit_loss),
                )
                .value_color(format::pnl_color(inv.unrealized_profit_loss)),
                Kpi::new(
                    ph::CHECK_CIRCLE,
                    "Realised P/L",
                    format::signed_money(inv.realized_profit_loss, ccy),
                    "lifetime, closed trades",
                    format::pnl_color(inv.realized_profit_loss),
                )
                .value_color(format::pnl_color(inv.realized_profit_loss)),
            ],
        );
        ui.add_space(GAP);

        card(ui, |ui| {
            ui.horizontal(|ui| {
                card_header(ui, ph::LIST_BULLETS, "Positions", "");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text(format!(
                                "{}  Search ticker, name or ISIN",
                                ph::MAGNIFYING_GLASS
                            ))
                            .desired_width(280.0),
                    );
                });
            });
            ui.add_space(4.0);

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
                    SortBy::Weight => x.weight.total_cmp(&y.weight),
                };
                if self.descending { ord.reverse() } else { ord }
            });

            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            TableBuilder::new(ui)
                .id_salt("holdings-table")
                .striped(true)
                .vscroll(false)
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::remainder().at_least(220.0))
                .column(Column::exact(80.0))
                .column(Column::exact(120.0))
                .column(Column::exact(120.0))
                .column(Column::exact(115.0))
                .column(Column::exact(85.0))
                .column(Column::exact(110.0))
                .column(Column::exact(110.0))
                .header(32.0, |mut h| {
                    h.col(|ui| self.header(ui, "Instrument", Some(SortBy::Name), false));
                    h.col(|ui| self.header(ui, "Shares", None, true));
                    h.col(|ui| self.header(ui, "Avg · Last", None, true));
                    h.col(|ui| self.header(ui, "Value", Some(SortBy::Value), true));
                    h.col(|ui| self.header(ui, "P/L", Some(SortBy::Pnl), true));
                    h.col(|ui| self.header(ui, "P/L %", Some(SortBy::PnlPct), true));
                    h.col(|ui| self.header(ui, "Dividends", Some(SortBy::Dividends), true));
                    h.col(|ui| self.header(ui, "Weight", Some(SortBy::Weight), true));
                })
                .body(|mut body| {
                    for h in rows {
                        body.row(52.0, |mut row| {
                            row.col(|ui| {
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 2.0;
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(short_ticker(&h.ticker))
                                                .font(semibold(13.5)),
                                        );
                                        if h.quote_currency != ccy && !h.quote_currency.is_empty() {
                                            chip(ui, &h.quote_currency, VIOLET);
                                        }
                                    });
                                    ui.label(
                                        RichText::new(truncate(&h.name, 40))
                                            .size(11.5)
                                            .color(MUTED),
                                    );
                                });
                            });
                            row.col(|ui| {
                                right(ui, |ui| {
                                    ui.label(
                                        RichText::new(format::quantity(h.quantity)).monospace(),
                                    );
                                })
                            });
                            row.col(|ui| {
                                right(ui, |ui| {
                                    ui.vertical(|ui| {
                                        ui.with_layout(Layout::top_down(Align::Max), |ui| {
                                            ui.spacing_mut().item_spacing.y = 2.0;
                                            ui.label(
                                                RichText::new(format::money(
                                                    h.current_price,
                                                    &h.quote_currency,
                                                ))
                                                .monospace(),
                                            );
                                            ui.label(
                                                RichText::new(format::money(
                                                    h.avg_price,
                                                    &h.quote_currency,
                                                ))
                                                .monospace()
                                                .size(11.5)
                                                .color(MUTED),
                                            );
                                        });
                                    });
                                })
                            });
                            row.col(|ui| {
                                right(ui, |ui| {
                                    ui.label(
                                        RichText::new(format::money(h.value, ccy))
                                            .monospace()
                                            .strong(),
                                    );
                                })
                            });
                            row.col(|ui| {
                                right(ui, |ui| {
                                    let l = ui.label(
                                        RichText::new(format::signed_money(h.pnl, ccy))
                                            .monospace()
                                            .color(format::pnl_color(h.pnl)),
                                    );
                                    if let Some(fx) = h.fx_impact {
                                        l.on_hover_text(format!(
                                            "of which FX: {}",
                                            format::signed_money(fx, ccy)
                                        ));
                                    }
                                })
                            });
                            row.col(|ui| {
                                right(ui, |ui| match h.pnl_pct {
                                    Some(p) => {
                                        ui.label(
                                            RichText::new(format::signed_pct(p))
                                                .monospace()
                                                .color(format::pnl_color(p)),
                                        );
                                    }
                                    None => {
                                        ui.label(RichText::new("—").color(MUTED));
                                    }
                                })
                            });
                            row.col(|ui| {
                                right(ui, |ui| {
                                    ui.vertical(|ui| {
                                        ui.with_layout(Layout::top_down(Align::Max), |ui| {
                                            ui.spacing_mut().item_spacing.y = 2.0;
                                            ui.label(
                                                RichText::new(format::money(
                                                    h.dividends_received,
                                                    ccy,
                                                ))
                                                .monospace(),
                                            );
                                            if let Some(y) = h.yield_on_cost {
                                                ui.label(
                                                    RichText::new(format!(
                                                        "YoC {}",
                                                        format::pct(y)
                                                    ))
                                                    .size(11.5)
                                                    .color(GOLD),
                                                );
                                            }
                                        });
                                    });
                                })
                            });
                            row.col(|ui| {
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 4.0;
                                    right(ui, |ui| {
                                        ui.label(
                                            RichText::new(format::pct(h.weight))
                                                .monospace()
                                                .size(12.5),
                                        );
                                    });
                                    weight_bar(
                                        ui,
                                        h.weight / max_weight(a.holdings.as_slice()),
                                        ACCENT.linear_multiply(0.85),
                                    );
                                });
                            });
                        });
                    }
                });
        });
    }

    fn header(&mut self, ui: &mut Ui, text: &str, sort: Option<SortBy>, align_right: bool) {
        let active = sort.is_some_and(|s| s == self.sort);
        let arrow = if active {
            if self.descending {
                ph::CARET_DOWN
            } else {
                ph::CARET_UP
            }
        } else {
            ""
        };
        let label = RichText::new(format!("{text} {arrow}").trim_end().to_uppercase())
            .font(semibold(11.0))
            .color(if active { TEXT } else { MUTED });
        let mut draw = |ui: &mut Ui| {
            let resp = ui.add(egui::Label::new(label.clone()).sense(egui::Sense::click()));
            if let Some(s) = sort {
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                if resp.clicked() {
                    if self.sort == s {
                        self.descending = !self.descending;
                    } else {
                        self.sort = s;
                        self.descending = s != SortBy::Name;
                    }
                }
            }
        };
        if align_right {
            right(ui, |ui| draw(ui))
        } else {
            draw(ui)
        }
    }
}

fn max_weight(rows: &[HoldingRow]) -> f64 {
    rows.iter()
        .map(|h| h.weight)
        .fold(0.0, f64::max)
        .max(f64::EPSILON)
}
