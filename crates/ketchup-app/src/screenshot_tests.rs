use super::*;
use image::Rgba;

fn selected(indices: &[usize]) -> Settings {
    let mut settings = Settings {
        views: [false; 8],
        ..Default::default()
    };
    for &i in indices {
        settings.views[i] = true;
    }
    settings
}

#[test]
fn defaults_survive_older_preferences_and_all_options_roundtrip() {
    let defaults: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(defaults, Settings::default());
    assert_eq!(defaults.selected(), vec![1, 2, 7]);
    for format in Format::ALL {
        for paper in Paper::ALL {
            let settings = Settings {
                paper,
                format,
                landscape: false,
                combined: false,
                dpi: 300,
                ..selected(&[0, 3, 5, 6])
            };
            let restored: Settings =
                serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
            assert_eq!(settings, restored);
        }
    }
}

#[test]
fn physical_paper_sizes_orientation_and_resolution_match() {
    let mut settings = Settings {
        landscape: false,
        paper: Paper::Letter,
        dpi: 300,
        ..Default::default()
    };
    assert_eq!(settings.pixels(), [2550, 3300]);
    assert_eq!(settings.paper_mm(), [215.9, 279.4]);
    settings.landscape = true;
    assert_eq!(settings.pixels(), [3300, 2550]);
    settings.paper = Paper::A4;
    settings.dpi = 150;
    assert_eq!(settings.pixels(), [1754, 1240]);
}

#[test]
fn empty_and_excessively_large_exports_are_rejected() {
    assert!(selected(&[]).validate().is_err());
    let settings = Settings {
        paper: Paper::A3,
        combined: false,
        dpi: 300,
        ..selected(&[0, 1, 2, 3, 4, 5, 6, 7])
    };
    assert!(settings.validate().is_err());
    assert!(
        Settings {
            combined: true,
            ..settings
        }
        .validate()
        .is_ok()
    );
}

#[test]
fn combined_layout_preserves_every_selected_view_without_overlap() {
    for landscape in [true, false] {
        for n in 1..=8 {
            let settings = Settings {
                landscape,
                ..selected(&(0..n).collect::<Vec<_>>())
            };
            let [cols, rows] = settings.grid();
            assert!(cols * rows >= n as u32);
            let size = [cols * 20, rows * 20];
            let views: Vec<_> = (0..n)
                .map(|i| RgbaImage::from_pixel(20, 20, Rgba([i as u8 * 25, 30, 40, 255])))
                .collect();
            let pages = compose(&settings, &views, size);
            assert_eq!(pages.len(), 1);
            assert_eq!(pages[0].dimensions(), (size[0], size[1]));
            for i in 0..n {
                assert_eq!(
                    pages[0].get_pixel(i as u32 % cols * 20 + 10, i as u32 / cols * 20 + 10),
                    views[i].get_pixel(10, 10)
                );
            }
        }
    }
}

#[test]
fn all_raster_formats_decode_and_separate_images_are_named_in_zip() {
    let image = RgbaImage::from_pixel(48, 32, Rgba([75, 130, 190, 255]));
    for format in [Format::Png, Format::WebP] {
        let settings = Settings {
            format,
            ..selected(&[2])
        };
        let export = encode(&settings, std::slice::from_ref(&image)).unwrap();
        let decoded = image::load_from_memory(&export.bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (48, 32));
        let settings = Settings {
            combined: false,
            ..selected(&[2, 3])
        };
        let export = encode(
            &Settings { format, ..settings },
            &[image.clone(), image.clone()],
        )
        .unwrap();
        assert_eq!(export.extension, "zip");
        let mut archive = zip::ZipArchive::new(Cursor::new(export.bytes)).unwrap();
        assert_eq!(archive.len(), 2);
        for view in ["front", "back"] {
            let mut file = archive
                .by_name(&format!("{view}.{}", format.extension()))
                .unwrap();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut bytes).unwrap();
            assert_eq!(image::load_from_memory(&bytes).unwrap().width(), 48);
        }
    }
}

#[test]
fn pdf_contains_physical_pages_lossless_images_and_valid_xref_offsets() {
    let settings = Settings {
        format: Format::Pdf,
        combined: false,
        landscape: false,
        paper: Paper::Letter,
        ..selected(&[1, 2])
    };
    let page = RgbaImage::from_pixel(48, 32, Rgba([120, 80, 30, 255]));
    let export = encode(&settings, &[page.clone(), page]).unwrap();
    let text = String::from_utf8_lossy(&export.bytes);
    assert!(text.starts_with("%PDF-1.4"));
    assert!(text.contains("/Count 2"));
    assert_eq!(text.matches("/MediaBox [0 0 612.0000 792.0000]").count(), 2);
    assert_eq!(text.matches("/Subtype /Image").count(), 4);
    assert_eq!(text.matches("/ColorSpace /DeviceGray").count(), 2);
    assert!(text.contains("/SMask 6 0 R"));
    assert!(text.contains("/SMask 10 0 R"));
    let start = text
        .rsplit("startxref\n")
        .next()
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .parse::<usize>()
        .unwrap();
    assert_eq!(&export.bytes[start..start + 4], b"xref");
    assert!(encode(&settings, &[]).is_err());
}

#[test]
fn standard_cameras_are_fitted_without_changing_the_source_camera_or_model() {
    let mut model = Model::default();
    model.add_edge(DVec3::ZERO, DVec3::new(5., 3., 2.)).unwrap();
    let current = Camera::default();
    let before = model.clone();
    for view in 1..8 {
        let camera = view_camera(&model, &current, view, [640, 480]);
        assert_eq!(camera.standard_view(), STANDARD_VIEWS[view]);
        assert_eq!(camera.projection, Projection::Parallel);
        assert!(camera.valid());
        for vertex in model.vertices.values() {
            let p = camera
                .project(vertex.position, DVec2::new(640., 480.))
                .unwrap();
            assert!(p.x >= 0. && p.x <= 640. && p.y >= 0. && p.y <= 480.);
        }
    }
    assert_eq!(view_camera(&model, &current, 0, [640, 480]), current);
    assert_eq!(model, before);
    assert_eq!(current, Camera::default());
}

#[test]
fn transparent_pixels_survive_composition_and_supported_image_exports() {
    let settings = selected(&[1, 2, 3]);
    let [cols, rows] = settings.grid();
    let size = [cols * 20 + 1, rows * 20 + 1];
    let mut view = RgbaImage::new(20, 20);
    view.put_pixel(10, 10, Rgba([90, 140, 190, 128]));
    let pages = compose(&settings, &[view.clone(), view.clone(), view.clone()], size);
    assert_eq!(pages[0].get_pixel(0, 0).0, [0; 4]);
    assert_eq!(pages[0].get_pixel(size[0] - 1, size[1] - 1).0, [0; 4]);
    assert_eq!(pages[0].get_pixel(10, 10).0, [90, 140, 190, 128]);
    for format in [Format::Png, Format::WebP] {
        let export = encode(
            &Settings {
                format,
                ..settings.clone()
            },
            &pages,
        )
        .unwrap();
        let restored = image::load_from_memory(&export.bytes).unwrap().to_rgba8();
        assert_eq!(restored, pages[0]);
    }
    assert!(!Format::ALL.contains(&Format::Jpeg));
    assert!(
        encode(
            &Settings {
                format: Format::Jpeg,
                ..settings
            },
            &pages
        )
        .is_err()
    );
}

#[test]
fn readback_converts_premultiplied_edges_without_dark_halos() {
    let mut pixels = [50, 25, 0, 128, 99, 88, 77, 0, 17, 30, 45, 255];
    unpremultiply(&mut pixels);
    assert_eq!(pixels, [100, 50, 0, 128, 0, 0, 0, 0, 17, 30, 45, 255]);
}
