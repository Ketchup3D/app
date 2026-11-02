//! The standard header for every PDF exporter. Content stays below body_top.
use std::fmt::Write;

pub(crate) struct Text {
    pub x: f64,
    pub y: f64,
    pub size: f64,
    pub value: String,
}
pub(crate) struct Header {
    pub graphics: String,
    pub text: Vec<Text>,
    pub body_top: f64,
}
pub(crate) fn layout(
    name: &str,
    subtitle: &str,
    date: &str,
    page: [f64; 2],
    measure: impl Fn(&str, f64) -> f64,
) -> Header {
    let [width, height] = page;
    let right = width - 42.;
    let top = height - 57.;
    let mut text = vec![
        Text {
            x: 85.,
            y: top,
            size: 17.,
            value: "KetchUp".into(),
        },
        Text {
            x: 85.,
            y: top - 15.,
            size: 10.,
            value: subtitle.into(),
        },
    ];
    let max_width = (width * 0.42).min(245.);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in name.split_whitespace() {
        let next = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if measure(&next, 20.) <= max_width {
            line = next;
            continue;
        }
        if !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        for ch in word.chars() {
            let mut next = line.clone();
            next.push(ch);
            if !line.is_empty() && measure(&next, 20.) > max_width {
                lines.push(std::mem::take(&mut line));
            }
            line.push(ch);
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    let mut y = top;
    for line in lines {
        text.push(Text {
            x: right - measure(&line, 20.),
            y,
            size: 20.,
            value: line,
        });
        y -= 24.;
    }
    text.push(Text {
        x: right - measure(date, 9.),
        y: y + 6.,
        size: 9.,
        value: date.into(),
    });
    let separator = (y - 16.).min(height - 94.);
    Header {
        graphics: format!(
            "q 32 0 0 32 42 {:.3} cm /Logo Do Q\n0.80 0.83 0.80 RG 0.5 w 42 {separator:.3} m {right:.3} {separator:.3} l S\n",
            height - 74.
        ),
        text,
        body_top: separator - 31.,
    }
}
/// The same bundled mark used by Time & Cost Estimates, composited on paper.
pub(crate) fn logo_rgb() -> Vec<u8> {
    let image = image::load_from_memory(include_bytes!("../../../assets/branding/icon-256.png"))
        .expect("valid bundled KetchUp logo")
        .to_rgba8();
    image
        .pixels()
        .flat_map(|p| {
            let a = u32::from(p[3]);
            [0, 1, 2].map(|i| ((u32::from(p[i]) * a + 255 * (255 - a)) / 255) as u8)
        })
        .collect()
}
pub(crate) fn logo_hex_object() -> String {
    let rgb = logo_rgb();
    let mut hex = String::with_capacity(rgb.len() * 2 + 1);
    for byte in rgb {
        let _ = write!(hex, "{byte:02X}");
    }
    hex.push('>');
    format!(
        "<< /Type /XObject /Subtype /Image /Width 256 /Height 256 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /ASCIIHexDecode /Length {} >>\nstream\n{hex}\nendstream",
        hex.len()
    )
}
/// Approximate base-font measurement for existing WinAnsi PDF exporters.
pub(crate) fn base_width(text: &str, size: f64) -> f64 {
    text.chars().count() as f64 * size * 0.54
}
pub(crate) fn literal(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars().filter(|c| !c.is_control()) {
        let b = if (' '..='~').contains(&ch) || ('\u{a0}'..='\u{ff}').contains(&ch) {
            ch as u8
        } else {
            b'?'
        };
        if b == b'(' || b == b')' || b == b'\\' || b >= 128 {
            let _ = write!(out, "\\{b:03o}");
        } else {
            out.push(b as char);
        }
    }
    out
}
