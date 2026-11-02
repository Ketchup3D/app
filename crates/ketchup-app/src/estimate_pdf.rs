//! Printable, selectable-text estimate reports. Shared by native and WASM exports.
use eframe::egui;
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
};
use std::{collections::BTreeMap, fmt::Write};

#[derive(Default)]
pub(crate) struct Report {
    pub name: String,
    pub summary: [String; 3],
    pub notes: Vec<String>,
    pub costs: Vec<Group>,
    pub tasks: Vec<[String; 3]>,
    pub totals: [String; 3],
    pub currency: String,
}
pub(crate) struct Group {
    pub title: String,
    pub rows: Vec<[String; 4]>,
}

const LEFT: f64 = 42.;
const RIGHT: f64 = 553.;
const BOTTOM: f64 = 55.;
const INK: &str = "0.15 0.20 0.17";
const BORDER: &str = "0.73 0.79 0.74";

struct Pdf<'a> {
    font: FontRef<'a>,
    fallback: FontRef<'a>,
    chars: BTreeMap<char, (u16, u16, f64)>,
    pages: Vec<String>,
    page: String,
    header_name: String,
    header_date: String,
    y: f64,
}
impl<'a> Pdf<'a> {
    fn glyph(&self, ch: char) -> (u8, u16, f64) {
        let (index, font) = if self.font.charmap().map(ch).is_some() {
            (1, &self.font)
        } else {
            (2, &self.fallback)
        };
        let glyph = font.charmap().map(ch).unwrap_or_default();
        let width = font
            .glyph_metrics(Size::new(1000.), LocationRef::default())
            .advance_width(glyph)
            .unwrap_or(500.);
        (index, glyph.to_u32() as u16, f64::from(width))
    }
    fn width(&self, text: &str, size: f64) -> f64 {
        text.chars().map(|ch| self.glyph(ch).2 * size / 1000.).sum()
    }
    fn text(&mut self, x: f64, y: f64, size: f64, text: &str) {
        self.text_color(x, y, size, text, INK);
    }
    fn bold_text_color(&mut self, x: f64, y: f64, size: f64, text: &str, color: &str) {
        // Fill and stroke the embedded light font to give emphasized text bold weight.
        let _ = writeln!(self.page, "q {color} RG 0.45 w 2 Tr");
        self.text_color(x, y, size, text, color);
        self.page.push_str("Q\n");
    }
    fn text_color(&mut self, mut x: f64, y: f64, size: f64, text: &str, color: &str) {
        for ch in text.chars().filter(|ch| !ch.is_control()) {
            let (font, gid, width) = self.glyph(ch);
            let next = self.chars.len() as u16 + 1;
            let (cid, _, _) = *self.chars.entry(ch).or_insert((next, gid, width));
            let _ = writeln!(
                self.page,
                "{color} rg BT /F{font} {size} Tf 1 0 0 1 {x:.3} {y:.3} Tm <{cid:04X}> Tj ET"
            );
            x += width * size / 1000.;
        }
    }
    fn panel(&mut self, x: f64, bottom: f64, width: f64, height: f64, fill: &str) {
        let _ = writeln!(
            self.page,
            "q {fill} rg {BORDER} RG 0.6 w {x:.2} {bottom:.2} {width:.2} {height:.2} re B Q"
        );
    }
    fn table_panel(&mut self, top: f64, bottom: f64, widths: &[f64], fill: &str) {
        self.panel(LEFT, bottom, RIGHT - LEFT, top - bottom, fill);
        let mut x = LEFT;
        for width in widths.iter().take(widths.len().saturating_sub(1)) {
            x += width;
            let _ = writeln!(
                self.page,
                "q {BORDER} RG 0.4 w {x:.2} {bottom:.2} m {x:.2} {top:.2} l S Q"
            );
        }
    }
    fn group_heading(&mut self, title: &str) {
        let lines = self.wrap(title, RIGHT - LEFT - 20., 11.);
        let height = lines.len() as f64 * 16. + 12.;
        self.ensure(height + 65.);
        let top = self.y + 10.;
        let bottom = top - height;
        self.panel(LEFT, bottom, RIGHT - LEFT, height, "0.88 0.92 0.89");
        for (index, line) in lines.iter().enumerate() {
            self.text(LEFT + 10., top - 18. - index as f64 * 16., 11., line);
        }
        self.y = bottom - 14.;
    }
    fn new_page(&mut self) {
        if !self.page.is_empty() {
            self.pages.push(std::mem::take(&mut self.page));
        }
        let header=crate::pdf_header::layout(&self.header_name,"Time & Cost Estimates",&self.header_date,[595.,842.],|text,size|self.width(text,size));
        self.page.push_str(&header.graphics);
        for text in header.text {self.text(text.x,text.y,text.size,&text.value);}
        self.y=header.body_top;
    }
    fn ensure(&mut self, height: f64) {
        if self.y - height < BOTTOM {
            self.new_page();
        }
    }
    fn wrap(&self, text: &str, width: f64, size: f64) -> Vec<String> {
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            let mut line = String::new();
            // Measure actual font advances; very long names also break safely.
            for word in paragraph.split_whitespace() {
                let candidate = if line.is_empty() {
                    word.into()
                } else {
                    format!("{line} {word}")
                };
                if self.width(&candidate, size) <= width {
                    line = candidate;
                    continue;
                }
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                for ch in word.chars() {
                    let candidate = format!("{line}{ch}");
                    if !line.is_empty() && self.width(&candidate, size) > width {
                        lines.push(std::mem::take(&mut line));
                    }
                    line.push(ch);
                }
            }
            lines.push(line);
        }
        lines
    }
    fn paragraph(&mut self, text: &str, size: f64) {
        for line in self.wrap(text, RIGHT - LEFT, size) {
            self.ensure(size + 6.);
            self.text(LEFT, self.y, size, &line);
            self.y -= size + 5.;
        }
        self.y -= 5.;
    }
    fn note(&mut self, text: &str) {
        self.paragraph(text, 9.);
        self.y -= 7.;
    }
    fn heading(&mut self, title: &str) {
        self.ensure(75.);
        self.y -= 6.;
        self.bold_text_color(LEFT, self.y, 14., title, INK);
        // Leave every section's content top the same distance below its title.
        self.y -= 26.;
    }
    fn table_header(&mut self, headers: &[String], widths: &[f64]) {
        let bottom = self.y - 10.;
        self.table_panel(self.y + 14., bottom, widths, "0.92 0.94 0.92");
        let mut x = LEFT;
        for (header, width) in headers.iter().zip(widths) {
            self.text(x + 6., self.y, 8., header);
            x += width;
        }
        self.y = bottom - 9.;
    }
    fn table_row(
        &mut self,
        cells: &[String],
        widths: &[f64],
        headers: &[String],
        group: &str,
        stripe: bool,
    ) {
        let lines: Vec<_> = cells
            .iter()
            .zip(widths)
            .map(|(cell, width)| self.wrap(cell, width - 12., 9.))
            .collect();
        let count = lines.iter().map(Vec::len).max().unwrap_or(1);
        let mut start = 0;
        while start < count {
            if self.y - 25. < BOTTOM {
                self.new_page();
                self.group_heading(&format!("{group} (continued)"));
                self.table_header(headers, widths);
            }
            let available = (((self.y - BOTTOM - 10.) / 13.).floor() as usize).max(1);
            let end = count.min(start + available);
            let height = (end - start) as f64 * 13. + 9.;
            let fill = if cells.first().is_some_and(|cell| cell == "Total") {
                "0.88 0.92 0.89"
            } else if stripe {
                "0.97 0.98 0.97"
            } else {
                "1 1 1"
            };
            self.table_panel(self.y + 9., self.y - height + 9., widths, fill);
            let mut x = LEFT;
            for (cell, width) in lines.iter().zip(widths) {
                let visible = cell.len().min(end).saturating_sub(start);
                let offset = (end - start - visible) as f64 * 6.5;
                for index in start..end {
                    if let Some(line) = cell.get(index) {
                        self.text(
                            x + 6.,
                            self.y - 4.5 - offset - (index - start) as f64 * 13.,
                            9.,
                            line,
                        );
                    }
                }
                x += width;
            }
            self.y -= height;
            start = end;
        }
    }
    fn finish(mut self, font_data: &[u8], fallback_data: &[u8]) -> Result<Vec<u8>, String> {
        self.pages.push(std::mem::take(&mut self.page));
        let count = self.pages.len();
        // Add page numbers before constructing the shared font's character map.
        for index in 0..count {
            self.page.clear();
            self.text(
                LEFT,
                29.,
                8.,
                "KetchUp - Planning estimate, verify before purchasing",
            );
            self.text(480., 29., 8., &format!("{} / {count}", index + 1));
            self.pages[index].push_str(&self.page);
        }
        let mut cids = vec![(0_u16, 0_f64); self.chars.len() + 1];
        let mut mappings = Vec::new();
        for (ch, (cid, gid, width)) in &self.chars {
            cids[*cid as usize] = (*gid, *width);
            let utf16: String = ch
                .encode_utf16(&mut [0; 2])
                .iter()
                .map(|unit| format!("{unit:04X}"))
                .collect();
            mappings.push(format!("<{cid:04X}> <{utf16}>"));
        }
        let mut cmap = "/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /KetchUpUnicode def /CMapType 2 def 1 begincodespacerange <0000> <FFFF> endcodespacerange\n".to_owned();
        for chunk in mappings.chunks(100) {
            let _ = writeln!(
                cmap,
                "{} beginbfchar\n{}\nendbfchar",
                chunk.len(),
                chunk.join("\n")
            );
        }
        cmap.push_str("endcmap CMapName currentdict /CMap defineresource pop end end");
        let widths = cids
            .iter()
            .map(|(_, width)| format!("{width:.0}"))
            .collect::<Vec<_>>()
            .join(" ");
        let cid_map: Vec<u8> = cids.iter().flat_map(|(gid, _)| gid.to_be_bytes()).collect();
        let rgb = crate::pdf_header::logo_rgb();
        let mut objects = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            format!("<< /Type /Pages /Count {count} /Kids [{}] >>", (0..count).map(|i| format!("{} 0 R", 14+i*2)).collect::<Vec<_>>().join(" ")).into_bytes(),
            b"<< /Type /Font /Subtype /Type0 /BaseFont /KetchUpUbuntu /Encoding /Identity-H /DescendantFonts [4 0 R] /ToUnicode 8 0 R >>".to_vec(),
            format!("<< /Type /Font /Subtype /CIDFontType2 /BaseFont /KetchUpUbuntu /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor 5 0 R /CIDToGIDMap 7 0 R /W [0 [{widths}]] >>").into_bytes(),
            b"<< /Type /FontDescriptor /FontName /KetchUpUbuntu /Flags 32 /FontBBox [-1000 -1000 3000 3000] /ItalicAngle 0 /Ascent 1000 /Descent -300 /CapHeight 750 /StemV 80 /FontFile2 6 0 R >>".to_vec(),
            stream(&format!("/Length1 {}", font_data.len()), font_data),
            stream("", &cid_map), stream("", cmap.as_bytes()),
            stream("/Type /XObject /Subtype /Image /Width 256 /Height 256 /ColorSpace /DeviceRGB /BitsPerComponent 8", &rgb),
        ];
        objects.extend([
            b"<< /Type /Font /Subtype /Type0 /BaseFont /KetchUpHack /Encoding /Identity-H /DescendantFonts [11 0 R] /ToUnicode 8 0 R >>".to_vec(),
            format!("<< /Type /Font /Subtype /CIDFontType2 /BaseFont /KetchUpHack /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor 12 0 R /CIDToGIDMap 7 0 R /W [0 [{widths}]] >>").into_bytes(),
            b"<< /Type /FontDescriptor /FontName /KetchUpHack /Flags 33 /FontBBox [-1000 -1000 3000 3000] /ItalicAngle 0 /Ascent 1000 /Descent -300 /CapHeight 750 /StemV 80 /FontFile2 13 0 R >>".to_vec(),
            stream(&format!("/Length1 {}", fallback_data.len()), fallback_data),
        ]);
        for (i, page) in self.pages.iter().enumerate() {
            objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 3 0 R /F2 10 0 R >> /XObject << /Logo 9 0 R >> >> /Contents {} 0 R >>", 15+i*2).into_bytes());
            objects.push(stream("", page.as_bytes()));
        }
        let mut bytes = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::new();
        for (i, object) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            bytes.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            bytes.extend_from_slice(object);
            bytes.extend_from_slice(b"\nendobj\n");
        }
        let xref = bytes.len();
        bytes.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        Ok(bytes)
    }
}
fn stream(attributes: &str, bytes: &[u8]) -> Vec<u8> {
    let mut out = format!("<< /Length {} {attributes} >>\nstream\n", bytes.len()).into_bytes();
    out.extend_from_slice(bytes);
    out.extend_from_slice(b"\nendstream");
    out
}

// Both targets label the date explicitly as UTC, independent of host timezone.
pub(crate) fn generation_date() -> Result<String, String> {
    #[cfg(not(target_arch = "wasm32"))]
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("Could not read the generation date: {error}"))?
        .as_secs()
        / 86_400;
    #[cfg(target_arch = "wasm32")]
    let days = (js_sys::Date::now() / 86_400_000.).floor() as u64;
    Ok(date_from_unix_days(days as i64))
}

fn date_from_unix_days(days: i64) -> String {
    // Gregorian civil date from days since 1970-01-01 (400-year eras).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    let months = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    format!(
        "Generated: {day} {} {year} (UTC)",
        months[(month - 1) as usize]
    )
}

pub(crate) fn render(report: &Report) -> Result<Vec<u8>, String> {
    let definitions = egui::FontDefinitions::default();
    let data = &definitions.font_data["Ubuntu-Light"].font;
    let data: &[u8] = data.as_ref().as_ref();
    let font =
        FontRef::new(data).map_err(|error| format!("Could not load report font: {error}"))?;
    let fallback_data: &[u8] = definitions.font_data["Hack"].font.as_ref().as_ref();
    let fallback = FontRef::new(fallback_data)
        .map_err(|error| format!("Could not load report symbol font: {error}"))?;
    let mut text = vec![report.name.as_str(), report.currency.as_str()];
    text.extend(report.summary.iter().map(String::as_str));
    text.extend(report.notes.iter().map(String::as_str));
    text.extend(report.totals.iter().map(String::as_str));
    text.extend(report.tasks.iter().flatten().map(String::as_str));
    for group in &report.costs {
        text.push(&group.title);
        text.extend(group.rows.iter().flatten().map(String::as_str));
    }
    let chars: std::collections::BTreeSet<_> = text
        .iter()
        .flat_map(|text| text.chars())
        .filter(|ch| !ch.is_control())
        .collect();
    if chars.len() > 65_000 {
        return Err("Too many distinct characters for one estimate PDF.".into());
    }
    if let Some(ch) = chars
        .iter()
        .find(|ch| font.charmap().map(**ch).is_none() && fallback.charmap().map(**ch).is_none())
    {
        return Err(format!(
            "The PDF font cannot display '{ch}' (U+{:04X}). Replace this character before exporting; the report was not truncated.",
            *ch as u32
        ));
    }
    let mut pdf = Pdf {
        font,
        fallback,
        chars: BTreeMap::new(),
        pages: Vec::new(),
        page: String::new(),
        header_name: report.name.clone(),
        header_date: generation_date()?,
        y: 0.,
    };
    pdf.new_page();
    let width = (RIGHT - LEFT) / 4.;
    let summary_lines: Vec<_> = report
        .summary
        .iter()
        .chain(std::iter::once(&report.totals[0]))
        .map(|value| pdf.wrap(value, width - 14., 17.))
        .collect();
    let summary_height = summary_lines.iter().map(Vec::len).max().unwrap_or(1);
    let vertical_padding = 8.;
    let height = summary_height as f64 * 21. + 34. + 2. * vertical_padding;
    pdf.ensure(height + 81.);
    pdf.y -= 16.;
    pdf.heading("Summary");
    let top = pdf.y;
    let bottom = top - height;
    let metrics = pdf.font.metrics(Size::new(17.), LocationRef::default());
    for (column, (label, lines)) in [
        "Materials",
        "Hands-on work",
        "Drying / Waiting",
        "Sequential total",
    ]
    .iter()
    .zip(&summary_lines)
    .enumerate()
    {
        let x = LEFT + column as f64 * width;
        // Cost is green; active work is warm, waiting is cool, and their total is purple.
        let (background, foreground) = [
            ([234., 239., 235.], [64., 97., 75.]),
            ([252., 241., 228.], [139., 83., 29.]),
            ([234., 242., 250.], [49., 87., 129.]),
            ([241., 235., 248.], [100., 66., 135.]),
        ][column];
        let [r, g, b] = background.map(|channel| channel / 255.);
        pdf.panel(x, bottom, width, height, &format!("{r:.5} {g:.5} {b:.5}"));
        let [r, g, b] = foreground.map(|channel| channel / 255.);
        let foreground = format!("{r:.5} {g:.5} {b:.5}");
        // Center the number block inside the area above the label band.
        let center = (top + bottom + 24.) * 0.5;
        let first_baseline = center + lines.len().saturating_sub(1) as f64 * 10.5
            - f64::from(metrics.ascent + metrics.descent) * 0.5;
        for (index, line) in lines.iter().enumerate() {
            pdf.bold_text_color(
                x + (width - pdf.width(line, 17.)) * 0.5,
                first_baseline - index as f64 * 21.,
                17.,
                line,
                &foreground,
            );
        }
        pdf.text_color(
            x + (width - pdf.width(label, 9.)) * 0.5,
            bottom + 12. + vertical_padding,
            9.,
            label,
            &foreground,
        );
    }
    pdf.y = bottom - 16.;
    for note in &report.notes {
        pdf.note(note);
    }
    pdf.ensure(135.);
    pdf.y -= 16.;
    pdf.heading("Cost estimate");
    let headers = [
        "Item".into(),
        "Quantity".into(),
        format!("Unit price ({})", report.currency),
        format!("Amount ({})", report.currency),
    ];
    let widths = [245., 65., 100., 101.];
    for group in &report.costs {
        pdf.ensure(95.);
        pdf.y -= 10.;
        pdf.group_heading(&group.title);
        pdf.table_header(&headers, &widths);
        for (index, row) in group.rows.iter().enumerate() {
            pdf.table_row(row, &widths, &headers, &group.title, index % 2 == 0);
        }
        pdf.y -= 12.;
    }
    pdf.ensure(150.);
    pdf.y -= 16.;
    pdf.heading("Sequential total");
    let headers = ["Task".into(), "Hands-on".into(), "Drying / Waiting".into()];
    let widths = [285., 113., 113.];
    pdf.y -= 14.; // Align the table's top edge with the shared section content top.
    pdf.table_header(&headers, &widths);
    for (index, row) in report.tasks.iter().enumerate() {
        pdf.table_row(row, &widths, &headers, "Sequential total", index % 2 == 0);
    }
    pdf.table_row(
        &[
            "Total".into(),
            report.totals[1].clone(),
            report.totals[2].clone(),
        ],
        &widths,
        &headers,
        "Sequential total",
        true,
    );
    pdf.y -= 16.;
    pdf.note("Includes final drying. Objects dry together between steps. Time covers sanding and finishing only; labor is not priced.");
    pdf.finish(data, fallback_data)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generation_dates_handle_epoch_leap_day_and_year_boundaries() {
        assert_eq!(date_from_unix_days(0), "Generated: 1 January 1970 (UTC)");
        assert_eq!(
            date_from_unix_days(11016),
            "Generated: 29 February 2000 (UTC)"
        );
        assert_eq!(
            date_from_unix_days(20757),
            "Generated: 31 October 2026 (UTC)"
        );
        assert_eq!(
            date_from_unix_days(20819),
            "Generated: 1 January 2027 (UTC)"
        );
        let generated = generation_date().unwrap();
        assert!(generated.starts_with("Generated: "));
        assert!(generated.ends_with(" (UTC)"));
    }

    #[test]
    fn short_cells_are_centered_beside_multiline_items() {
        let fonts = egui::FontDefinitions::default();
        let data: &[u8] = fonts.font_data["Ubuntu-Light"].font.as_ref().as_ref();
        let fallback: &[u8] = fonts.font_data["Hack"].font.as_ref().as_ref();
        let mut pdf = Pdf {
            font: FontRef::new(data).unwrap(),
            fallback: FontRef::new(fallback).unwrap(),
            chars: BTreeMap::new(),
            pages: Vec::new(),
            page: String::new(),
            header_name: String::new(),
            header_date: String::new(),
            y: 700.,
        };
        pdf.table_row(
            &[
                "First\nSecond\nThird".into(),
                "2".into(),
                "25.00".into(),
                "50.00".into(),
            ],
            &[245., 65., 100., 101.],
            &[
                "Item".into(),
                "Quantity".into(),
                "Price".into(),
                "Amount".into(),
            ],
            "Paint",
            true,
        );
        assert!(pdf.page.contains("48.000 695.500 Tm"));
        assert!(pdf.page.contains("293.000 682.500 Tm"));
        assert!(pdf.page.contains("358.000 682.500 Tm"));
    }

    #[test]
    fn report_paginates_long_rows_and_embeds_real_logo_and_unicode_font() {
        let report = Report {
            name: "Étagère - pièce № 1".into(),
            summary: [
                "CAD 125.00 (partial)".into(),
                "1 h 20 min".into(),
                "24 h 0 min".into(),
            ],
            currency: "CAD".into(),
            costs: vec![Group {
                title: "Paint".into(),
                rows: (0..100)
                    .map(|i| {
                        [
                            format!("Produit {i}: {}", "Long description ".repeat(30)),
                            "2 containers, 946 mL each".into(),
                            "25.00".into(),
                            "50.00".into(),
                        ]
                    })
                    .collect(),
            }],
            totals: [
                "25 h 20 min".into(),
                "1 h 20 min".into(),
                "24 h 0 min".into(),
            ],
            ..Default::default()
        };
        let bytes = render(&report).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(bytes.starts_with(b"%PDF-1.4"));
        assert!(text.contains("/ToUnicode"));
        assert!(text.contains("/Subtype /Image /Width 256"));
        assert!(text.matches("/Type /Page ").count() > 2);
        assert!(text.contains("<00C9>"));
    }
}
