//! Shared layout for inspector tables: full width, fixed column boundaries,
//! top-aligned cells, and inputs that fill their cell by default.
use eframe::egui::{self, Rect};

const COLUMN_SPACING: f32 = 8.;
const ROW_HEIGHT: f32 = 26.;

/// Product descriptions share a three-line budget, including status messages.
/// Label supplies one tooltip containing the complete text only when elided.
pub(crate) fn product_cell_text(ui: &mut egui::Ui, lines: Vec<egui::RichText>) {
    let mut job = egui::text::LayoutJob::default();
    for line in lines {
        if !job.text.is_empty() {
            let format = job
                .sections
                .last()
                .map(|section| section.format.clone())
                .unwrap_or_default();
            job.append("\n", 0., format);
        }
        let part = egui::WidgetText::from(line).into_layout_job(
            ui.style(),
            egui::FontSelection::Default,
            egui::Align::Min,
        );
        for section in &part.sections {
            job.append(
                &part.text[section.byte_range.start.0..section.byte_range.end.0],
                section.leading_space,
                section.format.clone(),
            );
        }
    }
    job.wrap.max_rows = 3;
    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.add(egui::Label::new(job).wrap());
    });
}

#[derive(Clone, Copy)]
pub(crate) struct Column {
    minimum: f32,
    weight: f32,
}

impl Column {
    pub(crate) fn flex(weight: f32) -> Self {
        Self {
            minimum: 0.,
            weight,
        }
    }

    pub(crate) fn at_least(width: f32) -> Self {
        Self {
            minimum: width,
            weight: width,
        }
    }

    pub(crate) fn fixed(width: f32) -> Self {
        Self {
            minimum: width,
            weight: 0.,
        }
    }
}

pub(crate) struct InspectorTable<const N: usize> {
    widths: [f32; N],
    row: usize,
    striped: bool,
    column_spacing: f32,
}

impl<const N: usize> InspectorTable<N> {
    pub(crate) fn new(available: f32, columns: [Column; N]) -> Self {
        Self::with_column_spacing(available, columns, COLUMN_SPACING)
    }

    pub(crate) fn with_column_spacing(
        available: f32,
        columns: [Column; N],
        column_spacing: f32,
    ) -> Self {
        let minimum: f32 = columns.iter().map(|column| column.minimum).sum();
        let weight: f32 = columns.iter().map(|column| column.weight).sum();
        let extra = (available - minimum - N.saturating_sub(1) as f32 * column_spacing).max(0.);
        Self {
            widths: columns.map(|column| {
                column.minimum
                    + if weight > 0. {
                        extra * column.weight / weight
                    } else {
                        0.
                    }
            }),
            row: 0,
            striped: false,
            column_spacing,
        }
    }

    pub(crate) fn striped(mut self) -> Self {
        self.striped = true;
        self
    }

    pub(crate) fn header(&mut self, ui: &mut egui::Ui, labels: [&str; N]) {
        self.row(ui, "header", |column, cell| {
            cell.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                // Truncated labels already show their full text on hover in egui.
                ui.add(egui::Label::new(egui::RichText::new(labels[column]).strong()).truncate());
            });
        });
    }

    /// Use one table for every row so headers and body share column boundaries.
    /// Cells may contain several vertically stacked widgets; their height grows
    /// with wrapped content, and the next row starts below the tallest cell.
    pub(crate) fn row(
        &mut self,
        ui: &mut egui::Ui,
        key: impl std::hash::Hash + std::fmt::Debug,
        mut contents: impl FnMut(usize, &mut egui::Ui),
    ) -> Rect {
        let origin = ui.available_rect_before_wrap().min;
        let background = ui.painter().add(egui::Shape::Noop);
        let mut x = origin.x;
        let mut height = ROW_HEIGHT;
        let id = ui.make_persistent_id(key);
        for (column, width) in self.widths.into_iter().enumerate() {
            let rect = Rect::from_min_max(
                egui::pos2(x, origin.y),
                egui::pos2(x + width, ui.max_rect().bottom()),
            );
            let mut cell = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt((id, column))
                    .max_rect(rect)
                    .layout(egui::Layout::top_down_justified(egui::Align::Min)),
            );
            cell.set_width(width);
            cell.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            cell.spacing_mut().interact_size.y = ROW_HEIGHT;
            cell.spacing_mut().text_edit_width = width;
            cell.spacing_mut().combo_width = width;
            let clip = ui.clip_rect();
            cell.set_clip_rect(clip.intersect(Rect::from_min_max(
                egui::pos2(x, clip.top()),
                egui::pos2(x + width, clip.bottom()),
            )));
            contents(column, &mut cell);
            height = height.max(cell.min_rect().height());
            x += width + self.column_spacing;
        }
        let (_, row) = ui.allocate_space(egui::vec2(
            (x - origin.x - self.column_spacing).max(0.),
            height,
        ));
        if self.striped && self.row % 2 == 1 {
            ui.painter().set(
                background,
                egui::Shape::rect_filled(row, 0., ui.visuals().faint_bg_color),
            );
        }
        self.row += 1;
        row
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_text_limits_the_whole_cell_and_keeps_full_tooltip_text() {
        fn check(shape: &egui::Shape, found: &mut bool) {
            match shape {
                egui::Shape::Text(text) => {
                    *found = true;
                    assert!(text.galley.rows.len() <= 3);
                    assert!(text.galley.elided);
                    assert!(!text.galley.job.justify);
                    assert!(text.galley.job.text.ends_with("Final status message"));
                    text.galley.job.debug_sanity_check();
                    for sections in text.galley.job.sections.windows(2) {
                        assert_eq!(sections[0].byte_range.end, sections[1].byte_range.start);
                    }
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        check(shape, found);
                    }
                }
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let mut table = InspectorTable::new(180., [Column::flex(1.)]);
            table.row(ui, "product", |_, cell| {
                product_cell_text(
                    cell,
                    vec![
                        egui::RichText::new("First line\nSecond line\nThird line\nFourth line")
                            .strong(),
                        egui::RichText::new("Final status message").small(),
                    ],
                );
            });
        });
        let mut found = false;
        for shape in output.shapes {
            check(&shape.shape, &mut found);
        }
        assert!(found);
    }

    #[test]
    fn headers_stay_on_one_line_without_justification_or_overflow() {
        fn check(shape: &egui::Shape, count: &mut usize) {
            match shape {
                egui::Shape::Text(text) => {
                    *count += 1;
                    assert_eq!(text.galley.rows.len(), 1);
                    assert!(!text.galley.job.justify);
                    assert_eq!(text.galley.job.wrap.max_rows, 1);
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        check(shape, count);
                    }
                }
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        for width in [180., 280., 640.] {
            let output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.set_width(width);
                let mut table = InspectorTable::new(width, [Column::flex(1.); 3]);
                table.header(
                    ui,
                    ["Material name", "Unit price (CAD)", "Drying / Waiting"],
                );
                let widths = table.widths;
                let right = ui.max_rect().right();
                table.row(ui, "body", |column, cell| {
                    assert!((cell.max_rect().width() - widths[column]).abs() < 0.01);
                    assert!(cell.max_rect().right() <= right + 1.);
                });
            });
            let mut count = 0;
            for shape in output.shapes {
                check(&shape.shape, &mut count);
            }
            assert_eq!(count, 3);
        }
    }

    #[test]
    fn columns_fill_the_viewport_and_keep_catalog_actions_compact() {
        for available in [260., 600., 1200.] {
            let table = InspectorTable::new(
                available,
                [
                    Column::at_least(100.),
                    Column::at_least(66.),
                    Column::fixed(28.),
                ],
            );
            assert_eq!(table.widths[2], 28.);
            assert!(table.widths[0] >= 100.);
            assert!(table.widths[1] >= 66.);
            assert!((table.widths.iter().sum::<f32>() + 16. - available).abs() < 0.01);
        }
        let table = InspectorTable::new(
            120.,
            [
                Column::at_least(100.),
                Column::at_least(66.),
                Column::fixed(28.),
            ],
        );
        assert_eq!(table.widths, [100., 66., 28.]);
    }

    #[test]
    fn rows_align_and_inputs_fill_cells_after_resizing() {
        let ctx = egui::Context::default();
        for width in [280., 640., 360., 1000., 280.] {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.set_width(width);
                let mut table = InspectorTable::new(width, [Column::flex(1.); 3]);
                let mut previous = None;
                let mut column_bounds = Vec::new();
                for row_index in 0..4 {
                    let mut cells = Vec::new();
                    let row = table.row(ui, row_index, |column, cell| {
                        let bounds = cell.max_rect();
                        let response = match row_index {
                            0 => cell.add(
                                egui::Label::new("Wrapped column heading with details").wrap(),
                            ),
                            1 => cell.add(egui::TextEdit::singleline(&mut "Board".to_owned())),
                            2 => cell.add(egui::DragValue::new(&mut 12.5)),
                            _ => {
                                egui::ComboBox::from_id_salt("choice")
                                    .selected_text("Choice")
                                    .show_ui(cell, |ui| {
                                        ui.label("Choice");
                                    })
                                    .response
                            }
                        };
                        assert!((response.rect.left() - bounds.left()).abs() <= 1.);
                        assert!((response.rect.right() - bounds.right()).abs() <= 1.);
                        assert!((response.rect.top() - bounds.top()).abs() <= 1.);
                        if row_index == 0 {
                            column_bounds.push((bounds.left(), bounds.right()));
                        } else {
                            assert_eq!((bounds.left(), bounds.right()), column_bounds[column]);
                        }
                        cells.push(cell.min_rect());
                    });
                    assert!((row.width() - width).abs() <= 1.);
                    for cell in cells {
                        assert!(cell.bottom() <= row.bottom() + 1.);
                        assert!((cell.top() - row.top()).abs() <= 1.);
                    }
                    if let Some(previous) = previous {
                        assert!(row.top() >= previous);
                    }
                    previous = Some(row.bottom());
                }
            });
        }
    }

    #[test]
    fn disabled_inputs_keep_the_same_column_width() {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_width(400.);
            let mut table = InspectorTable::new(400., [Column::flex(1.); 2]);
            table.row(ui, "disabled", |_, cell| {
                let bounds = cell.max_rect();
                let response = cell
                    .add_enabled_ui(false, |ui| ui.add(egui::DragValue::new(&mut 5.)))
                    .inner;
                assert!(!response.enabled());
                assert!((response.rect.width() - bounds.width()).abs() <= 1.);
            });
        });
    }
}
