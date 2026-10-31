//! Deterministic, kerf-aware guillotine stock packing.
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    pub sheet_width_mm: f64,
    pub sheet_length_mm: f64,
    pub board_length_mm: f64,
    pub kerf_mm: f64,
    pub allow_rotation: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            sheet_width_mm: 1220.,
            sheet_length_mm: 2440.,
            board_length_mm: 2440.,
            kerf_mm: 3.,
            allow_rotation: true,
        }
    }
}
impl Settings {
    pub fn validate(self) -> Result<(), String> {
        if [
            self.sheet_width_mm,
            self.sheet_length_mm,
            self.board_length_mm,
        ]
        .iter()
        .any(|v| !v.is_finite() || !(1. ..=1_000_000.).contains(v))
            || !self.kerf_mm.is_finite()
            || !(0. ..=100.).contains(&self.kerf_mm)
        {
            return Err("Stock dimensions must be 1-1,000,000 mm and saw kerf 0-100 mm.".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Piece {
    pub mark: usize,
    /// In tenths of a millimetre. X is length, Y is width.
    pub length: u64,
    pub width: u64,
    pub quantity: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rect {
    pub x: u64,
    pub y: u64,
    pub length: u64,
    pub width: u64,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placement {
    pub mark: usize,
    pub rect: Rect,
    pub rotated: bool,
}
#[derive(Debug)]
pub(crate) struct Stock {
    pub length: u64,
    pub width: u64,
    pub placements: Vec<Placement>,
    free: Vec<Rect>,
}
impl Stock {
    fn new(length: u64, width: u64) -> Self {
        Self {
            length,
            width,
            placements: Vec::new(),
            free: vec![Rect {
                x: 0,
                y: 0,
                length,
                width,
            }],
        }
    }
}
#[derive(Debug, Default)]
pub(crate) struct Layout {
    pub stock: Vec<Stock>,
    pub oversized: Vec<Piece>,
}

pub(crate) fn tenths(mm: f64) -> u64 {
    (mm * 10.).round() as u64
}

/// Each split is a full straight cut across the current free rectangle. Kerf is
/// reserved between retained pieces; a cut at the stock edge needs no extra gap.
pub(crate) fn sheets(pieces: &[Piece], settings: Settings) -> Layout {
    let length = tenths(settings.sheet_length_mm);
    let width = tenths(settings.sheet_width_mm);
    let kerf = tenths(settings.kerf_mm);
    let mut pieces = pieces.to_vec();
    pieces.sort_by_key(|p| {
        (
            std::cmp::Reverse(p.length as u128 * p.width as u128),
            std::cmp::Reverse(p.length),
            p.mark,
        )
    });
    let mut layout = Layout::default();
    for piece in pieces {
        let orientations = [
            (piece.length, piece.width, false),
            (piece.width, piece.length, true),
        ];
        if !orientations.iter().any(|(l, w, rotated)| {
            (!*rotated || settings.allow_rotation) && *l <= length && *w <= width
        }) {
            layout.oversized.push(piece);
            continue;
        }
        for _ in 0..piece.quantity {
            // Best-area-fit among the remaining guillotine rectangles.
            let mut best = None;
            for (stock_index, stock) in layout.stock.iter().enumerate() {
                for (free_index, free) in stock.free.iter().enumerate() {
                    for &(l, w, rotated) in &orientations {
                        if rotated && !settings.allow_rotation || l > free.length || w > free.width
                        {
                            continue;
                        }
                        let score = (
                            free.length * free.width - l * w,
                            (free.length - l).min(free.width - w),
                        );
                        if best
                            .as_ref()
                            .is_none_or(|(_, _, _, _, _, previous)| score < *previous)
                        {
                            best = Some((stock_index, free_index, l, w, rotated, score));
                        }
                    }
                }
            }
            let (stock_index, free_index, l, w, rotated) = if let Some((s, f, l, w, r, _)) = best {
                (s, f, l, w, r)
            } else {
                let &(l, w, r) = orientations
                    .iter()
                    .find(|(l, w, r)| {
                        (!*r || settings.allow_rotation) && *l <= length && *w <= width
                    })
                    .unwrap();
                layout.stock.push(Stock::new(length, width));
                (layout.stock.len() - 1, 0, l, w, r)
            };
            let stock = &mut layout.stock[stock_index];
            let free = stock.free.remove(free_index);
            stock.placements.push(Placement {
                mark: piece.mark,
                rect: Rect {
                    x: free.x,
                    y: free.y,
                    length: l,
                    width: w,
                },
                rotated,
            });
            // Horizontal cut first, then a vertical cut inside the lower band.
            if free.length > l + kerf {
                stock.free.push(Rect {
                    x: free.x + l + kerf,
                    y: free.y,
                    length: free.length - l - kerf,
                    width: w,
                });
            }
            if free.width > w + kerf {
                stock.free.push(Rect {
                    x: free.x,
                    y: free.y + w + kerf,
                    length: free.length,
                    width: free.width - w - kerf,
                });
            }
        }
    }
    layout
}

impl Layout {
    pub fn oversized_marks(&self) -> BTreeMap<usize, usize> {
        self.oversized
            .iter()
            .map(|p| (p.mark, p.quantity))
            .collect()
    }
}

#[cfg(test)]
#[path = "cutting_layout_tests.rs"]
mod tests;
