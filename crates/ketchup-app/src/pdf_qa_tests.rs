use crate::{
    cutting_plan::{CutGroup, CuttingPlan},
    estimate_pdf::{Group, Report},
    screenshots::{self, Format, Settings},
};

#[test]
fn all_pdf_exports_use_the_standard_header_and_keep_content_below_it() {
    let name = "Walnut Bookcase - Workshop";
    let estimate = crate::estimate_pdf::render(&Report {
        name: name.into(),
        summary: ["Wood stock".into(), "2 h".into(), "24 h".into()],
        notes: vec!["Finished dimensions; verify stock before cutting.".into()],
        costs: vec![Group {
            title: "Wood stock".into(),
            rows: (0..60)
                .map(|i| {
                    [
                        format!("Walnut panel {}", i + 1),
                        "1".into(),
                        "CAD 45.00".into(),
                        "CAD 45.00".into(),
                    ]
                })
                .collect(),
        }],
        totals: ["26 h".into(), "2 h".into(), "24 h".into()],
        ..Default::default()
    })
    .unwrap();
    let material = ketchup_core::StockMaterial {
        name: "Walnut".into(),
        dimensions: [190, 1400, 24000],
    };
    let cutting = CuttingPlan {
        groups: vec![CutGroup {
            material,
            material_id: None,
            dimensions: [190, 1400, 8500],
            rectangular: true,
            parts: (0..32)
                .map(|i| format!("Shelf {} / Bookcase", i + 1))
                .collect(),
        }],
        skipped: 0,
    }
    .pdf(name, false, true);
    let mut settings = Settings {
        format: Format::Pdf,
        ..Default::default()
    };
    settings.views = [true, false, false, false, false, false, false, false];
    let page = image::RgbaImage::from_fn(900, 600, |x, y| {
        if (200..700).contains(&x) && (160..440).contains(&y) {
            image::Rgba([145, 98, 61, 255])
        } else {
            image::Rgba([0, 0, 0, 0])
        }
    });
    let screenshot = screenshots::encode_with_title(&settings, &[page], name)
        .unwrap()
        .bytes;
    for (filename, bytes) in [
        ("estimate.pdf", estimate),
        ("cutting-plan.pdf", cutting),
        ("model-views.pdf", screenshot),
    ] {
        let text = String::from_utf8_lossy(&bytes);
        assert!(bytes.starts_with(b"%PDF-1.4"));
        let pages = text.matches("/Type /Page ").count();
        assert!(pages > 0);
        assert_eq!(text.matches("/Logo Do").count(), pages, "{filename}");
        assert!(text.contains("/Subtype /Image /Width 256"));
        if let Some(dir) = std::env::var_os("KETCHUP_PDF_QA_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(std::path::Path::new(&dir).join(filename), bytes).unwrap();
        }
    }
}
