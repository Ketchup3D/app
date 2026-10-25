# Sidebar preferences

Every new option or expandable section in the right sidebar must remember its state across reloads and app restarts.

- Use the shared preferences in `crates/ketchup-app/src/sidebar.rs`. Put display options in the serialized `DisplaySettings`; put other user preferences in `InspectorPreferences` and include them in capture/restore.
- Use `inspector_section` for sidebar sections. For custom `CollapsingHeader` disclosures, use `restored_header_open` and `remember_section`; for `CollapsingState`, use `restore_section` and `remember_section`. Supply a stable, unique key. Keys must not depend on display titles, section order, or docked/floating layout.
- Keep preference loading compatible with older saved data using defaults for added fields. Preserve existing section order and append new sections.
- Keep model-specific state, such as tag visibility and entity assignments, in the document; do not apply those values to unrelated projects.
- Save changes through `persist_inspector_changes` so preferences survive immediate reloads, without requiring a model edit or waiting for framework autosave.
- Add regression coverage for newly persisted controls, defaults, and restoration where appropriate.
