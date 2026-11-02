//! Screenshot settings, paper composition and portable image/PDF packaging.
use image::{DynamicImage, RgbaImage};
use ketchup_core::{Camera, DVec2, DVec3, Model, Projection, StandardView};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Write};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    pub views: [bool; 8],
    pub combined: bool,
    pub paper: Paper,
    pub landscape: bool,
    pub format: Format,
    pub dpi: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            views: [false, true, true, false, false, false, false, true],
            combined: true,
            paper: Paper::A4,
            landscape: true,
            format: Format::Png,
            dpi: 150,
        }
    }
}

pub(crate) const VIEW_LABELS: [&str; 8] = [
    "Current",
    "Top",
    "Front",
    "Back",
    "Left",
    "Right",
    "Bottom",
    "Isometric",
];
const STANDARD_VIEWS: [Option<StandardView>; 8] = [
    None,
    Some(StandardView::Top),
    Some(StandardView::Front),
    Some(StandardView::Back),
    Some(StandardView::Left),
    Some(StandardView::Right),
    Some(StandardView::Bottom),
    Some(StandardView::Iso),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Paper {
    A5,
    A4,
    A3,
    Letter,
    Legal,
    Tabloid,
}
impl Paper {
    pub const ALL: [Self; 6] = [
        Self::A5,
        Self::A4,
        Self::A3,
        Self::Letter,
        Self::Legal,
        Self::Tabloid,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::A5 => "A5 · 148 × 210 mm",
            Self::A4 => "A4 · 210 × 297 mm",
            Self::A3 => "A3 · 297 × 420 mm",
            Self::Letter => "Letter · 8.5 × 11 in",
            Self::Legal => "Legal · 8.5 × 14 in",
            Self::Tabloid => "Tabloid · 11 × 17 in",
        }
    }
    fn mm(self) -> [f64; 2] {
        match self {
            Self::A5 => [148., 210.],
            Self::A4 => [210., 297.],
            Self::A3 => [297., 420.],
            Self::Letter => [215.9, 279.4],
            Self::Legal => [215.9, 355.6],
            Self::Tabloid => [279.4, 431.8],
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Format {
    Png,
    Jpeg,
    WebP,
    Pdf,
}
impl Format {
    // Retain Jpeg in the serialized enum for older preferences, but only
    // offer formats that preserve the generated image's transparency.
    pub const ALL: [Self; 3] = [Self::Png, Self::WebP, Self::Pdf];
    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::WebP => "WebP",
            Self::Pdf => "PDF",
        }
    }
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::WebP => "webp",
            Self::Pdf => "pdf",
        }
    }
    #[cfg(target_arch = "wasm32")]
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::WebP => "image/webp",
            Self::Pdf => "application/pdf",
        }
    }
}
impl Settings {
    pub fn selected(&self) -> Vec<usize> {
        self.views
            .iter()
            .enumerate()
            .filter_map(|(i, on)| on.then_some(i))
            .collect()
    }
    pub fn paper_mm(&self) -> [f64; 2] {
        let [w, h] = self.paper.mm();
        if self.landscape { [h, w] } else { [w, h] }
    }
    pub fn pixels(&self) -> [u32; 2] {
        self.paper_mm()
            .map(|mm| (mm / 25.4 * f64::from(self.dpi.clamp(72, 300))).round() as u32)
    }
    pub fn grid(&self) -> [u32; 2] {
        let n = self.selected().len().max(1) as u32;
        if !self.combined {
            return [1, 1];
        }
        let [w, h] = self.paper_mm();
        let cols = (1..=n)
            .min_by(|&a, &b| {
                let score = |c: u32| {
                    ((w / c as f64) / (h / n.div_ceil(c) as f64) / 1.4)
                        .ln()
                        .abs()
                };
                score(a).total_cmp(&score(b))
            })
            .unwrap_or(1);
        [cols, n.div_ceil(cols)]
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.selected().is_empty() {
            return Err("Select at least one view.".into());
        }
        let [w, h] = self.pixels();
        let pages = if self.combined {
            1
        } else {
            self.selected().len() as u64
        };
        if u64::from(w) * u64::from(h) * pages > 64_000_000 {
            return Err("This export is too large. Choose a lower resolution, smaller paper or fewer views.".into());
        }
        Ok(())
    }
}

/// Fit visible geometry, excluding vertices belonging only to hidden parts.
pub(crate) fn view_camera(model: &Model, current: &Camera, view: usize, size: [u32; 2]) -> Camera {
    let mut camera = current.clone();
    if let Some(standard) = STANDARD_VIEWS[view] {
        camera.set_standard_view(standard);
        camera.projection = Projection::Parallel;
    }
    let mut points = Vec::new();
    let mut include = |p: DVec3| {
        points.push(p);
    };
    for edge in model.edges.values().filter(|e| !model.is_hidden(e.id)) {
        let (a, b) = model.points(edge);
        include(a);
        include(b);
    }
    for face in model.faces.values().filter(|f| !model.is_hidden(f.id)) {
        for id in face.vertices.iter().chain(face.holes.iter().flatten()) {
            include(model.vertices[id].position);
        }
    }
    for dimension in model.dimensions.values().filter(|d| !model.is_hidden(d.id)) {
        for p in dimension.layout(model) {
            include(p);
        }
    }
    for text in model.texts.values().filter(|t| !model.is_hidden(t.id)) {
        for p in text.layout(model) {
            include(p);
        }
    }
    fit_content(&mut camera, &points, size);
    camera
}

/// Fit this view's actual extents rather than a sphere around the 3D bounds.
/// Reserve four percent on each side for strokes and annotation glyphs; the
/// capture viewport separately reserves space for the view title and margins.
fn fit_content(camera: &mut Camera, points: &[DVec3], size: [u32; 2]) {
    let Some(&origin) = points.first() else {
        return;
    };
    let right = camera.right();
    let up = camera.up();
    let forward = (camera.target - camera.eye()).normalize();
    let points: Vec<_> = points
        .iter()
        .map(|&point| {
            let point = point - origin;
            DVec3::new(point.dot(right), point.dot(up), point.dot(forward))
        })
        .collect();
    let (lo, hi) = points.iter().fold(
        (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
        |(lo, hi), &point| (lo.min(point), hi.max(point)),
    );
    let mut center = (lo + hi) * 0.5;
    let aspect = (f64::from(size[0].max(1)) / f64::from(size[1].max(1))).max(0.01);
    let tan_y = (camera.fov * 0.5).tan() * 0.92;
    let tan_x = tan_y * aspect;
    let distance = if camera.projection == Projection::Parallel {
        ((hi.y - lo.y) / (2. * tan_y)).max((hi.x - lo.x) / (2. * tan_x))
    } else {
        // At depth z, each point must fit inside the perspective frustum:
        // |x - center.x| <= tan_x * (distance + z), and likewise for y.
        // Intersect these constraints to fit even deep or off-center models
        // without changing the current viewing angle or clipping near points.
        let mut lower = DVec2::splat(f64::NEG_INFINITY);
        let mut upper = DVec2::splat(f64::INFINITY);
        let tangent = DVec2::new(tan_x, tan_y);
        for point in &points {
            let depth = point.z - center.z;
            lower = lower.max(point.truncate() - tangent * depth);
            upper = upper.min(point.truncate() + tangent * depth);
        }
        let xy = (lower + upper) * 0.5;
        center.x = xy.x;
        center.y = xy.y;
        ((lower - upper) / (2. * tangent))
            .max_element()
            .max(center.z - lo.z + 0.01)
    };
    camera.distance = distance.clamp(0.05, 1e6);
    if camera.projection == Projection::Parallel {
        // Depth does not affect orthographic scale. Move the eye in front of
        // deep geometry without adding empty space to its projected bounds.
        center.z = center.z.min(lo.z + camera.distance - 0.01);
    }
    camera.target = origin + right * center.x + up * center.y + forward * center.z;
}

pub(crate) fn compose(settings: &Settings, views: &[RgbaImage], size: [u32; 2]) -> Vec<RgbaImage> {
    if !settings.combined {
        return views.to_vec();
    }
    let mut page = RgbaImage::new(size[0], size[1]);
    let [cols, rows] = settings.grid();
    for (i, view) in views.iter().enumerate() {
        image::imageops::replace(
            &mut page,
            view,
            (i as u32 % cols * (size[0] / cols)) as i64,
            (i as u32 / cols * (size[1] / rows)) as i64,
        );
    }
    vec![page]
}

pub(crate) struct Export {
    pub extension: &'static str,
    #[cfg(target_arch = "wasm32")]
    pub mime: &'static str,
    pub bytes: Vec<u8>,
}

pub(crate) fn unpremultiply(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        for channel in &mut pixel[..3] {
            *channel = if alpha == 0 {
                0
            } else {
                ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8
            };
        }
    }
}

#[cfg(test)]
pub(crate) fn encode(settings: &Settings, pages: &[RgbaImage]) -> Result<Export, String> {
    encode_with_title(settings,pages,"Model views")
}
pub(crate) fn encode_with_title(settings: &Settings, pages: &[RgbaImage], title:&str) -> Result<Export, String> {
    settings.validate()?;
    if settings.format == Format::Jpeg {
        return Err("JPEG cannot preserve transparency. Choose PNG, WebP or PDF.".into());
    }
    if pages.len()
        != if settings.combined {
            1
        } else {
            settings.selected().len()
        }
    {
        return Err("Some screenshot views did not finish rendering.".into());
    }
    if settings.format == Format::Pdf {
        return Ok(Export {
            extension: "pdf",
            #[cfg(target_arch = "wasm32")]
            mime: "application/pdf",
            bytes: pdf(settings.paper_mm(), pages, title),
        });
    }
    let encode_image = |image: &RgbaImage| -> Result<Vec<u8>, String> {
        let mut output = Cursor::new(Vec::new());
        let image = DynamicImage::ImageRgba8(image.clone());
        match settings.format {
            Format::WebP => image.write_to(&mut output, image::ImageFormat::WebP),
            _ => image.write_to(&mut output, image::ImageFormat::Png),
        }
        .map_err(|e| e.to_string())?;
        Ok(output.into_inner())
    };
    if pages.len() == 1 {
        return Ok(Export {
            extension: settings.format.extension(),
            #[cfg(target_arch = "wasm32")]
            mime: settings.format.mime(),
            bytes: encode_image(&pages[0])?,
        });
    }
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (&view, page) in settings.selected().iter().zip(pages) {
        zip.start_file(
            format!(
                "{}.{}",
                VIEW_LABELS[view].to_ascii_lowercase(),
                settings.format.extension()
            ),
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .map_err(|e| e.to_string())?;
        zip.write_all(&encode_image(page)?)
            .map_err(|e| e.to_string())?;
    }
    Ok(Export {
        extension: "zip",
        #[cfg(target_arch = "wasm32")]
        mime: "application/zip",
        bytes: zip.finish().map_err(|e| e.to_string())?.into_inner(),
    })
}

/// Lossless RGB images with alpha masks and physical page dimensions in PDF points.
fn pdf(mm: [f64; 2], pages: &[RgbaImage], title:&str) -> Vec<u8> {
    let [w, h] = mm.map(|v| v * 72. / 25.4);
    let mut objects: Vec<Vec<u8>> = vec![b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(), Vec::new()];
    let logo_id=3+pages.len()*4;
    let font_id=logo_id+1;
    let date=crate::estimate_pdf::generation_date().unwrap_or_else(|_| "Date unavailable".into());
    let header=crate::pdf_header::layout(title,"Model Views",&date,[w,h],crate::pdf_header::base_width);
    let mut kids = Vec::new();
    for page in pages {
        let id = objects.len() + 1;
        kids.push(format!("{id} 0 R"));
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w:.4} {h:.4}] /Resources << /Font << /FH {font_id} 0 R >> /XObject << /Logo {logo_id} 0 R /View {} 0 R >> >> /Contents {} 0 R >>", id + 1, id + 2).into_bytes());
        let rgb = DynamicImage::ImageRgba8(page.clone()).to_rgb8();
        let alpha = image::GrayImage::from_fn(page.width(), page.height(), |x, y| {
            image::Luma([page.get_pixel(x, y)[3]])
        });
        objects.push(pdf_image_stream(
            DynamicImage::ImageRgb8(rgb),
            "DeviceRGB",
            3,
            Some(id + 3),
        ));
        let scale=((w-84.)/f64::from(page.width())).min((header.body_top-42.).max(1.)/f64::from(page.height()));
        let image_w=f64::from(page.width())*scale;
        let image_h=f64::from(page.height())*scale;
        let x=(w-image_w)*0.5;
        let y=42.+(header.body_top-42.-image_h).max(0.)*0.5;
        let mut commands=header.graphics.clone();
        for text in &header.text {
            use std::fmt::Write;
            let _=writeln!(commands,"0.15 0.20 0.17 rg BT /FH {} Tf 1 0 0 1 {:.3} {:.3} Tm ({}) Tj ET",text.size,text.x,text.y,crate::pdf_header::literal(&text.value));
        }
        commands.push_str(&format!("q {image_w:.4} 0 0 {image_h:.4} {x:.4} {y:.4} cm /View Do Q\n"));
        objects.push(
            format!(
                "<< /Length {} >>\nstream\n{commands}endstream",
                commands.len()
            )
            .into_bytes(),
        );
        objects.push(pdf_image_stream(
            DynamicImage::ImageLuma8(alpha),
            "DeviceGray",
            1,
            None,
        ));
    }
    objects.push(crate::pdf_header::logo_hex_object().into_bytes());
    objects.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec());
    objects[1] = format!(
        "<< /Type /Pages /Count {} /Kids [{}] >>",
        pages.len(),
        kids.join(" ")
    )
    .into_bytes();
    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = vec![0];
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in offsets.iter().skip(1) {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len()
        )
        .as_bytes(),
    );
    out
}

fn pdf_image_stream(
    image: DynamicImage,
    color_space: &str,
    channels: usize,
    mask: Option<usize>,
) -> Vec<u8> {
    // PNG's concatenated IDAT stream is already zlib-compressed scanlines;
    // PDF's PNG predictor decodes both the RGB image and its grayscale alpha.
    let mut png = Cursor::new(Vec::new());
    image
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("writing a PNG to memory");
    let png = png.into_inner();
    let mut compressed = Vec::new();
    let mut at = 8;
    while at + 12 <= png.len() {
        let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
        if &png[at + 4..at + 8] == b"IDAT" {
            compressed.extend_from_slice(&png[at + 8..at + 8 + len]);
        }
        at += len + 12;
    }
    let mask = mask.map_or_else(String::new, |id| format!(" /SMask {id} 0 R"));
    let mut stream = format!("<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /{color_space} /BitsPerComponent 8 /Filter /FlateDecode /DecodeParms << /Predictor 15 /Colors {channels} /BitsPerComponent 8 /Columns {} >>{mask} /Length {} >>\nstream\n", image.width(), image.height(), image.width(), compressed.len()).into_bytes();
    stream.extend(compressed);
    stream.extend_from_slice(b"\nendstream");
    stream
}

#[cfg(test)]
#[path = "screenshot_tests.rs"]
mod tests;
