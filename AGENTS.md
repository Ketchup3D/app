# Persistent options and settings

Every future user-facing option, setting, tool choice, and expandable section throughout the app must remember its last state across reloads and app restarts. This applies to toolbars, menus, dialogs, and panels as well as the right sidebar. Persistence is part of implementing a control, not a separate follow-up task.

- Use the shared preferences in `crates/ketchup-app/src/sidebar.rs`. Put display options in the serialized `DisplaySettings`; put other user preferences in `InspectorPreferences` and include them in capture/restore.
- Use `inspector_section` for sidebar sections. For custom `CollapsingHeader` disclosures, use `restored_header_open` and `remember_section`; for `CollapsingState`, use `restore_section` and `remember_section`. Supply a stable, unique key. Keys must not depend on display titles, section order, or docked/floating layout.
- Keep preference loading compatible with older saved data using defaults for added fields. Preserve existing section order and append new sections.
- Keep model-specific state, such as tag visibility and entity assignments, in the document; do not apply those values to unrelated projects.
- Save changes through `persist_inspector_changes` so preferences survive immediate reloads, without requiring a model edit or waiting for framework autosave.
- Add regression coverage for newly persisted controls, defaults, and restoration where appropriate, including changes saved without a model edit.

# Release workflow

Whenever the user asks to push a new version tag:

1. Prepare user-facing release notes for the new version in `assets/release-notes.json` first, using the changes since the previous release. Keep releases newest first, preserve older entries, and include the version, release date, title, and categorized changes. This file powers Help → Release notes and the GitHub Release description.
2. Update the workspace version and lockfile as needed. Validate the matching tag and notes with `python3 scripts/desktop_release.py version --tag vX.Y.Z`, and preview the published text with `python3 scripts/desktop_release.py notes --tag vX.Y.Z`.
3. Commit and push the release notes and version changes, together with the changes intended for that release.
4. Only after the changes have been pushed successfully, create and push the matching version tag. Never push the tag before its release notes and release changes.

# Compilation checks

Only at the end of each change, immediately before reporting completion, check the currently running KetchUp process, if one exists, and its associated compiler or development-server output for new compilation errors. Perform this check once per fix, not during intermediate edits. Use the existing process and logs; do not start a new build just for this check. If the output is unavailable, report that the check could not be completed.

# Modal actions

In every modal, place the least impactful action (usually Cancel or Close) on the right. Use the shared `Modal` shell in `crates/ketchup-app/src/modal.rs`; its footer renders right to left, so declare actions from least to most impactful. For unsaved changes, the visual order is Discard changes, Save, Cancel. Apply this convention to all future modals.
