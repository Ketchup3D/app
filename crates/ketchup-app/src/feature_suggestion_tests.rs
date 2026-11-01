use super::*;

#[test]
fn suggest_feature_opens_new_github_issue_directly() {
    let mut h = Harness::new();
    let output = h.frame(vec![], egui::Modifiers::NONE);
    let output = h.click(text_position(&output, "Suggest a feature"));
    assert!(output.platform_output.commands.iter().any(|command| {
        matches!(command, egui::OutputCommand::OpenUrl(url)
            if url.new_tab && url.url == "https://github.com/Ketchup3D/app/issues/new")
    }));
    assert!(!h.app.project.modified);
    assert!(h.app.dirty_since.is_none());
}
