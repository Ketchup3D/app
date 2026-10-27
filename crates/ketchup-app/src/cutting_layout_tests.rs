use super::*;

fn piece(mark: usize, length: u64, width: u64, quantity: usize) -> Piece {
    Piece {
        mark,
        length,
        width,
        quantity,
    }
}
fn settings(length: f64, width: f64, kerf: f64) -> Settings {
    Settings {
        sheet_length_mm: length,
        sheet_width_mm: width,
        board_length_mm: length,
        kerf_mm: kerf,
        allow_rotation: false,
    }
}

#[test]
fn saw_kerf_changes_required_board_and_sheet_counts() {
    let cuts = [piece(0, 5000, 1000, 2)];
    assert_eq!(boards(&cuts, settings(1000., 100., 0.)).stock.len(), 1);
    assert_eq!(boards(&cuts, settings(1000., 100., 3.)).stock.len(), 2);
    assert_eq!(sheets(&cuts, settings(1000., 100., 0.)).stock.len(), 1);
    assert_eq!(sheets(&cuts, settings(1000., 100., 3.)).stock.len(), 2);
    // Exact edge fits never require an extra kerf outside the stock.
    assert_eq!(
        boards(&[piece(0, 10000, 1000, 1)], settings(1000., 100., 3.))
            .stock
            .len(),
        1
    );
    assert_eq!(
        sheets(&[piece(0, 10000, 1000, 1)], settings(1000., 100., 3.))
            .stock
            .len(),
        1
    );
}

#[test]
fn rotation_is_opt_in_and_oversized_pieces_never_enlarge_stock() {
    let cuts = [piece(7, 7000, 9000, 2)];
    let options = settings(1000., 800., 3.);
    let locked = sheets(&cuts, options);
    assert!(locked.stock.is_empty());
    assert_eq!(locked.oversized, cuts);
    let rotated = sheets(
        &cuts,
        Settings {
            allow_rotation: true,
            ..options
        },
    );
    assert_eq!(rotated.stock.len(), 2);
    assert!(rotated.oversized.is_empty());
    for stock in rotated.stock {
        assert_eq!((stock.length, stock.width), (10000, 8000));
        assert!(stock.placements[0].rotated);
        assert_eq!(stock.placements[0].rect.length, 9000);
        assert_eq!(stock.placements[0].rect.width, 7000);
    }
    let huge = [piece(0, 200_000_000_000_000, 200_000_000_000_000, 2)];
    assert_eq!(sheets(&huge, options).oversized, huge);
    assert_eq!(boards(&huge, options).oversized, huge);
}

#[test]
fn sheet_layouts_conserve_quantities_keep_pieces_in_bounds_and_reserve_kerf() {
    let options = Settings {
        allow_rotation: true,
        ..Settings::default()
    };
    let cuts: Vec<_> = (0..30)
        .map(|i| {
            piece(
                i,
                1000 + ((i * 197) % 9000) as u64,
                500 + ((i * 313) % 6000) as u64,
                i % 3 + 1,
            )
        })
        .collect();
    let layout = sheets(&cuts, options);
    assert!(layout.oversized.is_empty());
    let kerf = tenths(options.kerf_mm);
    let mut counts = BTreeMap::new();
    for stock in &layout.stock {
        for (index, p) in stock.placements.iter().enumerate() {
            let r = p.rect;
            assert!(r.x + r.length <= stock.length);
            assert!(r.y + r.width <= stock.width);
            let original = &cuts[p.mark];
            assert_eq!(
                (r.length, r.width),
                if p.rotated {
                    (original.width, original.length)
                } else {
                    (original.length, original.width)
                }
            );
            *counts.entry(p.mark).or_insert(0) += 1;
            for other in &stock.placements[index + 1..] {
                let q = other.rect;
                assert!(
                    r.x + r.length + kerf <= q.x
                        || q.x + q.length + kerf <= r.x
                        || r.y + r.width + kerf <= q.y
                        || q.y + q.width + kerf <= r.y,
                    "Pieces overlap or omit kerf: {r:?}, {q:?}"
                );
            }
        }
    }
    for cut in cuts {
        assert_eq!(counts[&cut.mark], cut.quantity);
    }
}

#[test]
fn board_layouts_keep_codes_quantities_and_kerf_for_mixed_lengths() {
    let cuts = [
        piece(5, 6000, 890, 7),
        piece(2, 21000, 890, 2),
        piece(8, 30000, 890, 3),
    ];
    let layout = boards(&cuts, Settings::default());
    assert_eq!(layout.oversized, [cuts[2]]);
    let mut counts = BTreeMap::new();
    for stock in &layout.stock {
        let mut previous_end = None;
        for p in &stock.placements {
            if let Some(end) = previous_end {
                assert!(p.rect.x >= end + 30);
            }
            assert!(p.rect.x + p.rect.length <= stock.length);
            previous_end = Some(p.rect.x + p.rect.length);
            *counts.entry(p.mark).or_insert(0) += 1;
        }
    }
    assert_eq!(counts[&5], 7);
    assert_eq!(counts[&2], 2);
}

#[test]
fn invalid_stock_settings_are_rejected() {
    for options in [
        Settings {
            kerf_mm: -1.,
            ..Settings::default()
        },
        Settings {
            sheet_width_mm: 0.,
            ..Settings::default()
        },
        Settings {
            board_length_mm: f64::NAN,
            ..Settings::default()
        },
        Settings {
            kerf_mm: f64::INFINITY,
            ..Settings::default()
        },
    ] {
        assert!(options.validate().is_err());
    }
    assert!(Settings::default().validate().is_ok());
}
