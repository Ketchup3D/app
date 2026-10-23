//! Move-copy previews and editable linear arrays share the original model snapshot.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CopyArray {
    count: usize,
    divide: bool,
}
impl Default for CopyArray {
    fn default() -> Self {
        Self {
            count: 1,
            divide: false,
        }
    }
}
impl CopyArray {
    pub(super) fn parse(input: &str) -> Result<Self, &'static str> {
        let input = input.trim();
        let (number, divide) = if let Some(n) = input.strip_prefix('/') {
            (n, true)
        } else if let Some(n) = input.strip_suffix('/') {
            (n, true)
        } else if let Some(n) = input.strip_prefix(['x', 'X', '*']) {
            (n, false)
        } else if let Some(n) = input.strip_suffix(['x', 'X', '*']) {
            (n, false)
        } else {
            return Err("Enter an array count, such as /3 or 3x");
        };
        let number = number.trim();
        if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
            return Err("Enter a whole array count from 1 to 1000");
        }
        let count = number
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=1000).contains(n))
            .ok_or("Enter a whole array count from 1 to 1000")?;
        Ok(Self { count, divide })
    }
}

pub(super) struct MoveCopy {
    pub base: Model,
    pub source: Model,
    pub delta: DVec3,
}

impl KetchupApp {
    pub(super) fn move_copy_key(ctx: &egui::Context) -> &'static str {
        if ctx.os() == egui::os::OperatingSystem::Mac {
            "Option"
        } else {
            "Ctrl"
        }
    }

    pub(super) fn move_modifier(
        &mut self,
        ctx: &egui::Context,
        modifiers: egui::Modifiers,
        shortcuts: bool,
    ) {
        let mac = ctx.os() == egui::os::OperatingSystem::Mac;
        let down = if mac { modifiers.alt } else { modifiers.ctrl };
        let pressed = down && !self.move_modifier_down;
        self.move_modifier_down = down;
        if !pressed
            || shortcuts
            || !ctx.input(|i| i.focused)
            || self.tool != Tool::Move
            || self.paste_preview.is_some()
            || (mac && (modifiers.ctrl || modifiers.mac_cmd))
        {
            return;
        }
        self.move_copy = !self.move_copy;
        self.last_move_copy = None;
        if let Some(edit) = &mut self.modeling {
            edit.copy = self.move_copy;
            edit.preview = None;
            edit.preview_delta = None;
        }
        if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
            self.update_modeling(pos);
        }
        ctx.request_repaint();
    }

    pub(super) fn move_delta(edit: &Modeling, value: f64) -> Result<DVec3, &'static str> {
        if !value.is_finite() {
            return Err("Enter a finite displacement");
        }
        if value.abs() < 1e-7 {
            Ok(DVec3::ZERO)
        } else {
            Ok(edit
                .direction
                .try_normalize()
                .ok_or("Move the pointer to choose a direction first")?
                * value)
        }
    }

    pub(super) fn move_copies(
        base: &Model,
        source: &Model,
        delta: DVec3,
        array: CopyArray,
    ) -> Result<(Model, BTreeSet<u64>), &'static str> {
        let mut model = base.clone();
        let mut selected = BTreeSet::new();
        for index in 1..=array.count {
            let factor = if array.divide {
                index as f64 / array.count as f64
            } else {
                index as f64
            };
            selected = model.paste_entities(source, delta * factor);
            if selected.is_empty() {
                return Err(
                    "Cannot copy this geometry here; check the distance and component nesting",
                );
            }
        }
        Ok((model, selected))
    }

    pub(super) fn commit_move_array(&mut self, ctx: &egui::Context) {
        let Some(copy) = &self.last_move_copy else {
            return;
        };
        let result = CopyArray::parse(&self.length_input)
            .and_then(|array| Self::move_copies(&copy.base, &copy.source, copy.delta, array));
        match result {
            Ok((model, selected)) => {
                self.doc.model = model;
                self.selected = selected;
                self.length_input.clear();
                // The initial copy already recorded the original model. Replacing its
                // result keeps the copy and every array revision in a single undo step.
                self.changed(ctx);
                #[cfg(target_arch = "wasm32")]
                self.persist();
            }
            Err(message) => self.say(ctx, message),
        }
    }
}
