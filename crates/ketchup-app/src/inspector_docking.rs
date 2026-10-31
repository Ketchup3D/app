//! Persistent, nested split areas for inspector sections. A center drop joins
//! the hovered stack; a drop within 10 points inside an edge splits that area.
use super::*;

const SNAP_RANGE: f32 = 10.;
const DIVIDER: f32 = 6.;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) enum DockLayout {
    Stack {
        id: u64,
        sections: Vec<InspectorSection>,
    },
    Split {
        id: u64,
        horizontal: bool,
        ratio: f32,
        first: Box<Self>,
        second: Box<Self>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Top,
    Right,
    Bottom,
    Left,
    Center,
}

#[derive(Clone)]
struct Pane {
    id: u64,
    rect: Rect,
    sections: Vec<InspectorSection>,
}

#[derive(Clone, Copy)]
struct DropTarget {
    pane: u64,
    side: Side,
    index: usize,
    preview: Rect,
}

// The inner edge band splits the area; the remaining interior joins the stack.
// At corners the closest edge wins. Drops outside the area cancel docking.
fn drop_target(rect: Rect, pointer: Pos2) -> Option<(Side, Rect)> {
    if !rect.contains(pointer) {
        return None;
    }
    let distances = [
        (pointer.y - rect.top(), Side::Top),
        (rect.right() - pointer.x, Side::Right),
        (rect.bottom() - pointer.y, Side::Bottom),
        (pointer.x - rect.left(), Side::Left),
    ];
    let Some((_, side)) = distances
        .into_iter()
        .filter(|(distance, _)| *distance <= SNAP_RANGE)
        .min_by(|a, b| a.0.total_cmp(&b.0))
    else {
        return Some((Side::Center, rect));
    };
    let mut preview = rect;
    match side {
        Side::Top => preview.max.y = rect.center().y - DIVIDER * 0.5,
        Side::Right => preview.min.x = rect.center().x + DIVIDER * 0.5,
        Side::Bottom => preview.min.y = rect.center().y + DIVIDER * 0.5,
        Side::Left => preview.max.x = rect.center().x - DIVIDER * 0.5,
        Side::Center => {}
    }
    Some((side, preview))
}

fn find_drop_target(panes: &[Pane], pointer: Pos2) -> Option<DropTarget> {
    panes.iter().find_map(|pane| {
        drop_target(pane.rect, pointer).map(|(side, preview)| DropTarget {
            pane: pane.id,
            side,
            index: 0,
            preview,
        })
    })
}

impl DockLayout {
    fn stack(order: &[InspectorSection]) -> Self {
        Self::Stack {
            id: 0,
            sections: order.to_vec(),
        }
    }

    fn stack_mut(&mut self, pane: u64) -> Option<&mut Self> {
        match self {
            Self::Stack { id, .. } if *id == pane => Some(self),
            Self::Split { first, second, .. } => {
                first.stack_mut(pane).or_else(|| second.stack_mut(pane))
            }
            _ => None,
        }
    }

    fn pane_for(&self, section: InspectorSection) -> Option<u64> {
        match self {
            Self::Stack { id, sections } if sections.contains(&section) => Some(*id),
            Self::Split { first, second, .. } => {
                first.pane_for(section).or_else(|| second.pane_for(section))
            }
            _ => None,
        }
    }

    fn sections(&self, order: &mut Vec<InspectorSection>) {
        match self {
            Self::Stack { sections, .. } => order.extend(sections),
            Self::Split { first, second, .. } => {
                first.sections(order);
                second.sections(order);
            }
        }
    }

    fn remove(&mut self, section: InspectorSection) {
        match self {
            Self::Stack { sections, .. } => sections.retain(|s| *s != section),
            Self::Split { first, second, .. } => {
                first.remove(section);
                second.remove(section);
                if matches!(first.as_ref(), Self::Stack { sections, .. } if sections.is_empty()) {
                    *self = *second.clone();
                } else if matches!(second.as_ref(), Self::Stack { sections, .. } if sections.is_empty())
                {
                    *self = *first.clone();
                }
            }
        }
    }

    fn next_id(&self) -> u64 {
        match self {
            Self::Stack { id, .. } => id + 1,
            Self::Split {
                id, first, second, ..
            } => (id + 1).max(first.next_id()).max(second.next_id()),
        }
    }

    // Reject ambiguous IDs/duplicates, empty branches, bad ratios and excessive
    // nesting. Missing sections are valid older data and get appended below.
    fn valid(
        &self,
        depth: usize,
        ids: &mut BTreeSet<u64>,
        seen: &mut Vec<InspectorSection>,
    ) -> bool {
        if depth >= InspectorSection::COUNT {
            return false;
        }
        match self {
            Self::Stack { id, sections } => {
                *id < u64::MAX - 2
                    && ids.insert(*id)
                    && !sections.is_empty()
                    && sections.iter().all(|s| {
                        if seen.contains(s) {
                            false
                        } else {
                            seen.push(*s);
                            true
                        }
                    })
            }
            Self::Split {
                id,
                ratio,
                first,
                second,
                ..
            } => {
                *id < u64::MAX - 2
                    && ids.insert(*id)
                    && ratio.is_finite()
                    && (0.1..=0.9).contains(ratio)
                    && first.valid(depth + 1, ids, seen)
                    && second.valid(depth + 1, ids, seen)
            }
        }
    }

    fn append(&mut self, missing: &[InspectorSection]) {
        match self {
            Self::Stack { sections, .. } => sections.extend_from_slice(missing),
            Self::Split { second, .. } => second.append(missing),
        }
    }

    fn dock(&mut self, section: InspectorSection, pane: u64, side: Side, index: usize) {
        let next_id = self.next_id();
        let Some(Self::Stack { sections, .. }) = self.stack_mut(pane) else {
            return;
        };
        if sections.as_slice() == [section] {
            return;
        }
        // Keep the destination's ID alive when the empty source is collapsed.
        self.remove(section);
        let Some(target) = self.stack_mut(pane) else {
            return;
        };
        if side == Side::Center {
            if let Self::Stack { sections, .. } = target {
                sections.insert(index.min(sections.len()), section);
            }
        } else {
            let fresh = Self::Stack {
                id: next_id,
                sections: vec![section],
            };
            let original = target.clone();
            let (first, second) = if matches!(side, Side::Top | Side::Left) {
                (fresh, original)
            } else {
                (original, fresh)
            };
            *target = Self::Split {
                id: next_id + 1,
                horizontal: matches!(side, Side::Left | Side::Right),
                ratio: 0.5,
                first: Box::new(first),
                second: Box::new(second),
            };
        }
    }

    fn panes(&mut self, ui: &mut egui::Ui, rect: Rect, panes: &mut Vec<Pane>, dragging: bool) {
        match self {
            Self::Stack { id, sections } => panes.push(Pane {
                id: *id,
                rect,
                sections: sections.clone(),
            }),
            Self::Split {
                id,
                horizontal,
                ratio,
                first,
                second,
            } => {
                let span = if *horizontal {
                    rect.width()
                } else {
                    rect.height()
                };
                let divider_width = DIVIDER.min(span.max(0.));
                let available = (span - divider_width).max(0.);
                let offset = available * *ratio;
                let (a, divider, b) = if *horizontal {
                    let x = rect.left() + offset;
                    (
                        Rect::from_min_max(rect.min, egui::pos2(x, rect.bottom())),
                        Rect::from_min_max(
                            egui::pos2(x, rect.top()),
                            egui::pos2(x + divider_width, rect.bottom()),
                        ),
                        Rect::from_min_max(egui::pos2(x + divider_width, rect.top()), rect.max),
                    )
                } else {
                    let y = rect.top() + offset;
                    (
                        Rect::from_min_max(rect.min, egui::pos2(rect.right(), y)),
                        Rect::from_min_max(
                            egui::pos2(rect.left(), y),
                            egui::pos2(rect.right(), y + divider_width),
                        ),
                        Rect::from_min_max(egui::pos2(rect.left(), y + divider_width), rect.max),
                    )
                };
                let response = ui.interact(
                    divider,
                    egui::Id::unique(("inspector_divider", *id)),
                    Sense::drag(),
                );
                if !dragging && response.dragged() && available > 1. {
                    let delta = ui.input(|i| {
                        if *horizontal {
                            i.pointer.delta().x
                        } else {
                            i.pointer.delta().y
                        }
                    });
                    *ratio = (*ratio + delta / available).clamp(0.1, 0.9);
                }
                response.on_hover_cursor(if *horizontal {
                    egui::CursorIcon::ResizeHorizontal
                } else {
                    egui::CursorIcon::ResizeVertical
                });
                ui.painter()
                    .rect_filled(divider, 2, Palette::for_ui(ui).border);
                first.panes(ui, a, panes, dragging);
                second.panes(ui, b, panes, dragging);
            }
        }
    }
}

#[derive(Default)]
pub(in super::super) struct InspectorLayout {
    pub(super) dock: Option<DockLayout>,
    heights: [f32; InspectorSection::COUNT],
    drag: Option<InspectorDrag>,
}

#[derive(Clone, Copy)]
struct InspectorDrag {
    section: InspectorSection,
    grab_offset: Vec2,
    width: f32,
    insertion: Option<(u64, usize)>,
}

impl InspectorLayout {
    pub(super) fn restore(
        saved: Option<&DockLayout>,
        order: &[InspectorSection; InspectorSection::COUNT],
    ) -> Self {
        let mut seen = Vec::new();
        let mut dock = match saved.filter(|dock| dock.valid(0, &mut BTreeSet::new(), &mut seen)) {
            Some(saved) => saved.clone(),
            None => DockLayout::stack(order),
        };
        let mut present = Vec::new();
        dock.sections(&mut present);
        // The legacy order already appends newly introduced sections correctly.
        let missing: Vec<_> = order
            .iter()
            .copied()
            .filter(|s| !present.contains(s))
            .collect();
        dock.append(&missing);
        Self {
            dock: Some(dock),
            ..Default::default()
        }
    }

    pub(super) fn order(&self) -> [InspectorSection; InspectorSection::COUNT] {
        let mut order = Vec::new();
        if let Some(dock) = &self.dock {
            dock.sections(&mut order);
        }
        order.try_into().unwrap_or(InspectorSection::ALL)
    }

    fn show_layout_preview(&self, target: DropTarget) -> bool {
        self.drag.is_some_and(|drag| {
            target.side != Side::Center
                || self
                    .dock
                    .as_ref()
                    .and_then(|dock| dock.pane_for(drag.section))
                    != Some(target.pane)
        })
    }

    fn insertion_index(
        &self,
        sections: &[InspectorSection],
        section: InspectorSection,
        pointer_y: f32,
        spacing: f32,
    ) -> usize {
        let mut top = 0.;
        let mut index = 0;
        for &neighbor in sections {
            let height = self.heights[neighbor.index()];
            if neighbor != section {
                if pointer_y < top + (height - spacing).max(0.) * 0.5 {
                    break;
                }
                index += 1;
            }
            // Keep the dragged section's slot in the coordinate system, so a
            // neighbor must be crossed halfway before it moves into that slot.
            top += height;
        }
        index
    }
}

impl KetchupApp {
    pub(in super::super) fn inspector_contents(&mut self, ui: &mut egui::Ui) {
        let mut layout = std::mem::take(&mut self.inspector_layout);
        let ctx = ui.ctx().clone();
        if !ui.is_enabled() || !egui::DragAndDrop::has_payload_of_type::<InspectorSection>(&ctx) {
            layout.drag = None;
        }
        let rect = ui.available_rect_before_wrap();
        let mut panes = Vec::new();
        layout
            .dock
            .get_or_insert_with(|| DockLayout::stack(&self.inspector_order))
            .panes(ui, rect, &mut panes, layout.drag.is_some());
        let pointer = ctx.pointer_interact_pos();
        let mut target = pointer.and_then(|p| find_drop_target(&panes, p));
        let show_insertion_slot = !target.is_some_and(|target| target.side != Side::Center);
        if !target.is_some_and(|target| target.side == Side::Center)
            && let Some(drag) = &mut layout.drag
        {
            drag.insertion = None;
        }
        // Keep the original sidebar scroll key for preferences saved before splits.
        let single = panes.len() == 1;
        let dragging = layout.drag;
        for pane in panes {
            if pane.rect.width() <= 1. || pane.rect.height() <= 1. {
                continue;
            }
            let mut pane_ui = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("inspector_stack", pane.id))
                    .max_rect(pane.rect),
            );
            pane_ui.set_clip_rect(pane.rect.intersect(ui.clip_rect()));
            let key = if pane.id == 0 {
                "sidebar_right".to_owned()
            } else {
                format!("sidebar_right/{}", pane.id)
            };
            let margin = if single { 18 } else { 8 };
            let scroll = restored_scroll_area(
                &pane_ui,
                &key,
                egui::ScrollArea::vertical()
                    .id_salt(("inspector_stack_scroll", pane.id))
                    .content_margin(margin)
                    .auto_shrink([false, false]),
            )
            .show_viewport(&mut pane_ui, |ui, viewport| {
                let origin = ui.cursor().min;
                let mut sections = pane.sections.clone();
                if let (Some(drag), Some(target), Some(pointer)) = (dragging, &mut target, pointer)
                    && target.pane == pane.id
                    && target.side == Side::Center
                {
                    // Start from the visible preview, including its reserved
                    // slot, so a stationary pointer cannot keep moving sections.
                    if let Some((pane_id, index)) = drag.insertion
                        && pane_id == pane.id
                    {
                        sections.retain(|s| *s != drag.section);
                        sections.insert(index.min(sections.len()), drag.section);
                    }
                    target.index = layout.insertion_index(
                        &sections,
                        drag.section,
                        pointer.y - origin.y,
                        ui.spacing().item_spacing.y,
                    );
                    sections.retain(|s| *s != drag.section);
                    sections.insert(target.index, drag.section);
                    layout.drag.as_mut().unwrap().insertion = Some((pane.id, target.index));
                } else if let (Some(drag), Some(target)) = (dragging, target)
                    && target.side == Side::Center
                    && target.pane != pane.id
                {
                    sections.retain(|s| *s != drag.section);
                }
                let fill_height = (pane.sections.len() == 1 && sections.len() == 1)
                    .then_some((viewport.height() - 2. * margin as f32).max(0.));
                self.inspector_stack(
                    ui,
                    &sections,
                    &mut layout,
                    dragging,
                    fill_height,
                    show_insertion_slot,
                );
                if dragging.is_some()
                    && target.is_some_and(|t| t.pane == pane.id)
                    && let Some(pointer) = pointer
                    && ui.input(|i| i.pointer.primary_down())
                {
                    let clip = pane.rect;
                    let speed = ((clip.top() + 32. - pointer.y).max(0.)
                        - (pointer.y - (clip.bottom() - 32.)).max(0.))
                        * 12.;
                    if speed != 0. {
                        let dt = ui.input(|i| i.stable_dt.min(0.05));
                        ui.scroll_with_delta_animation(
                            egui::vec2(0., speed * dt),
                            egui::style::ScrollAnimation::none(),
                        );
                        ctx.request_repaint();
                    }
                }
            });
            remember_scroll(&ctx, &key, scroll.state.clamped_offset());
        }
        ui.allocate_rect(rect, Sense::hover());
        if let Some(drag) = layout.drag {
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
            if let Some(target) = target
                && layout.show_layout_preview(target)
            {
                let colors = Palette::for_ui(ui);
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::unique("inspector_drop_preview"),
                ));
                painter.rect(
                    target.preview.shrink(2.),
                    6,
                    colors.accent.linear_multiply(0.15),
                    Stroke::new(2., colors.accent),
                    egui::StrokeKind::Inside,
                );
            }
            if ui.input(|i| i.pointer.button_released(egui::PointerButton::Primary)) {
                if let Some(target) = target {
                    layout.dock.as_mut().unwrap().dock(
                        drag.section,
                        target.pane,
                        target.side,
                        target.index,
                    );
                    self.inspector_order = layout.order();
                }
                layout.drag = None;
                egui::DragAndDrop::clear_payload(&ctx);
                ctx.request_repaint();
            }
        }
        self.inspector_layout = layout;
    }

    fn inspector_stack(
        &mut self,
        ui: &mut egui::Ui,
        sections: &[InspectorSection],
        layout: &mut InspectorLayout,
        dragging: Option<InspectorDrag>,
        fill_height: Option<f32>,
        show_insertion_slot: bool,
    ) {
        let origin = ui.cursor().min;
        let width = ui.available_width();
        let pointer = ui.ctx().pointer_interact_pos();
        let spacing = ui.spacing().item_spacing.y;
        let colors = Palette::for_ui(ui);
        let mut top = 0.;
        for &section in sections {
            let index = section.index();
            let id = egui::Id::unique(("inspector_section", section.id()));
            let target = origin + egui::vec2(0., top);
            let following = dragging.filter(|d| d.section == section);
            let floating = following.is_some();
            let pos = following
                .and_then(|drag| pointer.map(|p| p - drag.grab_offset))
                .unwrap_or(target);
            if following.is_some() && show_insertion_slot {
                let slot = Rect::from_min_size(
                    target,
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
            let panel_width = following.map_or(width, |d| d.width);
            let mut builder = egui::UiBuilder::new()
                .scope_id(id)
                .max_rect(Rect::from_min_max(
                    pos,
                    egui::pos2(
                        pos.x + panel_width,
                        fill_height.map_or(f32::INFINITY, |height| pos.y + height),
                    ),
                ))
                .layout(egui::Layout::top_down(egui::Align::Min));
            if floating {
                builder = builder.layer_id(egui::LayerId::new(egui::Order::Tooltip, id));
            }
            let mut panel_ui = ui.new_child(builder);
            if floating {
                panel_ui.set_clip_rect(ui.ctx().viewport_rect());
            }
            let panel = inspector_section_sized(
                &mut panel_ui,
                section.title(),
                section.id(),
                true,
                fill_height.map(|height| egui::vec2(panel_width, height)),
                (!floating).then_some(ui.clip_rect()),
                |ui| match section {
                    InspectorSection::Outliner => self.outliner_contents(ui),
                    InspectorSection::EntityInfo => self.entity_info_contents(ui),
                    InspectorSection::Components => self.components_contents(ui),
                    InspectorSection::Tags => self.tags_contents(ui),
                    InspectorSection::Display => self.display_contents(ui),
                    InspectorSection::View => self.view_contents(ui),
                    InspectorSection::Units => self.units_contents(ui),
                    InspectorSection::TimeCostEstimates => self.time_estimates_contents(ui),
                    InspectorSection::Materials => self.materials_contents(ui),
                },
            );
            let mut header = panel.inner;
            let settings_rect = (section == InspectorSection::Materials).then(|| {
                let rect = Rect::from_center_size(
                    egui::pos2(header.rect.right() - 22., header.rect.center().y),
                    egui::vec2(32., 32.),
                );
                // Keep the gear separate from the disclosure and docking drag target.
                header.interact_rect.max.x = header.interact_rect.right().min(rect.left());
                rect
            });
            let header = header.interact(egui::Sense::click_and_drag())
                .on_hover_cursor(egui::CursorIcon::Grab)
                .on_hover_text("Drop within 10px inside an area's edge to split; cross a section's vertical midpoint to reorder, or drop in another area's center to join");
            if let Some(rect) = settings_rect {
                // Use the header's layer so the button follows sticky and floating headers.
                let mut action_ui = panel_ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt("material_settings")
                        .layer_id(header.layer_id)
                        .max_rect(rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                action_ui.set_clip_rect(panel_ui.clip_rect().intersect(header.rect));
                if Self::icon_button(
                    &mut action_ui,
                    "settings",
                    "Manage available stock materials, sizes, and prices",
                    true,
                    colors.muted,
                ) {
                    self.general_settings.section = settings::Section::StockPricing;
                    self.settings_open = true;
                }
            }
            layout.heights[index] = panel_ui.min_rect().height() + spacing;
            // Keep the slot's space stable even when a split preview hides its
            // outline, or when reordering within the same area.
            top += layout.heights[index];
            if layout.drag.is_none() && header.drag_started_by(egui::PointerButton::Primary) {
                let press = ui
                    .input(|i| i.pointer.press_origin())
                    .unwrap_or(header.rect.center());
                layout.drag = Some(InspectorDrag {
                    section,
                    // Anchor to the visible header, even when its section's
                    // original top has scrolled outside the area.
                    grab_offset: press - header.rect.min
                        + Vec2::splat(header.rect.left() - panel.response.rect.left()),
                    width,
                    insertion: None,
                });
                egui::DragAndDrop::set_payload(ui.ctx(), section);
                ui.ctx().request_repaint();
            }
        }
        ui.allocate_space(egui::vec2(width, (top - spacing).max(0.)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reordering_waits_for_the_hovered_sections_midpoint_in_both_directions() {
        use InspectorSection::{Components, Outliner, Tags};

        let mut layout = InspectorLayout::default();
        // Full panels are 200, 100, and 200 points tall, plus a 10-point gap.
        layout.heights[Outliner.index()] = 210.;
        layout.heights[Components.index()] = 110.;
        layout.heights[Tags.index()] = 210.;
        let sections = [Outliner, Components, Tags];

        // The dragged slot stays in place while moving through Tags' top half.
        for pointer_y in [225., 320., 419.9] {
            assert_eq!(
                layout.insertion_index(&sections, Components, pointer_y, 10.),
                1
            );
        }
        assert_eq!(layout.insertion_index(&sections, Components, 420., 10.), 2);
        assert_eq!(layout.insertion_index(&sections, Components, 100., 10.), 1);
        assert_eq!(layout.insertion_index(&sections, Components, 99.9, 10.), 0);

        // After swapping down, hold the new position until the pointer crosses
        // the now-hovered section's midpoint in the opposite direction.
        let preview = [Outliner, Tags, Components];
        for pointer_y in [420., 400., 310.] {
            assert_eq!(
                layout.insertion_index(&preview, Components, pointer_y, 10.),
                2
            );
        }
        assert_eq!(layout.insertion_index(&preview, Components, 309.9, 10.), 1);
        let preview = [Components, Outliner, Tags];
        assert_eq!(layout.insertion_index(&preview, Components, 99.9, 10.), 0);
        assert_eq!(layout.insertion_index(&preview, Components, 209.9, 10.), 0);
        assert_eq!(layout.insertion_index(&preview, Components, 210., 10.), 1);
    }

    #[test]
    fn joining_another_stack_uses_each_full_sections_midpoint() {
        use InspectorSection::{Components, Outliner, Tags};

        let mut layout = InspectorLayout::default();
        layout.heights[Outliner.index()] = 210.;
        layout.heights[Tags.index()] = 110.;
        let sections = [Outliner, Tags];
        for (pointer_y, index) in [(99.9, 0), (100., 1), (259.9, 1), (260., 2)] {
            assert_eq!(
                layout.insertion_index(&sections, Components, pointer_y, 10.),
                index
            );
        }
    }

    #[test]
    fn edge_zones_extend_ten_points_inward_and_never_outward() {
        let rect = Rect::from_min_size(egui::pos2(100., 200.), egui::vec2(300., 400.));
        for (side, border, direction) in [
            (Side::Top, egui::pos2(250., 200.), egui::vec2(0., -1.)),
            (Side::Right, egui::pos2(400., 400.), egui::vec2(1., 0.)),
            (Side::Bottom, egui::pos2(250., 600.), egui::vec2(0., 1.)),
            (Side::Left, egui::pos2(100., 400.), egui::vec2(-1., 0.)),
        ] {
            for distance in [0., 0.1, 5., 10.] {
                let (actual, preview) = drop_target(rect, border - direction * distance).unwrap();
                assert_eq!(actual, side);
                assert!(rect.contains_rect(preview));
                match side {
                    Side::Left | Side::Right => assert_eq!(preview.width(), 147.),
                    Side::Top | Side::Bottom => assert_eq!(preview.height(), 197.),
                    Side::Center => unreachable!(),
                }
            }
            assert_eq!(
                drop_target(rect, border - direction * 10.1),
                Some((Side::Center, rect))
            );
            for distance in [0.1, 5., 10., 30.] {
                assert!(drop_target(rect, border + direction * distance).is_none());
            }
        }
        assert_eq!(drop_target(rect, rect.center()), Some((Side::Center, rect)));
        assert_eq!(
            drop_target(rect, rect.min + egui::vec2(4., 8.)).unwrap().0,
            Side::Left
        );
        assert_eq!(
            drop_target(rect, rect.min + egui::vec2(8., 4.)).unwrap().0,
            Side::Top
        );
        assert!(drop_target(rect, rect.min - egui::vec2(1., 4.)).is_none());
        let small = Rect::from_min_size(Pos2::ZERO, egui::vec2(30., 30.));
        assert_eq!(
            drop_target(small, small.center()),
            Some((Side::Center, small))
        );
    }

    #[test]
    fn inward_bands_target_the_containing_pane_and_exclude_dividers() {
        let panes = [
            Pane {
                id: 0,
                rect: Rect::from_min_max(egui::pos2(100., 100.), egui::pos2(300., 400.)),
                sections: vec![InspectorSection::Outliner],
            },
            Pane {
                id: 1,
                rect: Rect::from_min_max(egui::pos2(306., 100.), egui::pos2(506., 400.)),
                sections: vec![InspectorSection::Components],
            },
        ];
        let target = find_drop_target(&panes, egui::pos2(105., 250.)).unwrap();
        assert_eq!((target.pane, target.side), (0, Side::Left));
        assert!(find_drop_target(&panes, egui::pos2(99., 250.)).is_none());
        assert!(find_drop_target(&panes, egui::pos2(303., 250.)).is_none());
        let target = find_drop_target(&panes, egui::pos2(295., 250.)).unwrap();
        assert_eq!((target.pane, target.side), (0, Side::Right));
        let target = find_drop_target(&panes, egui::pos2(311., 250.)).unwrap();
        assert_eq!((target.pane, target.side), (1, Side::Left));
        let target = find_drop_target(&panes, egui::pos2(330., 250.)).unwrap();
        assert_eq!((target.pane, target.side), (1, Side::Center));
        let target = find_drop_target(&panes, egui::pos2(406., 250.)).unwrap();
        assert_eq!((target.pane, target.side), (1, Side::Center));
    }

    fn assert_complete(layout: &DockLayout) {
        let mut order = Vec::new();
        layout.sections(&mut order);
        assert_eq!(order.len(), InspectorSection::COUNT);
        for section in InspectorSection::ALL {
            assert!(order.contains(&section));
        }
        assert!(layout.valid(0, &mut BTreeSet::new(), &mut Vec::new()));
    }

    #[test]
    fn all_four_edges_split_and_center_rejoins_collapsing_the_empty_source() {
        for side in [Side::Top, Side::Right, Side::Bottom, Side::Left] {
            let mut layout = DockLayout::stack(&InspectorSection::ALL);
            layout.dock(InspectorSection::Components, 0, side, 0);
            assert_complete(&layout);
            let DockLayout::Split {
                horizontal,
                ratio,
                first,
                second,
                ..
            } = &layout
            else {
                panic!("expected split")
            };
            assert_eq!(*horizontal, matches!(side, Side::Left | Side::Right));
            assert_eq!(*ratio, 0.5);
            let fresh = if matches!(side, Side::Left | Side::Top) {
                first
            } else {
                second
            };
            assert!(
                matches!(fresh.as_ref(), DockLayout::Stack { sections, .. } if sections.as_slice() == [InspectorSection::Components])
            );
            layout.dock(InspectorSection::Components, 0, Side::Center, 2);
            assert_complete(&layout);
            let DockLayout::Stack { id, sections } = layout else {
                panic!("empty source should collapse")
            };
            assert_eq!(id, 0);
            assert_eq!(sections[2], InspectorSection::Components);
        }
    }

    #[test]
    fn nested_splits_accept_moves_from_other_areas_and_preserve_the_rest() {
        let mut layout = DockLayout::stack(&InspectorSection::ALL);
        layout.dock(InspectorSection::Components, 0, Side::Right, 0);
        layout.dock(InspectorSection::Tags, 0, Side::Bottom, 0);
        assert_complete(&layout);
        let mut right = layout.stack_mut(1).unwrap().clone();
        layout.dock(InspectorSection::Components, 3, Side::Left, 0);
        assert_complete(&layout);
        assert!(layout.stack_mut(1).is_none());
        layout.dock(InspectorSection::Tags, 5, Side::Center, 0);
        assert_complete(&layout);
        assert!(
            matches!(layout.stack_mut(5), Some(DockLayout::Stack { sections, .. }) if sections.as_slice() == [InspectorSection::Tags, InspectorSection::Components])
        );
        // Splitting the only section out of its own area, or missing a target,
        // must leave the layout untouched.
        let before = right.clone();
        right.dock(InspectorSection::Components, 1, Side::Bottom, 0);
        right.dock(InspectorSection::Components, 999, Side::Center, 0);
        assert_eq!(right, before);
    }

    #[test]
    fn legacy_orders_and_new_sections_restore_without_reordering_existing_areas() {
        let old: InspectorPreferences = serde_json::from_str("{}").unwrap();
        assert!(old.docking.is_none());
        let mut order = InspectorSection::ALL;
        order.rotate_right(2);
        assert_eq!(InspectorLayout::restore(None, &order).order(), order);
        let saved = DockLayout::Split {
            id: 9,
            horizontal: true,
            ratio: 0.3,
            first: Box::new(DockLayout::Stack {
                id: 4,
                sections: vec![InspectorSection::Tags],
            }),
            second: Box::new(DockLayout::Stack {
                id: 7,
                sections: vec![InspectorSection::Outliner],
            }),
        };
        let restored = InspectorLayout::restore(Some(&saved), &InspectorSection::ALL);
        let mut expected = vec![InspectorSection::Tags, InspectorSection::Outliner];
        expected.extend(
            InspectorSection::ALL
                .into_iter()
                .filter(|s| ![InspectorSection::Tags, InspectorSection::Outliner].contains(s)),
        );
        assert_eq!(restored.order().as_slice(), expected.as_slice());
        let DockLayout::Split { ratio, first, .. } = restored.dock.unwrap() else {
            panic!()
        };
        assert_eq!(ratio, 0.3);
        assert!(
            matches!(*first, DockLayout::Stack { id: 4, sections } if sections.as_slice() == [InspectorSection::Tags])
        );
    }

    #[test]
    fn invalid_saved_layouts_fall_back_to_the_legacy_order() {
        let stack = || DockLayout::stack(&InspectorSection::ALL);
        for saved in [
            DockLayout::Stack {
                id: 0,
                sections: vec![],
            },
            DockLayout::Stack {
                id: 0,
                sections: vec![InspectorSection::Tags; 2],
            },
            DockLayout::Stack {
                id: u64::MAX,
                sections: vec![InspectorSection::Tags],
            },
            DockLayout::Split {
                id: 1,
                horizontal: true,
                ratio: f32::NAN,
                first: Box::new(stack()),
                second: Box::new(stack()),
            },
            DockLayout::Split {
                id: 1,
                horizontal: true,
                ratio: 0.,
                first: Box::new(stack()),
                second: Box::new(stack()),
            },
            DockLayout::Split {
                id: 1,
                horizontal: true,
                ratio: 0.5,
                first: Box::new(stack()),
                second: Box::new(stack()),
            },
        ] {
            let restored = InspectorLayout::restore(Some(&saved), &InspectorSection::ALL);
            assert_eq!(restored.dock, Some(stack()));
        }
    }

    fn ui_frame(
        ctx: &egui::Context,
        app: &mut KetchupApp,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, Rect) {
        let mut rect = Rect::NOTHING;
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(600., 1000.))),
                events,
                ..Default::default()
            },
            |ui| {
                rect = ui.available_rect_before_wrap();
                app.inspector_contents(ui);
            },
        );
        (output, rect)
    }

    fn header_position(output: &egui::FullOutput, label: &str) -> Pos2 {
        fn locate(shape: &egui::Shape, label: &str) -> Option<Pos2> {
            match shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| locate(s, label)),
                _ => None,
            }
        }
        output
            .shapes
            .iter()
            .find_map(|s| locate(&s.shape, label))
            .unwrap_or_else(|| panic!("Missing {label}"))
    }

    fn drag(ctx: &egui::Context, app: &mut KetchupApp, from: Pos2, to: Pos2) {
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        ui_frame(
            ctx,
            app,
            vec![egui::Event::PointerMoved(from), button(from, true)],
        );
        ui_frame(ctx, app, vec![egui::Event::PointerMoved(to)]);
        ui_frame(ctx, app, vec![]);
        ui_frame(ctx, app, vec![button(to, false)]);
    }

    #[test]
    fn mouse_drags_split_on_every_edge_then_rejoin_the_hovered_center() {
        for side in [Side::Top, Side::Right, Side::Bottom, Side::Left] {
            let ctx = egui::Context::default();
            ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
            let mut app = KetchupApp::with_document(Document::default(), None, true);
            // Keep all headers visible and their click targets stable.
            for section in InspectorSection::ALL {
                set_section_open(&ctx, section.id(), false);
            }
            ui_frame(&ctx, &mut app, vec![]);
            let (output, rect) = ui_frame(&ctx, &mut app, vec![]);
            let from = header_position(&output, "Components");
            let mut to = rect.center();
            match side {
                Side::Top => to.y = rect.top() + 5.,
                Side::Right => to.x = rect.right() - 5.,
                Side::Bottom => to.y = rect.bottom() - 5.,
                Side::Left => to.x = rect.left() + 5.,
                Side::Center => unreachable!(),
            }
            drag(&ctx, &mut app, from, to);
            let dock = app.inspector_layout.dock.as_ref().unwrap();
            assert_complete(dock);
            let DockLayout::Split {
                horizontal,
                first,
                second,
                ..
            } = dock
            else {
                panic!("{side:?} did not split")
            };
            assert_eq!(*horizontal, matches!(side, Side::Left | Side::Right));
            let fresh = if matches!(side, Side::Left | Side::Top) {
                first
            } else {
                second
            };
            assert!(
                matches!(fresh.as_ref(), DockLayout::Stack { sections, .. } if sections.as_slice() == [InspectorSection::Components])
            );
            let (output, _) = ui_frame(&ctx, &mut app, vec![]);
            let from = header_position(&output, "Components");
            // Join the center of the original area, on the opposite side.
            let offset = match side {
                Side::Top => egui::vec2(0., rect.height() * 0.25),
                Side::Right => egui::vec2(-rect.width() * 0.25, 0.),
                Side::Bottom => egui::vec2(0., -rect.height() * 0.25),
                Side::Left => egui::vec2(rect.width() * 0.25, 0.),
                Side::Center => unreachable!(),
            };
            drag(&ctx, &mut app, from, rect.center() + offset);
            assert!(matches!(
                app.inspector_layout.dock,
                Some(DockLayout::Stack { id: 0, .. })
            ));
            assert_complete(app.inspector_layout.dock.as_ref().unwrap());
            assert!(!app.project.modified);
        }
    }

    #[test]
    fn escape_and_outside_releases_cancel_without_changing_the_layout() {
        let ctx = egui::Context::default();
        ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        for section in InspectorSection::ALL {
            set_section_open(&ctx, section.id(), false);
        }
        ui_frame(&ctx, &mut app, vec![]);
        let (output, rect) = ui_frame(&ctx, &mut app, vec![]);
        let before = app.inspector_layout.dock.clone();
        let from = header_position(&output, "Components");
        let to = egui::pos2(rect.right() - 5., rect.center().y);
        ui_frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(from),
                egui::Event::PointerButton {
                    pos: from,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        ui_frame(&ctx, &mut app, vec![egui::Event::PointerMoved(to)]);
        ui_frame(
            &ctx,
            &mut app,
            vec![egui::Event::Key {
                key: Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(app.inspector_layout.drag.is_none());
        ui_frame(
            &ctx,
            &mut app,
            vec![egui::Event::PointerButton {
                pos: to,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(app.inspector_layout.dock, before);
        let (output, _) = ui_frame(&ctx, &mut app, vec![]);
        drag(
            &ctx,
            &mut app,
            header_position(&output, "Components"),
            rect.min - egui::vec2(1., 1.),
        );
        assert_eq!(app.inspector_layout.dock, before);
        assert!(app.inspector_layout.drag.is_none());
    }

    #[test]
    fn mouse_dragging_a_divider_resizes_the_areas_without_changing_their_sections() {
        let ctx = egui::Context::default();
        ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        for section in InspectorSection::ALL {
            set_section_open(&ctx, section.id(), false);
        }
        app.inspector_layout = InspectorLayout::restore(None, &app.inspector_order);
        app.inspector_layout.dock.as_mut().unwrap().dock(
            InspectorSection::Components,
            0,
            Side::Right,
            0,
        );
        let order = app.inspector_layout.order();
        ui_frame(&ctx, &mut app, vec![]);
        let (_, rect) = ui_frame(&ctx, &mut app, vec![]);
        drag(
            &ctx,
            &mut app,
            rect.center(),
            rect.center() + egui::vec2(50., 0.),
        );
        let Some(DockLayout::Split { ratio, .. }) = &app.inspector_layout.dock else {
            panic!()
        };
        assert!(*ratio > 0.55 && *ratio < 0.65, "{ratio}");
        assert_eq!(app.inspector_layout.order(), order);
        assert!(!app.project.modified);
    }

    #[derive(Default)]
    struct Storage {
        values: std::collections::BTreeMap<String, String>,
        flushes: usize,
    }
    impl eframe::Storage for Storage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.values.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.values.insert(key.into(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.values.remove(key);
        }
        fn flush(&mut self) {
            self.flushes += 1;
        }
    }

    #[test]
    fn docking_ratios_and_disclosures_save_immediately_without_model_edits() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let model = app.doc.model.clone();
        app.inspector_layout = InspectorLayout::restore(None, &app.inspector_order);
        let mut storage = Storage::default();
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        let dock = app.inspector_layout.dock.as_mut().unwrap();
        dock.dock(InspectorSection::Components, 0, Side::Right, 0);
        dock.dock(InspectorSection::Tags, 0, Side::Bottom, 0);
        if let DockLayout::Split { ratio, .. } = dock {
            *ratio = 0.35;
        }
        app.inspector_order = app.inspector_layout.order();
        set_section_open(&ctx, "components", false);
        let flushes = storage.flushes;
        // Layout changes must save even when the caller's legacy order flag is false.
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushes > flushes);
        let restored_ctx = egui::Context::default();
        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        restored.restore_inspector_preferences(&restored_ctx, Some(&storage));
        assert_eq!(restored.inspector_layout.dock, app.inspector_layout.dock);
        assert_eq!(restored.inspector_order, app.inspector_order);
        // A divider-only change saves independently of section order changes.
        if let Some(DockLayout::Split { ratio, .. }) = &mut restored.inspector_layout.dock {
            *ratio = 0.65;
        }
        let flushes = storage.flushes;
        restored.persist_inspector_changes(&restored_ctx, Some(&mut storage), false);
        assert!(storage.flushes > flushes);
        let mut refreshed = KetchupApp::with_document(Document::default(), None, true);
        refreshed.restore_inspector_preferences(&restored_ctx, Some(&storage));
        assert_eq!(
            refreshed.inspector_layout.dock,
            restored.inspector_layout.dock
        );
        let _ = restored_ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.push_id("different_docked_or_floating_scope", |ui| {
                assert_eq!(restored_header_open(ui, "components", true), Some(false));
            });
        });
        assert_eq!(app.doc.model, model);
        assert!(!app.project.modified);
        assert!(app.dirty_since.is_none());
    }
}
