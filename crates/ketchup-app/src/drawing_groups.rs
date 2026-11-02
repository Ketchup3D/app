//! Root drawings own their topology; drawing inside an object edits that object.
use super::*;

impl KetchupApp {
    pub(super) fn prompt_drawn_group_name(&mut self, ctx: &egui::Context, group: Option<u64>) {
        if let Some(id) = group
            && let Some(object) = self.doc.model.objects.get(&id)
        {
            self.object_rename = Some((id, object.name.clone()));
            Self::request_name_input_focus(ctx, egui::Id::unique("rename_object_name"));
        }
    }

    pub(super) fn drawn_geometry(
        &self,
        points: &[DVec3],
        continuing_group: Option<u64>,
    ) -> Result<(Model, Option<u64>, bool), &'static str> {
        let context = self.doc.model.edit_context;
        let continuing_group = continuing_group.filter(|id| {
            context.is_none()
                && self.doc.model.objects.contains_key(id)
                && self.doc.model.parent(*id).is_none()
                && !self.doc.model.is_locked(*id)
                && !self.doc.model.is_hidden(*id)
        });
        let new_group = context.is_none() && continuing_group.is_none();
        let mut drawing = if new_group {
            Model::default()
        } else {
            self.doc.model.clone()
        };
        drawing.edit_context = context.or(continuing_group);
        let faces_before = drawing.faces.len();
        for pair in points.windows(2) {
            match drawing.add_edge(pair[0], pair[1]) {
                Ok(_) => {}
                Err("That edge already exists") if self.tool != Tool::Line => {}
                Err(error) => return Err(error),
            }
        }
        let closed = drawing.faces.len() > faces_before;
        if new_group {
            let mut model = self.doc.model.clone();
            let entities = model.paste_entities(&drawing, DVec3::ZERO);
            let group = model
                .make_object(&entities, false, self.tool.label())
                .ok_or("The drawing could not be grouped")?;
            Ok((model, Some(group), closed))
        } else {
            drawing.edit_context = context;
            Ok((drawing, continuing_group, closed))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(tool: Tool) -> KetchupApp {
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.set_tool(tool);
        app
    }
    #[test]
    fn completed_automatic_groups_prompt_for_a_name_but_existing_contexts_do_not() {
        let ctx = egui::Context::default();
        for nested in [false, true] {
            let mut app = app(Tool::Rectangle);
            if nested {
                let edge = app.doc.model.add_edge(-DVec3::X, -DVec3::Y).unwrap();
                let group = app.doc.model.make_object(&BTreeSet::from([edge]), false, "Existing").unwrap();
                assert!(app.doc.model.enter_object(group));
            }
            let points = [DVec3::ZERO, DVec3::X, DVec3::X + DVec3::Y, DVec3::Y, DVec3::ZERO];
            let (model, group, _) = app.drawn_geometry(&points, None).unwrap();
            app.doc.model = model;
            app.prompt_drawn_group_name(&ctx, group);
            assert_eq!(app.object_rename.as_ref().map(|(id, _)| *id), group);
            assert_eq!(app.object_rename.is_some(), !nested);
        }
    }

    #[test]
    fn every_root_drawing_tool_creates_one_group_without_merging_existing_geometry() {
        let points = [
            DVec3::ZERO,
            DVec3::X,
            DVec3::X + DVec3::Y,
            DVec3::Y,
            DVec3::ZERO,
        ];
        for tool in [
            Tool::Line,
            Tool::Rectangle,
            Tool::Circle,
            Tool::Polygon,
            Tool::Freehand,
        ] {
            let mut app = app(tool);
            let existing = app.doc.model.add_edge(-DVec3::X, DVec3::X * 2.).unwrap();
            let before = app.doc.model.clone();
            let (model, group, closed) = app.drawn_geometry(&points, None).unwrap();
            assert!(closed);
            let group = group.unwrap();
            assert_eq!(model.objects.len(), 1);
            assert!(
                model
                    .faces
                    .keys()
                    .all(|id| model.parent(*id) == Some(group))
            );
            assert_eq!(model.edges[&existing], before.edges[&existing]);
            assert_eq!(
                model.vertices[&model.edges[&existing].a],
                before.vertices[&before.edges[&existing].a]
            );
            assert!(model.parent(existing).is_none());
            assert!(model.edit_context.is_none());
            model.validate().unwrap();
        }
    }
    #[test]
    fn connected_line_segments_share_one_group_and_close_a_face() {
        let mut app = app(Tool::Line);
        let points = [
            DVec3::ZERO,
            DVec3::X,
            DVec3::X + DVec3::Y,
            DVec3::Y,
            DVec3::ZERO,
        ];
        let mut group = None;
        for (i, pair) in points.windows(2).enumerate() {
            let (model, id, closed) = app.drawn_geometry(pair, group).unwrap();
            if let Some(group) = group {
                assert_eq!(id, Some(group));
            }
            assert_eq!(closed, i == 3);
            group = id;
            app.doc.model = model;
            assert_eq!(app.doc.model.objects.len(), 1);
            assert!(app.doc.model.edit_context.is_none());
        }
        assert_eq!(app.doc.model.faces.len(), 1);
        app.doc.model.validate().unwrap();
    }
    #[test]
    fn drawing_in_an_existing_group_does_not_create_a_nested_group() {
        let mut app = app(Tool::Rectangle);
        let edge = app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
        let group = app
            .doc
            .model
            .make_object(&BTreeSet::from([edge]), false, "Existing")
            .unwrap();
        assert!(app.doc.model.enter_object(group));
        let (model, automatic_group, _) = app
            .drawn_geometry(&[DVec3::Y, DVec3::Y + DVec3::X], None)
            .unwrap();
        assert!(automatic_group.is_none());
        assert_eq!(model.objects.len(), 1);
        assert_eq!(model.edit_context, Some(group));
        assert!(
            model
                .edges
                .keys()
                .all(|id| model.parent(*id) == Some(group))
        );
        model.validate().unwrap();
    }
    #[test]
    fn grouped_drawing_undo_redo_restores_the_whole_shape() {
        let mut app = app(Tool::Rectangle);
        let before = app.doc.model.clone();
        let points = [
            DVec3::ZERO,
            DVec3::X,
            DVec3::X + DVec3::Y,
            DVec3::Y,
            DVec3::ZERO,
        ];
        let (model, group, _) = app.drawn_geometry(&points, None).unwrap();
        app.doc.model = model;
        app.selected = BTreeSet::from([group.unwrap()]);
        app.finish_edit("Draw rectangle", before.clone(), &egui::Context::default());
        let after = app.doc.model.clone();
        assert!(app.doc.history.undo(&mut app.doc.model));
        assert_eq!(app.doc.model, before);
        assert!(app.doc.history.redo(&mut app.doc.model));
        assert_eq!(app.doc.model, after);
    }
}
