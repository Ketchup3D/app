use super::*;
use ketchup_core::{DisplaySettings, FaceStyle, Projection};

#[path = "view_controls.rs"]
mod view_controls;

#[path = "outliner.rs"]
mod outliner;

const ORDER_PREFERENCE_KEY: &str = "ketchup.inspector-order.v1";

const PREFERENCE_KEY: &str = "ketchup.inspector.v1";
const OUTLINER_SEARCH_ID: &str = "outliner_search";

// Persist the whole settings object: new DisplaySettings fields automatically
// participate, while serde defaults keep older preferences compatible.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct InspectorPreferences {
    visible: bool,
    width: f32,
    component_filter: String,
    outliner_filter: String,
    paint_color: [u8; 3],
    last_seen_release_notes: Option<String>,
    display: Option<DisplaySettings>,
    camera: Option<CameraPreferences>,
    sections: std::collections::BTreeMap<String, bool>,
}

impl Default for InspectorPreferences {
    fn default() -> Self {
        Self {
            visible: true,
            width: 300.,
            component_filter: String::new(),
            outliner_filter: String::new(),
            paint_color: [195, 96, 67],
            last_seen_release_notes: None,
            display: None,
            camera: None,
            sections: Default::default(),
        }
    }
}

// Keep framing (target and distance) in the document, so opening another
// project never inherits the previous model's position or scale.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct CameraPreferences {
    projection: Projection,
    fov: f64,
    yaw: f64,
    pitch: f64,
}

impl Default for CameraPreferences {
    fn default() -> Self {
        Self::capture(&Camera::default())
    }
}

impl CameraPreferences {
    fn capture(camera: &Camera) -> Self {
        Self {
            projection: camera.projection,
            fov: camera.fov,
            yaw: camera.yaw,
            pitch: camera.pitch,
        }
    }

    fn restore(&self, camera: &mut Camera) {
        let candidate = Camera {
            projection: self.projection,
            fov: self.fov,
            yaw: self.yaw,
            pitch: self.pitch,
            ..camera.clone()
        };
        if candidate.valid() {
            *camera = candidate;
        }
    }
}

#[derive(Clone, Default)]
struct SectionMemory(
    std::collections::BTreeMap<String, bool>,
    std::collections::BTreeMap<String, egui::Id>,
);

fn section_memory_id() -> egui::Id {
    egui::Id::unique(PREFERENCE_KEY)
}

// All inspector disclosures (including future ones) use these helpers, with a
// stable key independent of their position, title, and docked/floating layout.
pub(super) fn restore_section(ctx: &egui::Context, id: egui::Id, key: &str, default_open: bool) {
    let open = ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<SectionMemory>(section_memory_id())
            .0
            .get(key)
            .copied()
    });
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, default_open);
    if let Some(open) = open {
        state.set_open(open);
    }
    state.store(ctx);
}

// CollapsingHeader owns its internal UI scope. Force the remembered value only
// when entering a different scope, then let egui handle clicks normally.
pub(super) fn restored_header_open(ui: &egui::Ui, key: &str, default_open: bool) -> Option<bool> {
    ui.ctx().data_mut(|data| {
        let memory = data.get_temp_mut_or_default::<SectionMemory>(section_memory_id());
        let previous_scope = memory.1.insert(key.to_owned(), ui.scope_id());
        (previous_scope != Some(ui.scope_id()))
            .then(|| memory.0.get(key).copied().unwrap_or(default_open))
    })
}

pub(super) fn remember_section(ctx: &egui::Context, id: egui::Id, key: &str) {
    if let Some(state) = egui::collapsing_header::CollapsingState::load(ctx, id) {
        ctx.data_mut(|data| {
            data.get_temp_mut_or_default::<SectionMemory>(section_memory_id())
                .0
                .insert(key.to_owned(), state.is_open());
        });
    }
}

fn set_section_open(ctx: &egui::Context, key: &str, open: bool) {
    ctx.data_mut(|data| {
        let memory = data.get_temp_mut_or_default::<SectionMemory>(section_memory_id());
        memory.0.insert(key.to_owned(), open);
        // Apply the new value even to a CollapsingHeader already in this scope.
        memory.1.remove(key);
    });
}

impl KetchupApp {
    pub(super) fn request_outliner_search(&mut self, ctx: &egui::Context) {
        self.inspector = true;
        self.focus_outliner_search = true;
        set_section_open(ctx, "outliner", true);
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::unique(OUTLINER_SEARCH_ID)));
        ctx.request_repaint();
    }

    pub(super) fn restore_inspector_preferences(
        &mut self,
        ctx: &egui::Context,
        storage: Option<&dyn eframe::Storage>,
    ) {
        self.inspector_order = restore_order(storage);
        let preferences = storage
            .and_then(|storage| eframe::get_value::<InspectorPreferences>(storage, PREFERENCE_KEY))
            .unwrap_or_default();
        self.inspector = preferences.visible;
        self.inspector_width = if preferences.width.is_finite() {
            preferences.width.clamp(260., 2000.)
        } else {
            300.
        };
        self.component_filter = preferences.component_filter.clone();
        self.outliner_filter = preferences.outliner_filter.clone();
        self.paint_color = preferences.paint_color;
        self.release_notes.last_seen_version = preferences.last_seen_release_notes.clone();
        if let Some(display) = &preferences.display
            && display.valid()
        {
            self.doc.display = display.clone();
        }
        if let Some(camera) = &preferences.camera {
            camera.restore(&mut self.doc.camera);
        }
        ctx.data_mut(|data| {
            data.insert_temp(
                section_memory_id(),
                SectionMemory(preferences.sections.clone(), Default::default()),
            )
        });
        self.inspector_preferences = preferences;
    }

    fn current_inspector_preferences(&self, ctx: &egui::Context) -> InspectorPreferences {
        InspectorPreferences {
            visible: self.inspector,
            width: self.inspector_width,
            component_filter: self.component_filter.clone(),
            outliner_filter: self.outliner_filter.clone(),
            paint_color: self.paint_color,
            last_seen_release_notes: self.release_notes.last_seen_version.clone(),
            display: Some(self.doc.display.clone()),
            camera: Some(CameraPreferences::capture(&self.doc.camera)),
            sections: ctx.data_mut(|data| {
                data.get_temp_mut_or_default::<SectionMemory>(section_memory_id())
                    .0
                    .clone()
            }),
        }
    }

    pub(super) fn save_inspector_preferences(&self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, PREFERENCE_KEY, &self.inspector_preferences);
        save_order(&self.inspector_order, storage);
    }

    pub(super) fn persist_inspector_changes<'storage>(
        &mut self,
        ctx: &egui::Context,
        storage: Option<&mut (dyn eframe::Storage + 'storage)>,
        order_changed: bool,
    ) {
        let preferences = self.current_inspector_preferences(ctx);
        let changed = preferences != self.inspector_preferences;
        self.inspector_preferences = preferences;
        if (changed || order_changed)
            && let Some(storage) = storage
        {
            self.save_inspector_preferences(storage);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum InspectorSection {
    Outliner,
    EntityInfo,
    Components,
    Tags,
    Display,
    View,
}

pub(super) fn restore_order(storage: Option<&dyn eframe::Storage>) -> [InspectorSection; 6] {
    let Some(saved) = storage.and_then(|storage| {
        eframe::get_value::<Vec<InspectorSection>>(storage, ORDER_PREFERENCE_KEY)
            // The original preference used a fixed five-element RON tuple.
            .or_else(|| {
                eframe::get_value::<[InspectorSection; 5]>(storage, ORDER_PREFERENCE_KEY)
                    .map(Vec::from)
            })
    }) else {
        return InspectorSection::ALL;
    };
    let mut order = Vec::new();
    for section in saved {
        if order.contains(&section) {
            return InspectorSection::ALL;
        }
        order.push(section);
    }
    for section in InspectorSection::ALL {
        if !order.contains(&section) {
            order.push(section);
        }
    }
    order.try_into().unwrap_or(InspectorSection::ALL)
}

pub(super) fn save_order(order: &[InspectorSection; 6], storage: &mut dyn eframe::Storage) {
    eframe::set_value(storage, ORDER_PREFERENCE_KEY, &order.to_vec());
    storage.flush();
}

#[cfg(test)]
mod preference_tests {
    use super::*;
    use eframe::Storage;

    #[derive(Default)]
    struct SavedPreferences {
        values: std::collections::BTreeMap<String, String>,
        flushed: bool,
    }

    impl Storage for SavedPreferences {
        fn get_string(&self, key: &str) -> Option<String> {
            self.values.get(key).cloned()
        }

        fn set_string(&mut self, key: &str, value: String) {
            self.values.insert(key.to_owned(), value);
        }

        fn remove_string(&mut self, key: &str) {
            self.values.remove(key);
        }

        fn flush(&mut self) {
            self.flushed = true;
        }
    }

    #[test]
    fn saved_order_survives_recreating_the_app() {
        let mut storage = SavedPreferences::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.inspector_order.rotate_left(2);
        let order = app.inspector_order;
        eframe::App::save(&mut app, &mut storage);
        assert!(storage.flushed);
        assert!(!app.project.modified);
        drop(app);

        let mut refreshed = KetchupApp::with_document(Document::default(), None, true);
        refreshed.inspector_order = restore_order(Some(&storage));
        assert_eq!(refreshed.inspector_order, order);
    }

    #[test]
    fn outliner_search_persists_immediately_and_defaults_to_empty() {
        let ctx = egui::Context::default();
        let mut storage = SavedPreferences::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.outliner_filter = "Handle".into();
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        assert!(!app.project.modified);

        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        restored.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
        assert_eq!(restored.outliner_filter, "Handle");
        let old: InspectorPreferences = serde_json::from_str("{}").unwrap();
        assert!(old.outliner_filter.is_empty());

        restored.outliner_filter.clear();
        restored.persist_inspector_changes(&ctx, Some(&mut storage), false);
        app.restore_inspector_preferences(&ctx, Some(&storage));
        assert!(app.outliner_filter.is_empty());
    }

    #[test]
    fn find_shortcut_reveals_and_focuses_outliner_and_persists_expansion() {
        for width in [1440., 800.] {
            for modifiers in [
                egui::Modifiers::CTRL,
                egui::Modifiers {
                    ctrl: true,
                    command: true,
                    ..Default::default()
                },
                egui::Modifiers {
                    mac_cmd: true,
                    command: true,
                    ..Default::default()
                },
            ] {
                let ctx = egui::Context::default();
                ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
                let mut app = KetchupApp::with_document(Document::default(), None, true);
                app.outliner_filter = "Cabinet".into();
                set_section_open(&ctx, "outliner", false);
                let screen_rect = Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(width, 900.)));
                let mut other_text = String::new();
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect,
                        ..Default::default()
                    },
                    |ui| {
                        app.inspector_panel(ui);
                        ui.add(egui::TextEdit::singleline(&mut other_text))
                            .request_focus();
                    },
                );
                assert!(ctx.egui_wants_keyboard_input());
                app.inspector = false;
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect,
                        modifiers,
                        events: vec![egui::Event::Key {
                            key: Key::F,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers,
                        }],
                        ..Default::default()
                    },
                    |ui| {
                        app.keyboard(&ctx);
                        assert!(!ctx.input(|input| input.key_pressed(Key::F)));
                        app.inspector_panel(ui);
                    },
                );
                // Allow newly opened windows and disclosures to finish their layout.
                for _ in 0..2 {
                    let _ = ctx.run_ui(
                        egui::RawInput {
                            screen_rect,
                            ..Default::default()
                        },
                        |ui| {
                            app.inspector_panel(ui);
                        },
                    );
                }
                assert!(app.inspector);
                assert!(!app.focus_outliner_search);
                assert_eq!(
                    ctx.memory(|memory| memory.focused()),
                    Some(egui::Id::unique(OUTLINER_SEARCH_ID))
                );
                assert_eq!(app.outliner_filter, "Cabinet");
                assert_eq!(app.tool, Tool::Select);

                let mut storage = SavedPreferences::default();
                app.persist_inspector_changes(&ctx, Some(&mut storage), false);
                assert!(!app.project.modified);
                let restored_ctx = egui::Context::default();
                let mut restored = KetchupApp::with_document(Document::default(), None, true);
                restored.restore_inspector_preferences(&restored_ctx, Some(&storage));
                assert!(restored.inspector);
                assert!(
                    restored
                        .current_inspector_preferences(&restored_ctx)
                        .sections["outliner"]
                );
            }
        }
    }

    #[test]
    fn unavailable_or_invalid_saved_order_uses_all_default_sections() {
        assert_eq!(restore_order(None), InspectorSection::ALL);
        let mut storage = SavedPreferences::default();
        assert_eq!(restore_order(Some(&storage)), InspectorSection::ALL);
        storage.set_string(ORDER_PREFERENCE_KEY, "corrupt preferences".into());
        assert_eq!(restore_order(Some(&storage)), InspectorSection::ALL);
        eframe::set_value(
            &mut storage,
            ORDER_PREFERENCE_KEY,
            &[InspectorSection::Tags; 5],
        );
        assert_eq!(restore_order(Some(&storage)), InspectorSection::ALL);
        eframe::set_value(
            &mut storage,
            ORDER_PREFERENCE_KEY,
            &InspectorSection::ALL[..4].to_vec(),
        );
        assert_eq!(restore_order(Some(&storage)), InspectorSection::ALL);
    }

    #[test]
    fn sidebar_preferences_save_immediately_and_restore_without_a_model_edit() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.inspector = false;
        app.inspector_width = 420.;
        app.component_filter = "Cabinet".into();
        app.paint_color = [12, 34, 56];
        let mut display = serde_json::to_value(DisplaySettings::default()).unwrap();
        // Exercise every toggle, including fields added to DisplaySettings later.
        for value in display.as_object_mut().unwrap().values_mut() {
            if let Some(enabled) = value.as_bool() {
                *value = serde_json::Value::Bool(!enabled);
            }
        }
        app.doc.display = serde_json::from_value(display).unwrap();
        app.doc.display.face_style = FaceStyle::Wireframe;
        app.doc.display.profile_width = 7.;
        app.doc.display.sun_angle = 280.;
        app.doc.display.sky_color = [3, 4, 5];
        app.doc.camera.projection = Projection::Parallel;
        app.doc.camera.fov = 65_f64.to_radians();
        app.doc
            .camera
            .set_standard_view(ketchup_core::StandardView::Bottom);
        for (key, open) in [
            ("display", false),
            ("fog", true),
            ("view", false),
            ("view_camera", true),
            ("view_standard_views", true),
            ("outliner_object/42", true),
        ] {
            let id = egui::Id::unique(key);
            let mut state =
                egui::collapsing_header::CollapsingState::load_with_default_open(&ctx, id, false);
            state.set_open(open);
            state.store(&ctx);
            remember_section(&ctx, id, key);
        }
        let expected = app.current_inspector_preferences(&ctx);
        let mut storage = SavedPreferences::default();
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        assert!(!app.project.modified);
        assert!(app.dirty_since.is_none());

        let refreshed_ctx = egui::Context::default();
        let mut refreshed = KetchupApp::with_document(Document::default(), None, true);
        refreshed.restore_inspector_preferences(&refreshed_ctx, Some(&storage));
        assert_eq!(
            refreshed.current_inspector_preferences(&refreshed_ctx),
            expected
        );
        assert!(!refreshed.project.modified);

        storage.flushed = false;
        refreshed.persist_inspector_changes(&refreshed_ctx, Some(&mut storage), false);
        assert!(
            !storage.flushed,
            "unchanged preferences must not write every frame"
        );
    }

    #[test]
    fn viewed_release_notes_save_immediately_and_restore_for_other_projects() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let mut storage = SavedPreferences::default();
        // Establish saved preferences first, so only the read state changes.
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        storage.flushed = false;
        assert!(app.release_notes.has_unread());
        app.release_notes.open_if_unread();
        assert!(app.release_notes.open);
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1000., 700.))),
                ..Default::default()
            },
            |_| app.release_notes.show(&ctx),
        );
        assert!(!app.release_notes.has_unread());
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        assert!(!app.project.modified);
        assert!(app.dirty_since.is_none());

        let mut other_doc = Document::default();
        other_doc.name = "Another project".into();
        let mut refreshed = KetchupApp::with_document(other_doc, None, true);
        let refreshed_ctx = egui::Context::default();
        refreshed.restore_inspector_preferences(&refreshed_ctx, Some(&storage));
        refreshed.release_notes.open_if_unread();
        assert!(!refreshed.release_notes.has_unread());
        assert!(!refreshed.release_notes.open);
        assert_eq!(refreshed.doc.name, "Another project");
        storage.flushed = false;
        refreshed.persist_inspector_changes(&refreshed_ctx, Some(&mut storage), false);
        assert!(!storage.flushed);

        // An older saved read version must allow the next release to open.
        let mut older = refreshed.current_inspector_preferences(&refreshed_ctx);
        older.last_seen_release_notes = Some("0.0.0".into());
        eframe::set_value(&mut storage, PREFERENCE_KEY, &older);
        let mut updated = KetchupApp::with_document(Document::default(), None, true);
        updated.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
        updated.release_notes.open_if_unread();
        assert!(updated.release_notes.open);
        assert!(updated.release_notes.has_unread());
    }

    #[test]
    fn bulk_expansion_overrides_existing_headers_and_restores_after_immediate_save() {
        fn render(ctx: &egui::Context, moved: bool) -> (bool, bool) {
            let mut visible = (false, false);
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.push_id(if moved { "floating" } else { "docked" }, |ui| {
                    let id = ui.make_persistent_id(("outliner_object", 42_u64));
                    restore_section(ctx, id, "outliner_object/42", false);
                    let state = egui::collapsing_header::CollapsingState::load(ctx, id).unwrap();
                    visible.0 = state.is_open();
                    remember_section(ctx, id, "outliner_object/42");
                    let key = "outliner_geometry/Some(42)";
                    let open = restored_header_open(ui, key, false);
                    let header = egui::CollapsingHeader::new("Geometry (1)")
                        .id_salt(("outliner_geometry", Some(42_u64)))
                        .open(open)
                        .show(ui, |ui| {
                            visible.1 = true;
                            ui.label("Edge");
                        });
                    remember_section(ctx, header.header_response.id, key);
                });
            });
            visible
        }

        let ctx = egui::Context::default();
        ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
        assert_eq!(render(&ctx, false), (false, false));
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let mut storage = SavedPreferences::default();
        for open in [true, false] {
            for key in ["outliner_object/42", "outliner_geometry/Some(42)"] {
                set_section_open(&ctx, key, open);
            }
            // Save before another render, just as an immediate reload would.
            storage.flushed = false;
            app.persist_inspector_changes(&ctx, Some(&mut storage), false);
            assert!(storage.flushed);
            assert_eq!(render(&ctx, false), (open, open));
            assert_eq!(render(&ctx, false), (open, open));

            let refreshed_ctx = egui::Context::default();
            refreshed_ctx.style_mut_of(refreshed_ctx.theme(), |style| style.animation_time = 0.);
            let mut refreshed = KetchupApp::with_document(Document::default(), None, true);
            refreshed.restore_inspector_preferences(&refreshed_ctx, Some(&storage));
            assert_eq!(render(&refreshed_ctx, true), (open, open));
            assert!(!refreshed.project.modified);
        }
        assert!(!app.project.modified);
        assert!(app.dirty_since.is_none());
    }

    #[test]
    fn missing_corrupt_or_invalid_preferences_preserve_document_display() {
        let ctx = egui::Context::default();
        let mut doc = Document::default();
        doc.display.axes = false;
        let mut app = KetchupApp::with_document(doc.clone(), None, true);
        app.restore_inspector_preferences(&ctx, None);
        assert_eq!(app.doc.display, doc.display);
        let mut storage = SavedPreferences::default();
        storage.set_string(PREFERENCE_KEY, "corrupt preferences".into());
        app.restore_inspector_preferences(&ctx, Some(&storage));
        assert_eq!(app.doc.display, doc.display);
        assert!(app.inspector);
        assert_eq!(app.inspector_width, 300.);

        let mut preferences = InspectorPreferences::default();
        let mut display = DisplaySettings::default();
        display.fog_end = display.fog_start;
        preferences.display = Some(display);
        preferences.width = -20.;
        eframe::set_value(&mut storage, PREFERENCE_KEY, &preferences);
        app.restore_inspector_preferences(&ctx, Some(&storage));
        assert_eq!(app.doc.display, doc.display);
        assert_eq!(app.inspector_width, 260.);

        let older: InspectorPreferences =
            serde_json::from_str(r#"{"display":{"axes":false}}"#).unwrap();
        assert!(older.visible);
        assert!(older.sections.is_empty());
        assert!(older.camera.is_none());
        assert!(older.last_seen_release_notes.is_none());
        let display = older.display.unwrap();
        assert!(!display.axes);
        assert_eq!(display.sky, DisplaySettings::default().sky);
        assert!(display.valid());
    }

    #[test]
    fn original_fixed_array_order_migrates_without_losing_layout() {
        let original = [
            InspectorSection::Tags,
            InspectorSection::Display,
            InspectorSection::Outliner,
            InspectorSection::EntityInfo,
            InspectorSection::Components,
        ];
        let expected = [
            InspectorSection::Tags,
            InspectorSection::Display,
            InspectorSection::Outliner,
            InspectorSection::EntityInfo,
            InspectorSection::Components,
            InspectorSection::View,
        ];
        let mut storage = SavedPreferences::default();
        eframe::set_value(&mut storage, ORDER_PREFERENCE_KEY, &original);
        assert_eq!(restore_order(Some(&storage)), expected);
        save_order(&expected, &mut storage);
        assert_eq!(restore_order(Some(&storage)), expected);
    }

    #[test]
    fn older_section_orders_keep_their_layout_and_append_new_sections() {
        let mut storage = SavedPreferences::default();
        eframe::set_value(
            &mut storage,
            ORDER_PREFERENCE_KEY,
            &vec![InspectorSection::Tags, InspectorSection::Display],
        );
        assert_eq!(
            restore_order(Some(&storage)),
            [
                InspectorSection::Tags,
                InspectorSection::Display,
                InspectorSection::Outliner,
                InspectorSection::EntityInfo,
                InspectorSection::Components,
                InspectorSection::View,
            ]
        );
    }

    #[test]
    fn new_sections_restore_after_click_save_reload_and_layout_change() {
        fn render(ctx: &egui::Context, events: Vec<egui::Event>, moved: bool) -> (Rect, bool) {
            let mut rect = Rect::NOTHING;
            let mut body_visible = false;
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(600., 900.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.push_id(if moved { "floating" } else { "docked" }, |ui| {
                        rect =
                            inspector_section(ui, "Future option", "future_option", false, |ui| {
                                body_visible = true;
                                ui.label("Future controls");
                            })
                            .inner
                            .rect;
                    });
                },
            );
            (rect, body_visible)
        }
        let ctx = egui::Context::default();
        ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
        assert!(!render(&ctx, vec![], false).1);
        let (rect, _) = render(&ctx, vec![], false);
        for pressed in [true, false] {
            render(
                &ctx,
                vec![
                    egui::Event::PointerMoved(rect.center()),
                    egui::Event::PointerButton {
                        pos: rect.center(),
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                false,
            );
        }
        assert!(render(&ctx, vec![], false).1);
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let mut storage = SavedPreferences::default();
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        let new_ctx = egui::Context::default();
        new_ctx.style_mut_of(new_ctx.theme(), |style| style.animation_time = 0.);
        let mut refreshed = KetchupApp::with_document(Document::default(), None, true);
        refreshed.restore_inspector_preferences(&new_ctx, Some(&storage));
        assert!(render(&new_ctx, vec![], true).1);
    }

    #[test]
    fn view_preferences_restore_groups_without_copying_another_models_framing() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.doc.camera.projection = Projection::Parallel;
        app.doc.camera.fov = 70_f64.to_radians();
        app.doc
            .camera
            .set_standard_view(ketchup_core::StandardView::Top);
        app.doc.camera.target = DVec3::splat(500.);
        app.doc.camera.distance = 300.;
        for (key, open) in [
            ("view", false),
            ("view_camera", true),
            ("view_standard_views", true),
        ] {
            set_section_open(&ctx, key, open);
        }
        let mut storage = SavedPreferences::default();
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        assert!(!app.project.modified);
        let mut doc = Document::default();
        doc.camera.target = DVec3::new(1., 2., 3.);
        doc.camera.distance = 42.;
        let refreshed_ctx = egui::Context::default();
        let mut refreshed = KetchupApp::with_document(doc.clone(), None, true);
        refreshed.restore_inspector_preferences(&refreshed_ctx, Some(&storage));
        assert_eq!(refreshed.doc.camera.target, doc.camera.target);
        assert_eq!(refreshed.doc.camera.distance, doc.camera.distance);
        assert_eq!(
            CameraPreferences::capture(&refreshed.doc.camera),
            CameraPreferences::capture(&app.doc.camera)
        );
        let _ = refreshed_ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.push_id("floating_view", |ui| {
                assert_eq!(restored_header_open(ui, "view", true), Some(false));
                assert_eq!(restored_header_open(ui, "view_camera", false), Some(true));
                assert_eq!(
                    restored_header_open(ui, "view_standard_views", false),
                    Some(true)
                );
            });
        });
        let mut preferences = refreshed.current_inspector_preferences(&refreshed_ctx);
        preferences.camera.as_mut().unwrap().fov = f64::NAN;
        // Exercise validation directly: JSON/RON need not preserve non-finite values.
        let before = refreshed.doc.camera.clone();
        preferences
            .camera
            .unwrap()
            .restore(&mut refreshed.doc.camera);
        assert_eq!(refreshed.doc.camera, before);
    }

    #[test]
    fn new_projects_keep_preferences_and_opened_projects_use_their_own_display() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.doc.display.axes = false;
        app.doc.display.fog = true;
        app.component_filter = "Cabinet".into();
        app.doc.camera.projection = Projection::Parallel;
        app.doc.camera.fov = 60_f64.to_radians();
        app.doc
            .camera
            .set_standard_view(ketchup_core::StandardView::Top);
        app.doc.camera.target = DVec3::splat(100.);
        app.doc.camera.distance = 200.;
        let camera_preferences = CameraPreferences::capture(&app.doc.camera);
        app.new_model(&ctx);
        assert!(!app.doc.display.axes);
        assert!(app.doc.display.fog);
        assert_eq!(app.component_filter, "Cabinet");
        assert_eq!(
            CameraPreferences::capture(&app.doc.camera),
            camera_preferences
        );
        assert_eq!(app.doc.camera.target, Camera::default().target);
        assert_eq!(app.doc.camera.distance, Camera::default().distance);
        assert!(!app.project.modified);
        app.replace_project(Document::default(), Some("opened.ketchup".into()), &ctx);
        assert_eq!(app.doc.display, DisplaySettings::default());
        assert_eq!(app.doc.camera, Camera::default());
        assert_eq!(app.component_filter, "Cabinet");
    }
}

impl InspectorSection {
    pub(super) const ALL: [Self; 6] = [
        Self::Outliner,
        Self::EntityInfo,
        Self::Components,
        Self::Tags,
        Self::Display,
        Self::View,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Outliner => "Outliner",
            Self::EntityInfo => "Entity Info",
            Self::Components => "Components",
            Self::Tags => "Tags",
            Self::Display => "Display",
            Self::View => "View",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Outliner => "outliner",
            Self::EntityInfo => "entity_info",
            Self::Components => "components",
            Self::Tags => "tags",
            Self::Display => "display",
            Self::View => "view",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Default)]
pub(super) struct InspectorLayout {
    heights: [f32; 6],
    header_heights: [f32; 6],
    drag: Option<InspectorDrag>,
    floating: Option<InspectorSection>,
}

struct InspectorDrag {
    section: InspectorSection,
    grab_offset: Vec2,
    order: [InspectorSection; 6],
}

impl InspectorLayout {
    fn preview_order(&mut self, header_top: f32) {
        let Some(drag) = &mut self.drag else {
            return;
        };
        let center = header_top + self.header_heights[drag.section.index()] * 0.5;
        let mut from = drag.order.iter().position(|s| *s == drag.section).unwrap();
        // Compare with target slots, rather than animated positions, so panels
        // cannot oscillate underneath a stationary pointer.
        while from > 0 {
            let neighbor = drag.order[from - 1];
            let top: f32 = drag.order[..from - 1]
                .iter()
                .map(|s| self.heights[s.index()])
                .sum();
            if center >= top + self.header_heights[neighbor.index()] * 0.5 {
                break;
            }
            drag.order.swap(from, from - 1);
            from -= 1;
        }
        while from + 1 < drag.order.len() {
            let neighbor = drag.order[from + 1];
            let top: f32 = drag.order[..=from]
                .iter()
                .map(|s| self.heights[s.index()])
                .sum();
            if center <= top + self.header_heights[neighbor.index()] * 0.5 {
                break;
            }
            drag.order.swap(from, from + 1);
            from += 1;
        }
    }
}

fn face_style_preview(
    painter: &egui::Painter,
    rect: Rect,
    style: FaceStyle,
    colors: Palette,
    faces: FacePalette,
) {
    let center = rect.center();
    let scale = rect.width().min(rect.height()) * 0.46;
    let point = |x, y| center + egui::vec2(x * scale, y * scale);
    let top = point(0., -1.);
    let left = point(-0.9, -0.55);
    let front = point(0., -0.1);
    let right = point(0.9, -0.55);
    let bottom_left = point(-0.9, 0.55);
    let bottom = point(0., 1.);
    let bottom_right = point(0.9, 0.55);
    let outline = Stroke::new(1.2, colors.ink);
    if style == FaceStyle::Wireframe {
        let back = point(0., 0.1);
        for end in [top, bottom_left, bottom_right] {
            painter.line_segment([back, end], Stroke::new(1., colors.muted));
        }
    } else {
        let fills = match style {
            FaceStyle::HiddenLine => [colors.paper; 3],
            FaceStyle::Monochrome => [faces.front, faces.side, faces.back],
            FaceStyle::Materials => faces.materials,
            FaceStyle::Shaded => [faces.front, faces.side, faces.shaded_side],
            FaceStyle::Wireframe => unreachable!(),
        };
        for (points, fill) in [
            ([top, right, front, left], fills[0]),
            ([left, front, bottom, bottom_left], fills[1]),
            ([front, right, bottom_right, bottom], fills[2]),
        ] {
            painter.add(egui::Shape::convex_polygon(
                points.to_vec(),
                fill,
                Stroke::NONE,
            ));
        }
    }
    painter.add(egui::Shape::closed_line(
        vec![top, right, bottom_right, bottom, bottom_left, left],
        outline,
    ));
    for end in [left, right, bottom] {
        painter.line_segment([front, end], outline);
    }
}

fn face_style_picker(ui: &mut egui::Ui, current: &mut FaceStyle) {
    let colors = Palette::for_ui(ui);
    let faces = FacePalette::for_ui(ui);
    let spacing = ui.spacing().item_spacing.x;
    let columns = ((ui.available_width() + spacing) / (96. + spacing))
        .floor()
        .clamp(1., FaceStyle::ALL.len() as f32) as usize;
    let width = (ui.available_width() - spacing * (columns - 1) as f32) / columns as f32;
    ui.push_id("face_style", |ui| {
        for row in FaceStyle::ALL.chunks(columns) {
            ui.horizontal(|ui| {
                for &style in row {
                    let selected = *current == style;
                    let mut response = ui
                        .add_sized(
                            egui::vec2(width, 100.),
                            egui::Button::new("").selected(selected).corner_radius(5),
                        )
                        .on_hover_text(style.label());
                    if response.clicked() && !selected {
                        *current = style;
                        response.mark_changed();
                    }
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::Role::RadioButton,
                            ui.is_enabled(),
                            *current == style,
                            style.label(),
                        )
                    });
                    if ui.is_rect_visible(response.rect) {
                        let rect = response.rect;
                        face_style_preview(
                            ui.painter(),
                            Rect::from_center_size(
                                egui::pos2(rect.center().x, rect.top() + 33.),
                                egui::vec2(width - 16., 50.),
                            ),
                            style,
                            colors,
                            faces,
                        );
                        let mut label = egui::text::LayoutJob::simple(
                            style.label().to_owned(),
                            FontId::proportional(11.),
                            colors.ink,
                            width - 12.,
                        );
                        label.halign = egui::Align::Center;
                        let galley = ui.fonts_mut(|fonts| fonts.layout_job(label));
                        let pos = egui::pos2(
                            rect.center().x,
                            rect.bottom() - 21. - galley.size().y * 0.5,
                        );
                        ui.painter().galley(pos, galley, colors.ink);
                    }
                }
            });
        }
    });
}

fn inspector_icon_color(ui: &egui::Ui, id: &str) -> Color32 {
    // Keep each section's hue consistent, with lighter strokes on dark surfaces.
    let (light, dark) = match id {
        "outliner" => ([48, 111, 173], [131, 185, 239]),
        "entity_info" => ([17, 129, 133], [103, 207, 211]),
        "components" => ([126, 83, 183], [188, 153, 239]),
        "tags" => ([160, 111, 22], [230, 190, 104]),
        "display" => ([198, 76, 58], [246, 142, 125]),
        "view" | "view_camera" => ([48, 111, 173], [131, 185, 239]),
        "view_standard_views" => ([17, 129, 133], [103, 207, 211]),
        "edge_style" => ([88, 109, 135], [159, 181, 209]),
        "sections" => ([176, 67, 113], [237, 142, 181]),
        "shadows" => ([183, 92, 19], [242, 170, 100]),
        "fog" => ([117, 105, 155], [181, 169, 219]),
        "display_environment" => ([67, 131, 68], [143, 205, 144]),
        _ => return Palette::for_ui(ui).green,
    };
    let [r, g, b] = if ui.visuals().dark_mode { dark } else { light };
    Color32::from_rgb(r, g, b)
}

// Keep egui's disclosure behavior, animation, and accessibility while giving
// inspector groups a shared surface and a full-width click target.
fn inspector_section<R>(
    ui: &mut egui::Ui,
    title: &str,
    id: &'static str,
    primary: bool,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<egui::Response> {
    let colors = Palette::for_ui(ui);
    let icon_color = inspector_icon_color(ui, id);
    let body_style = ui.style().as_ref().clone();
    let response = egui::Frame::new()
        .fill(colors.surface)
        .stroke(Stroke::new(1., colors.border))
        .corner_radius(if primary { 8 } else { 5 })
        .inner_margin(1)
        .shadow(if primary {
            egui::epaint::Shadow {
                offset: [0, 2],
                blur: 5,
                spread: 0,
                color: Color32::from_black_alpha(12),
            }
        } else {
            egui::epaint::Shadow::NONE
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.scope(|ui| {
                ui.spacing_mut().button_padding = egui::vec2(10., if primary { 10. } else { 7. });
                ui.spacing_mut().indent = 52.;
                let visuals = ui.visuals_mut();
                visuals.collapsing_header_frame = true;
                for widget in [
                    &mut visuals.widgets.inactive,
                    &mut visuals.widgets.hovered,
                    &mut visuals.widgets.active,
                ] {
                    widget.corner_radius = egui::CornerRadius::same(if primary { 7 } else { 4 });
                    widget.bg_stroke = Stroke::NONE;
                    widget.expansion = 0.;
                }
                visuals.widgets.inactive.weak_bg_fill =
                    if primary { colors.canvas } else { colors.paper };
                visuals.widgets.hovered.weak_bg_fill = colors.hovered;
                visuals.widgets.active.weak_bg_fill = colors.selected;
                let open = restored_header_open(ui, id, primary);
                let header = egui::CollapsingHeader::new(RichText::new(title).strong())
                    .id_salt(id)
                    .default_open(primary)
                    .open(open)
                    .icon(move |ui, openness, response| {
                        let center = response.rect.center() - egui::vec2(12., 0.);
                        icons::line_icon(
                            ui.painter(),
                            center + egui::vec2(24., 0.),
                            id,
                            icon_color,
                        );
                        let rotation =
                            egui::emath::Rot2::from_angle(openness * std::f32::consts::FRAC_PI_2);
                        let points = [
                            egui::vec2(-2., -4.),
                            egui::vec2(2., 0.),
                            egui::vec2(-2., 4.),
                        ]
                        .map(|point| center + rotation * point);
                        ui.painter().add(egui::Shape::line(
                            points.to_vec(),
                            Stroke::new(
                                1.6,
                                if openness > 0. {
                                    colors.green
                                } else {
                                    colors.muted
                                },
                            ),
                        ));
                    })
                    .show_unindented(ui, |ui| {
                        *ui.style_mut() = body_style;
                        egui::Frame::new().inner_margin(10).show(ui, contents).inner
                    })
                    .header_response;
                remember_section(ui.ctx(), header.id, id);
                header
            })
            .inner
        });
    ui.add_space(if primary { 4. } else { 1. });
    response
}

impl KetchupApp {
    pub(super) fn inspector_contents(&mut self, ui: &mut egui::Ui) {
        let mut layout = std::mem::take(&mut self.inspector_layout);
        let origin = ui.cursor().min;
        let width = ui.available_width();
        let clip = ui.clip_rect();
        let drop_rect = Rect::from_min_max(
            egui::pos2(origin.x, clip.top()),
            egui::pos2(origin.x + width, clip.bottom()),
        );
        let pointer = ui.ctx().pointer_interact_pos();
        let animation_time = ui.style().animation_time;
        let spacing = ui.spacing().item_spacing.y;
        let colors = Palette::for_ui(ui);
        // egui clears the payload on Escape or release, including a release
        // while the inspector is hidden. Retain the floating panel as it settles.
        if !egui::DragAndDrop::has_payload_of_type::<InspectorSection>(ui.ctx()) {
            layout.drag = None;
        }
        if let (Some(drag), Some(pointer)) = (&layout.drag, pointer)
            && drop_rect.contains(pointer)
        {
            let header_top = pointer.y - origin.y - drag.grab_offset.y;
            layout.preview_order(header_top);
        }
        let order = layout
            .drag
            .as_ref()
            .map_or(self.inspector_order, |d| d.order);
        let mut top = 0.;
        for section in order {
            let index = section.index();
            let id = egui::Id::unique(("inspector_section", section.id()));
            let target = egui::vec2(0., top);
            let following = layout.drag.as_ref().filter(|d| d.section == section);
            let mut floating = layout.floating == Some(section);
            let desired = following
                .and_then(|drag| pointer.map(|p| p - origin - drag.grab_offset))
                .unwrap_or(target);
            let duration = if following.is_some() {
                0.
            } else {
                animation_time
            };
            let position = egui::vec2(
                ui.ctx()
                    .animate_value_with_time(id.with("x"), desired.x, duration),
                ui.ctx()
                    .animate_value_with_time(id.with("y"), desired.y, duration),
            );
            if floating && following.is_none() && (position - target).length() < 0.5 {
                layout.floating = None;
                floating = false;
            }
            // Reserve the complete panel's slot while its real contents follow
            // the pointer on a foreground layer. Each panel is rendered once.
            if following.is_some() {
                let slot = Rect::from_min_size(
                    origin + target,
                    egui::vec2(width, (layout.heights[index] - spacing - 4.).max(0.)),
                );
                ui.painter().rect(
                    slot,
                    8,
                    colors.canvas,
                    Stroke::new(1., colors.green),
                    egui::StrokeKind::Inside,
                );
            }
            let pos = origin + position;
            let mut builder = egui::UiBuilder::new()
                .scope_id(id)
                .max_rect(Rect::from_min_max(
                    pos,
                    egui::pos2(pos.x + width, f32::INFINITY),
                ))
                .layout(egui::Layout::top_down(egui::Align::Min));
            if floating {
                builder = builder.layer_id(egui::LayerId::new(egui::Order::Tooltip, id));
            }
            let mut panel_ui = ui.new_child(builder);
            if floating {
                panel_ui.set_clip_rect(ui.ctx().viewport_rect());
            }
            let panel =
                inspector_section(&mut panel_ui, section.title(), section.id(), true, |ui| {
                    match section {
                        InspectorSection::Outliner => self.outliner_contents(ui),
                        InspectorSection::EntityInfo => self.entity_info_contents(ui),
                        InspectorSection::Components => self.components_contents(ui),
                        InspectorSection::Tags => self.tags_contents(ui),
                        InspectorSection::Display => self.display_contents(ui),
                        InspectorSection::View => self.view_contents(ui),
                    }
                });
            let header = panel
                .inner
                .interact(egui::Sense::click_and_drag())
                .on_hover_cursor(egui::CursorIcon::Grab)
                .on_hover_text("Drag to reorder this section");
            layout.heights[index] = panel_ui.min_rect().height() + spacing;
            layout.header_heights[index] = header.rect.height();
            top += layout.heights[index];
            if layout.drag.is_none() && header.drag_started_by(egui::PointerButton::Primary) {
                let press = ui
                    .input(|i| i.pointer.press_origin())
                    .unwrap_or(header.rect.center());
                layout.drag = Some(InspectorDrag {
                    section,
                    grab_offset: press - panel.response.rect.min,
                    order: self.inspector_order,
                });
                layout.floating = Some(section);
                egui::DragAndDrop::set_payload(ui.ctx(), section);
                ui.ctx().request_repaint();
            }
        }
        // Only the slots affect scroll extent; a floating panel never pushes
        // other content or changes the sidebar width.
        ui.allocate_space(egui::vec2(width, (top - spacing).max(0.)));
        if let (Some(drag), Some(pointer)) = (&layout.drag, pointer) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            if drop_rect.contains(pointer) {
                let header_top = pointer.y - origin.y - drag.grab_offset.y;
                layout.preview_order(header_top);
                if ui.input(|i| i.pointer.primary_down()) {
                    let edge = 32.;
                    let speed = ((clip.top() + edge - pointer.y).max(0.)
                        - (pointer.y - (clip.bottom() - edge)).max(0.))
                        * 12.;
                    if speed != 0. {
                        let dt = ui.input(|i| i.stable_dt.min(0.05));
                        ui.scroll_with_delta_animation(
                            egui::vec2(0., speed * dt),
                            egui::style::ScrollAnimation::none(),
                        );
                        ui.ctx().request_repaint();
                    }
                }
            }
            if ui.input(|i| i.pointer.button_released(egui::PointerButton::Primary)) {
                if drop_rect.contains(pointer) {
                    self.inspector_order = layout.drag.as_ref().unwrap().order;
                }
                layout.drag = None;
                egui::DragAndDrop::clear_payload(ui.ctx());
                ui.ctx().request_repaint();
            }
        }
        self.inspector_layout = layout;
    }

    pub(super) fn begin_component_placement(&mut self, id: u64, ctx: &egui::Context) {
        if !self
            .doc
            .model
            .objects
            .get(&id)
            .is_some_and(|o| matches!(o.kind, ObjectKind::Component { .. }))
        {
            return;
        }
        let mut model = self.doc.model.copy_entities(&BTreeSet::from([id]));
        // An instance placed from the library starts visible and unlocked.
        model.hidden.remove(&id);
        model.locked.remove(&id);
        model.entity_tags.remove(&id);
        let points = model.vertices.values().map(|v| v.position).chain(
            model
                .dimensions
                .values()
                .flat_map(|dimension| dimension.layout(&model)),
        );
        let mut points = points.chain(model.texts.values().flat_map(|t| t.layout(&model)));
        let Some(first) = points.next() else {
            self.say(ctx, "This component is empty");
            return;
        };
        let anchor = points.fold(first, |lo, point| lo.min(point));
        let clipboard = GeometryClipboard { model, anchor };
        self.set_tool(Tool::Select);
        self.paste_preview = Some(PastePreview {
            clipboard,
            point: self.doc.camera.target,
        });
        self.say(ctx, "Click to place the component · Escape to cancel");
    }

    fn components_contents(&mut self, ui: &mut egui::Ui) {
        let mut definitions = std::collections::BTreeMap::new();
        for object in self.doc.model.objects.values() {
            if let ObjectKind::Component { definition } = object.kind {
                let entry =
                    definitions
                        .entry(definition)
                        .or_insert((object.id, object.name.clone(), 0));
                entry.2 += 1;
            }
        }
        if definitions.is_empty() {
            ui.label(RichText::new("No components yet").color(Palette::for_ui(ui).muted));
            ui.label("Select geometry and choose Make Component from the context menu.");
        } else {
            ui.add(
                egui::TextEdit::singleline(&mut self.component_filter)
                    .hint_text("Search components")
                    .desired_width(f32::INFINITY),
            );
            let query = self.component_filter.to_lowercase();
            let mut rows: Vec<_> = definitions
                .into_values()
                .filter(|(_, name, _)| name.to_lowercase().contains(&query))
                .collect();
            rows.sort_by(|a, b| {
                a.1.to_lowercase()
                    .cmp(&b.1.to_lowercase())
                    .then(a.0.cmp(&b.0))
            });
            if rows.is_empty() {
                ui.label(RichText::new("No matching components").color(Palette::for_ui(ui).muted));
            }
            for (id, name, count) in rows {
                ui.push_id(id, |ui| {
                    egui::Frame::new()
                        .fill(Palette::for_ui(ui).canvas)
                        .inner_margin(8)
                        .corner_radius(4)
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(RichText::new(name).strong());
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "{count} instance{}",
                                        if count == 1 { "" } else { "s" }
                                    ))
                                    .small()
                                    .color(Palette::for_ui(ui).muted),
                                );
                                if ui.small_button("Place instance").clicked() {
                                    self.begin_component_placement(id, ui.ctx());
                                }
                            });
                        });
                });
            }
        }
        if ui
            .add_enabled(
                !self.selected.is_empty() && !self.doc.model.selection_protected(&self.selected),
                egui::Button::new("Make Component…"),
            )
            .clicked()
        {
            self.context_action(ui.ctx(), ContextAction::Component);
        }
    }

    fn tags_contents(&mut self, ui: &mut egui::Ui) {
        let mut before = None;
        let mut label = "Edit tags";
        ui.horizontal(|ui| {
            let input = ui.add(
                egui::TextEdit::singleline(&mut self.new_tag_name)
                    .hint_text("New tag name")
                    .desired_width((ui.available_width() - 40.).max(80.)),
            );
            let enter = input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if ui
                .add_enabled(!self.new_tag_name.trim().is_empty(), egui::Button::new("+"))
                .on_hover_text("Add tag")
                .clicked()
                || enter
            {
                before.get_or_insert_with(|| self.doc.model.clone());
                match self.doc.model.add_tag(&self.new_tag_name) {
                    Ok(_) => {
                        self.new_tag_name.clear();
                        label = "Add tag";
                    }
                    Err(error) => self.say(ui.ctx(), error),
                }
            }
        });
        ui.label(
            RichText::new("Visibility · Color · Name")
                .small()
                .color(Palette::for_ui(ui).muted),
        );
        ui.label("Untagged");
        let tags: Vec<_> = self.doc.model.tags.values().cloned().collect();
        for mut tag in tags {
            ui.push_id(tag.id, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .checkbox(&mut tag.visible, "")
                        .on_hover_text("Show or hide this tag and its contents")
                        .changed()
                    {
                        before.get_or_insert_with(|| self.doc.model.clone());
                        self.doc.model.tags.get_mut(&tag.id).unwrap().visible = tag.visible;
                    }
                    if ui.color_edit_button_srgb(&mut tag.color).changed() {
                        before.get_or_insert_with(|| self.doc.model.clone());
                        self.doc.model.tags.get_mut(&tag.id).unwrap().color = tag.color;
                    }
                    let name = ui
                        .add_sized(
                            egui::vec2((ui.available_width() - 80.).max(30.), 24.),
                            egui::Label::new(&tag.name).truncate(),
                        )
                        .on_hover_text("Double-click to rename");
                    if name.double_clicked() {
                        self.tag_rename = Some((tag.id, tag.name.clone()));
                    }
                    if ui
                        .small_button("Edit")
                        .on_hover_text("Rename tag")
                        .clicked()
                    {
                        self.tag_rename = Some((tag.id, tag.name.clone()));
                    }
                    if ui
                        .small_button("×")
                        .on_hover_text("Delete tag; its entities become Untagged")
                        .clicked()
                    {
                        before.get_or_insert_with(|| self.doc.model.clone());
                        self.doc.model.remove_tag(tag.id);
                        label = "Delete tag";
                    }
                });
            });
        }
        if let Some((id, mut name)) = self.tag_rename.take() {
            if self.doc.model.tags.contains_key(&id) {
                let mut keep = true;
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut name).desired_width(115.));
                    if ui.small_button("Save").clicked() {
                        before.get_or_insert_with(|| self.doc.model.clone());
                        match self.doc.model.rename_tag(id, &name) {
                            Ok(()) => {
                                keep = false;
                                label = "Rename tag";
                            }
                            Err(error) => self.say(ui.ctx(), error),
                        }
                    }
                    if ui.small_button("Cancel").clicked() {
                        keep = false;
                    }
                });
                if keep {
                    self.tag_rename = Some((id, name));
                }
            }
        }
        ui.separator();
        if self.selected.is_empty() {
            ui.label(
                RichText::new("Select entities to assign a tag.")
                    .small()
                    .color(Palette::for_ui(ui).muted),
            );
        } else {
            let first = self
                .selected
                .first()
                .and_then(|id| self.doc.model.entity_tags.get(id))
                .copied();
            let mixed = self
                .selected
                .iter()
                .any(|id| self.doc.model.entity_tags.get(id).copied() != first);
            let current = if mixed {
                "Mixed tags"
            } else {
                first
                    .and_then(|id| self.doc.model.tags.get(&id))
                    .map(|t| t.name.as_str())
                    .unwrap_or("Untagged")
            };
            let mut assign = None;
            ui.add_enabled_ui(!self.doc.model.selection_protected(&self.selected), |ui| {
                egui::ComboBox::from_id_salt("selection_tag")
                    .selected_text(current)
                    .width(ui.available_width() - 10.)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(!mixed && first.is_none(), "Untagged")
                            .clicked()
                        {
                            assign = Some(None);
                        }
                        for tag in self.doc.model.tags.values() {
                            if ui
                                .selectable_label(!mixed && first == Some(tag.id), &tag.name)
                                .clicked()
                            {
                                assign = Some(Some(tag.id));
                            }
                        }
                    });
            });
            if let Some(tag) = assign {
                before.get_or_insert_with(|| self.doc.model.clone());
                if let Err(error) = self.doc.model.assign_tag(&self.selected, tag) {
                    self.say(ui.ctx(), error);
                }
                label = "Assign tag";
            }
        }
        if let Some(before) = before.filter(|before| *before != self.doc.model) {
            // Tag visibility is global, while assignments inside definitions are shared.
            if label == "Assign tag" {
                self.finish_context_state_edit(label, before, ui.ctx());
            } else {
                self.finish_edit(label, before, ui.ctx());
            }
            if self
                .doc
                .model
                .edit_context
                .is_some_and(|id| self.doc.model.is_hidden(id))
            {
                self.navigate_context(None, ui.ctx());
            }
            self.hover = None;
            self.snap = None;
        }
    }

    fn display_contents(&mut self, ui: &mut egui::Ui) {
        let before = self.doc.display.clone();
        let d = &mut self.doc.display;
        ui.label(RichText::new("Visibility").strong());
        for (value, label) in [
            (&mut d.axes, "Axes"),
            (&mut d.guides, "Guides"),
            (&mut d.grid, "Grid"),
            (&mut d.dimensions, "Dimensions"),
            (&mut d.hidden_geometry, "Hidden geometry"),
            (&mut d.hidden_objects, "Hidden objects"),
        ] {
            ui.checkbox(value, label);
        }
        ui.separator();
        ui.label(RichText::new("Face style").strong());
        face_style_picker(ui, &mut d.face_style);
        ui.checkbox(&mut d.x_ray, "X-Ray");
        ui.checkbox(&mut d.color_by_tag, "Color by tag");
        inspector_section(ui, "Edge style", "edge_style", false, |ui| {
            for (value, label) in [
                (&mut d.edges, "Edges"),
                (&mut d.back_edges, "Back edges"),
                (&mut d.profiles, "Profiles"),
                (&mut d.depth_cue, "Depth cue"),
                (&mut d.endpoints, "Endpoints"),
                (&mut d.extensions, "Extensions"),
            ] {
                ui.checkbox(value, label);
            }
            ui.add_enabled_ui(d.profiles, |ui| {
                // Keep the label above the slider so this nested section fits
                // even at the inspector's minimum width.
                let label = ui.label("Profile width");
                ui.add(egui::Slider::new(&mut d.profile_width, 1. ..=8.))
                    .labelled_by(label.id);
            });
        });
        inspector_section(ui, "Sections", "sections", false, |ui| {
            ui.checkbox(&mut d.section_planes, "Section planes");
            ui.checkbox(&mut d.section_cuts, "Section cuts");
            ui.checkbox(&mut d.section_fill, "Section fill");
            ui.horizontal(|ui| {
                ui.label("Height");
                ui.add(
                    egui::DragValue::new(&mut d.section_height)
                        .speed(0.05)
                        .range(-1e8..=1e8)
                        .suffix(" m"),
                );
            });
            ui.label(
                RichText::new("Horizontal section: keeps geometry below the plane.")
                    .small()
                    .color(Palette::for_ui(ui).muted),
            );
        });
        inspector_section(ui, "Shadows", "shadows", false, |ui| {
            ui.checkbox(&mut d.shadows, "Display shadows");
            ui.add_enabled_ui(d.shadows, |ui| {
                ui.add(
                    egui::Slider::new(&mut d.sun_angle, 0. ..=360.)
                        .text("Direction")
                        .suffix("°"),
                );
                ui.add(
                    egui::Slider::new(&mut d.sun_elevation, 5. ..=85.)
                        .text("Elevation")
                        .suffix("°"),
                );
                ui.add(egui::Slider::new(&mut d.shadow_darkness, 0. ..=1.).text("Darkness"));
            });
        });
        inspector_section(ui, "Fog", "fog", false, |ui| {
            ui.checkbox(&mut d.fog, "Display fog");
            ui.add_enabled_ui(d.fog, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Start");
                    ui.add(
                        egui::DragValue::new(&mut d.fog_start)
                            .speed(0.5)
                            .range(0. ..=1e8)
                            .suffix(" m"),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("End");
                    ui.add(
                        egui::DragValue::new(&mut d.fog_end)
                            .speed(0.5)
                            .range(0.1..=1e9)
                            .suffix(" m"),
                    );
                });
                d.fog_end = d.fog_end.max(d.fog_start + 0.1);
            });
            ui.horizontal(|ui| {
                crate::environment::background_control(ui, d);
                ui.label("Background / fog color");
            });
        });
        inspector_section(ui, "Environment", "display_environment", false, |ui| {
            crate::environment::controls(ui, d);
        });
        if ui.button("Reset display").clicked() {
            *d = DisplaySettings::default();
        }
        if before != self.doc.display {
            self.changed(ui.ctx());
        }
    }
}
