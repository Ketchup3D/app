//! One currency preference and one price format for catalogs and estimates.
#[cfg(test)]
use crate::test_ui::ContextExt as _;
use super::searchable_selector::{Choice, SearchableSelector};
use eframe::egui;

pub(super) const DEFAULT: &str = "USD";
pub(super) const RESULTS_SCROLL_KEY: &str = "settings/stock/currency/results";

const CURRENCIES: &[(&str, &str)] = &[
    ("AED", "UAE dirham"),
    ("AUD", "Australian dollar"),
    ("BRL", "Brazilian real"),
    ("CAD", "Canadian dollar"),
    ("CHF", "Swiss franc"),
    ("CLP", "Chilean peso"),
    ("CNY", "Chinese yuan"),
    ("COP", "Colombian peso"),
    ("CZK", "Czech koruna"),
    ("DKK", "Danish krone"),
    ("EUR", "Euro"),
    ("GBP", "British pound"),
    ("HKD", "Hong Kong dollar"),
    ("HUF", "Hungarian forint"),
    ("IDR", "Indonesian rupiah"),
    ("ILS", "Israeli shekel"),
    ("INR", "Indian rupee"),
    ("JPY", "Japanese yen"),
    ("KRW", "South Korean won"),
    ("MXN", "Mexican peso"),
    ("MYR", "Malaysian ringgit"),
    ("NOK", "Norwegian krone"),
    ("NZD", "New Zealand dollar"),
    ("PHP", "Philippine peso"),
    ("PLN", "Polish zloty"),
    ("RON", "Romanian leu"),
    ("SAR", "Saudi riyal"),
    ("SEK", "Swedish krona"),
    ("SGD", "Singapore dollar"),
    ("THB", "Thai baht"),
    ("TRY", "Turkish lira"),
    ("TWD", "Taiwan dollar"),
    ("USD", "US dollar"),
    ("VND", "Vietnamese dong"),
    ("ZAR", "South African rand"),
];

// Saved preferences used to allow arbitrary text. Upgrade familiar symbols and
// codes while retaining custom currencies rather than discarding user data.
pub(super) fn code(value: &str) -> &str {
    let value = value.trim();
    match value {
        "" | "$" | "US$" => DEFAULT,
        "C$" | "CA$" => "CAD",
        "€" => "EUR",
        "£" => "GBP",
        "A$" | "AU$" => "AUD",
        "NZ$" => "NZD",
        "¥" | "JP¥" => "JPY",
        "CN¥" => "CNY",
        "HK$" => "HKD",
        "S$" | "SG$" => "SGD",
        "₹" => "INR",
        "₩" => "KRW",
        "R$" => "BRL",
        _ => CURRENCIES
            .iter()
            .find(|(code, _)| code.eq_ignore_ascii_case(value))
            .map(|(code, _)| *code)
            .unwrap_or(value),
    }
}

pub(super) fn normalize(value: &mut String) {
    let normalized = code(value);
    if normalized != value.as_str() {
        *value = normalized.to_owned();
    }
}

fn label(value: &str) -> String {
    let value = code(value);
    CURRENCIES
        .iter()
        .find(|(code, _)| *code == value)
        .map(|(code, name)| format!("{code} — {name}"))
        .unwrap_or_else(|| value.to_owned())
}

fn decimals(value: &str) -> usize {
    match code(value) {
        "JPY" | "KRW" | "VND" | "CLP" => 0,
        _ => 2,
    }
}

pub(super) fn format_value(currency: &str, amount: f64) -> String {
    let precision = decimals(currency);
    format!("{amount:.precision$}")
}

pub(super) fn price_input<'a>(currency: &str, value: &'a mut f64) -> egui::DragValue<'a> {
    let decimals = decimals(currency);
    egui::DragValue::new(value)
        .range(0. ..=1e9)
        .speed(if decimals == 0 { 1. } else { 0.1 })
        .fixed_decimals(decimals)
        .prefix(format!("{} ", code(currency)))
}

pub(super) fn selector(
    ui: &mut egui::Ui,
    currency: &mut String,
    filter: &mut String,
) -> egui::Response {
    let current = code(currency).to_owned();
    let mut choices = Vec::new();
    if !CURRENCIES.iter().any(|(code, _)| *code == current) {
        choices.push(Choice::new(current.clone(), current));
    }
    choices.extend(
        CURRENCIES
            .iter()
            .map(|(code, name)| Choice::new((*code).to_owned(), format!("{code} — {name}"))),
    );
    SearchableSelector {
        id_salt: egui::IdSalt::new("settings_material_currency"),
        scroll_key: RESULTS_SCROLL_KEY,
        selected_label: &label(currency),
        width: 220.,
        popup_width: 220.,
        hint: "Search code or name…",
        empty_text: "No currencies match your search.",
    }
    .show(ui, currency, filter, choices)
    .response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(
        ctx: &egui::Context,
        currency: &mut String,
        filter: &mut String,
        events: Vec<egui::Event>,
    ) -> (egui::Pos2, Vec<(String, egui::Pos2)>) {
        fn collect(shape: &egui::Shape, labels: &mut Vec<(String, egui::Pos2)>) {
            match shape {
                egui::Shape::Text(text) => labels.push((
                    text.galley.job.text.clone(),
                    text.pos + text.galley.rect.center().to_vec2(),
                )),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, labels);
                    }
                }
                _ => {}
            }
        }
        let mut center = egui::Pos2::ZERO;
        let output = ctx.run_test_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600., 500.),
                )),
                events,
                ..Default::default()
            },
            |ui| center = selector(ui, currency, filter).rect.center(),
        );
        let mut labels = Vec::new();
        for shape in &output.shapes {
            collect(&shape.shape, &mut labels);
        }
        (center, labels)
    }

    fn click(ctx: &egui::Context, currency: &mut String, filter: &mut String, pos: egui::Pos2) {
        for pressed in [true, false] {
            frame(
                ctx,
                currency,
                filter,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }

    #[test]
    fn focused_search_and_popup_keep_their_size_across_repaints() {
        for theme in [egui::ThemePreference::Light, egui::ThemePreference::Dark] {
            for query in [
                "",
                "dollar",
                "a very long search with no matching currencies",
            ] {
                let ctx = egui::Context::default();
                crate::theme::apply(&ctx);
                ctx.set_theme(theme);
                ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
                let mut currency = "USD".to_owned();
                let mut filter = query.to_owned();
                frame(&ctx, &mut currency, &mut filter, vec![]);
                let (center, _) = frame(&ctx, &mut currency, &mut filter, vec![]);
                click(&ctx, &mut currency, &mut filter, center);
                for _ in 0..3 {
                    frame(&ctx, &mut currency, &mut filter, vec![]);
                }
                let search_id = ctx.memory(|memory| memory.focused()).unwrap();
                let search = ctx.read_response(search_id).unwrap();
                let search_size = search.rect.size();
                let popup_id = search.layer_id.id;
                let popup_size = ctx
                    .memory(|memory| memory.area_rect(popup_id))
                    .unwrap()
                    .size();
                for _ in 0..90 {
                    frame(&ctx, &mut currency, &mut filter, vec![]);
                    assert_eq!(ctx.memory(|memory| memory.focused()), Some(search_id));
                    let current_search = ctx.read_response(search_id).unwrap().rect.size();
                    let current_popup = ctx
                        .memory(|memory| memory.area_rect(popup_id))
                        .unwrap()
                        .size();
                    for (name, current, initial) in [
                        ("search", current_search, search_size),
                        ("popup", current_popup, popup_size),
                    ] {
                        assert!(
                            (current.x - initial.x).abs() <= 1.
                                && (current.y - initial.y).abs() <= 1.,
                            "{name} grew while focused: {initial:?} → {current:?} ({theme:?}, {query:?})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn scrolling_results_keeps_search_pinned_and_does_not_snap_back() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
        // Start at the top so the wheel can scroll downward regardless of
        // egui automatically revealing the selected currency on open.
        let mut currency = "AED".to_owned();
        let mut filter = String::new();
        frame(&ctx, &mut currency, &mut filter, vec![]);
        let (center, _) = frame(&ctx, &mut currency, &mut filter, vec![]);
        click(&ctx, &mut currency, &mut filter, center);
        for _ in 0..3 {
            frame(&ctx, &mut currency, &mut filter, vec![]);
        }
        let search_id = ctx.memory(|memory| memory.focused()).unwrap();
        let search_rect = ctx.read_response(search_id).unwrap().rect;
        let pointer = search_rect.left_bottom() + egui::vec2(30., 80.);
        let (_, before) = frame(&ctx, &mut currency, &mut filter, vec![]);
        frame(
            &ctx,
            &mut currency,
            &mut filter,
            vec![
                egui::Event::PointerMoved(pointer),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                    delta: egui::vec2(0., -120.),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        for _ in 0..30 {
            frame(&ctx, &mut currency, &mut filter, vec![]);
        }
        let (_, labels) = frame(&ctx, &mut currency, &mut filter, vec![]);
        // Track a visible row shared by both frames; clipped rows need not
        // appear in egui's paint output.
        let (scrolled_label, scrolled_pos) = labels
            .iter()
            .find(|(text, pos)| {
                text.contains(" — ")
                    && before
                        .iter()
                        .any(|(old_text, old_pos)| old_text == text && pos.y < old_pos.y - 20.)
            })
            .unwrap_or_else(|| panic!("Results must move: before={before:?}, after={labels:?}"));
        let euro_y = scrolled_pos.y;
        for _ in 0..30 {
            let (_, labels) = frame(&ctx, &mut currency, &mut filter, vec![]);
            let current_y = labels
                .iter()
                .find(|(text, _)| text == scrolled_label)
                .unwrap()
                .1
                .y;
            assert!(
                (current_y - euro_y).abs() <= 1.,
                "Results snapped back while search was focused"
            );
            assert_eq!(ctx.memory(|memory| memory.focused()), Some(search_id));
            let current_search = ctx.read_response(search_id).unwrap().rect;
            assert!(
                (current_search.top() - search_rect.top()).abs() <= 1.,
                "Search must remain pinned"
            );
        }
        assert_eq!(currency, "AED");
        assert!(filter.is_empty());
    }

    #[test]
    fn selector_changes_currency_and_keeps_legacy_custom_choices() {
        let ctx = egui::Context::default();
        ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
        let mut currency = "credits".to_owned();
        let mut filter = String::new();
        frame(&ctx, &mut currency, &mut filter, vec![]);
        let (center, _) = frame(&ctx, &mut currency, &mut filter, vec![]);
        click(&ctx, &mut currency, &mut filter, center);
        let (_, labels) = frame(&ctx, &mut currency, &mut filter, vec![]);
        assert!(labels.iter().any(|(text, _)| text == "credits"));
        let canadian_dollar = labels
            .iter()
            .find(|(text, _)| text == "CAD — Canadian dollar")
            .unwrap()
            .1;
        click(&ctx, &mut currency, &mut filter, canadian_dollar);
        assert_eq!(currency, "CAD");
        let (_, labels) = frame(&ctx, &mut currency, &mut filter, vec![]);
        assert!(
            labels
                .iter()
                .any(|(text, _)| text == "CAD — Canadian dollar")
        );
    }

    #[test]
    fn search_matches_codes_and_names_and_enter_selects_the_single_result() {
        for (query, expected, expected_label) in [
            ("cAnAdIaN", "CAD", "CAD — Canadian dollar"),
            (" jPy ", "JPY", "JPY — Japanese yen"),
        ] {
            let ctx = egui::Context::default();
            ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
            let mut currency = "USD".to_owned();
            let mut filter = String::new();
            frame(&ctx, &mut currency, &mut filter, vec![]);
            let (center, _) = frame(&ctx, &mut currency, &mut filter, vec![]);
            click(&ctx, &mut currency, &mut filter, center);
            frame(&ctx, &mut currency, &mut filter, vec![]);
            frame(
                &ctx,
                &mut currency,
                &mut filter,
                vec![egui::Event::Text(query.into())],
            );
            let (_, labels) = frame(&ctx, &mut currency, &mut filter, vec![]);
            assert_eq!(filter, query);
            assert_eq!(
                currency, "USD",
                "Searching must not change the selected currency"
            );
            assert!(
                labels.iter().any(|(text, _)| text == expected_label),
                "{labels:?}"
            );
            assert!(
                !labels
                    .iter()
                    .any(|(text, _)| text == "AUD — Australian dollar")
            );
            frame(
                &ctx,
                &mut currency,
                &mut filter,
                vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert_eq!(currency, expected);
            assert_eq!(filter, query);
        }
    }

    #[test]
    fn unmatched_search_can_be_cleared_without_closing_the_picker() {
        let ctx = egui::Context::default();
        ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
        let mut currency = "CAD".to_owned();
        let mut filter = "no-such-currency".to_owned();
        frame(&ctx, &mut currency, &mut filter, vec![]);
        let (center, _) = frame(&ctx, &mut currency, &mut filter, vec![]);
        click(&ctx, &mut currency, &mut filter, center);
        let (_, labels) = frame(&ctx, &mut currency, &mut filter, vec![]);
        assert!(
            labels
                .iter()
                .any(|(text, _)| text == "No currencies match your search.")
        );
        let clear = labels.iter().find(|(text, _)| text == "Clear").unwrap().1;
        click(&ctx, &mut currency, &mut filter, clear);
        let (_, labels) = frame(&ctx, &mut currency, &mut filter, vec![]);
        assert!(filter.is_empty());
        assert_eq!(currency, "CAD");
        assert!(labels.iter().any(|(text, _)| text == "AED — UAE dirham"));
        assert!(
            labels
                .iter()
                .any(|(text, _)| text == "AUD — Australian dollar")
        );
    }

    #[test]
    fn legacy_currency_values_are_upgraded_without_losing_custom_values() {
        for (saved, expected) in [
            ("$", "USD"),
            ("", "USD"),
            (" cad ", "CAD"),
            ("EUR", "EUR"),
            ("€", "EUR"),
            ("£", "GBP"),
            ("CA$", "CAD"),
            (" R$ ", "BRL"),
            ("ISK", "ISK"),
            (" credits ", "credits"),
        ] {
            let mut value = saved.to_owned();
            normalize(&mut value);
            assert_eq!(value, expected);
            normalize(&mut value);
            assert_eq!(value, expected);
        }
    }

    #[test]
    fn amounts_use_currency_precision_without_repeating_currency() {
        assert_eq!(format_value("USD", 42.5), "42.50");
        assert_eq!(format_value("CAD ", 42.5), "42.50");
        assert_eq!(format_value("€", 42.5), "42.50");
        assert_eq!(format_value("JPY", 1234.), "1234");
        assert_eq!(format_value("KRW", 0.), "0");
        assert_eq!(format_value("credits", 12.), "12.00");
    }
}
