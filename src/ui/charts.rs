//! `egui_plot` charts: portfolio value history and monthly dividend income.

use chrono::{DateTime, TimeZone, Utc};
use eframe::egui::Ui;
use egui_plot::{Bar, BarChart, Corner, Legend, Line, Plot, PlotPoints};

use super::format;
use super::theme::palette::*;
use crate::portfolio::store::Snapshot;

const DAY: f64 = 86_400.0;

fn to_x(t: DateTime<Utc>) -> f64 {
    t.timestamp() as f64 / DAY
}

fn date_label(x: f64) -> String {
    Utc.timestamp_opt((x * DAY) as i64, 0)
        .single()
        .map_or_else(String::new, |d| d.format("%d %b %y").to_string())
}

/// Account value (from local snapshots) vs. cumulative net deposits.
pub fn value_history(
    ui: &mut Ui,
    snapshots: &[Snapshot],
    deposits: &[(DateTime<Utc>, f64)],
    ccy: &str,
    height: f32,
) {
    let ccy_axis = ccy.to_string();
    let ccy_label = ccy.to_string();
    Plot::new("value_history")
        .height(height)
        .legend(Legend::default().position(Corner::LeftTop))
        .allow_scroll(false)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_boxed_zoom(false)
        .show_background(false)
        .show_x(false)
        .show_y(false)
        // Grid every ~month / quarter / year; label only quarter-or-larger
        // marks so dates never collide.
        .x_grid_spacer(egui_plot::uniform_grid_spacer(|_| {
            [30.4375, 91.3125, 365.25]
        }))
        .x_axis_formatter(|mark, _| {
            if mark.step_size < 80.0 {
                String::new()
            } else {
                // Uniform marks land within a day or two of quarter starts.
                Utc.timestamp_opt(((mark.value + 3.0) * DAY) as i64, 0)
                    .single()
                    .map_or_else(String::new, |d| d.format("%b %y").to_string())
            }
        })
        .y_axis_formatter(move |mark, _| format::compact_money(mark.value, &ccy_axis))
        .label_formatter(move |name, p| {
            if name.is_empty() {
                String::new()
            } else {
                format!(
                    "{name}\n{}\n{}",
                    date_label(p.x),
                    format::money(p.y, &ccy_label)
                )
            }
        })
        .show(ui, |plot| {
            if !deposits.is_empty() {
                // Step line: hold each cumulative value until the next flow, then to "now".
                let mut pts: Vec<[f64; 2]> = Vec::with_capacity(deposits.len() * 2 + 1);
                for (i, (t, v)) in deposits.iter().enumerate() {
                    if i > 0 {
                        pts.push([to_x(*t), deposits[i - 1].1]);
                    }
                    pts.push([to_x(*t), *v]);
                }
                pts.push([to_x(Utc::now()), deposits.last().map_or(0.0, |d| d.1)]);
                plot.line(
                    Line::new("Net deposits", PlotPoints::from(pts))
                        .color(MUTED)
                        .width(1.5_f32),
                );
            }
            if !snapshots.is_empty() {
                let pts: Vec<[f64; 2]> = snapshots
                    .iter()
                    .map(|s| [to_x(s.at), s.total_value])
                    .collect();
                plot.line(
                    Line::new("Account value", PlotPoints::from(pts))
                        .color(ACCENT)
                        .width(2.2_f32)
                        .fill(0.0_f32),
                );
            }
        });
}

/// Monthly net dividend income bars, `(year, month, amount)` oldest first.
pub fn monthly_dividends(ui: &mut Ui, months: &[(i32, u32, f64)], ccy: &str, height: f32) {
    let labels: Vec<String> = months
        .iter()
        .map(|(y, m, _)| {
            chrono::NaiveDate::from_ymd_opt(*y, *m, 1)
                .map_or_else(String::new, |d| d.format("%b %y").to_string())
        })
        .collect();
    let bars: Vec<Bar> = months
        .iter()
        .enumerate()
        .map(|(i, (_, _, v))| {
            Bar::new(i as f64, *v)
                .width(0.7)
                .fill(GOLD.linear_multiply(0.85))
                .name(&labels[i])
        })
        .collect();
    let axis_labels = labels.clone();
    let ccy_axis = ccy.to_string();
    let ccy_label = ccy.to_string();
    Plot::new("monthly_dividends")
        .height(height)
        .allow_scroll(false)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_boxed_zoom(false)
        .show_background(false)
        .show_x(false)
        .show_y(false)
        .x_axis_formatter(move |mark, _| {
            let i = mark.value.round();
            if (mark.value - i).abs() < 1e-6 && i >= 0.0 {
                axis_labels.get(i as usize).cloned().unwrap_or_default()
            } else {
                String::new()
            }
        })
        .y_axis_formatter(move |mark, _| format::compact_money(mark.value, &ccy_axis))
        .show(ui, |plot| {
            plot.bar_chart(BarChart::new("Dividends", bars).element_formatter(Box::new(
                move |bar, _| format!("{}\n{}", bar.name, format::money(bar.value, &ccy_label)),
            )));
        });
}
