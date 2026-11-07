use super::*;

#[test]
fn suggest_feature_opens_new_github_issue_directly() {
    let mut h = Harness::new();
    let output = h.frame(vec![], egui::Modifiers::NONE);
    let output = h.click(text_position(&output, "Suggest a feature"));
    assert!(output.platform_output.commands.iter().any(|command| {
        matches!(command, egui::OutputCommand::OpenUrl(url)
            if url.new_tab && url.url.starts_with("https://github.com/Ketchup3D/app/issues/new?body=")
                && url.url.contains("%0A---%0A")
                && url.url.contains("System%20Information")
                && url.url.contains("KetchUp3D%20"))
    }));
    assert!(!h.app.project.modified);
    assert!(h.app.dirty_since.is_none());
}

#[test]
fn report_bug_opens_new_github_issue_with_diagnostics() {
    let mut h = Harness::new();
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(text_position(&output, "Help"));
    let output = h.frame(vec![], egui::Modifiers::NONE);
    let output = h.click(menu_text_position(&output, "Report a bug"));
    assert!(output.platform_output.commands.iter().any(|command| {
        matches!(command, egui::OutputCommand::OpenUrl(url)
            if url.new_tab
                && url.url.starts_with("https://github.com/Ketchup3D/app/issues/new?body=")
                && url.url.contains("%0A---%0A")
                && url.url.contains("System%20Information")
                && url.url.contains("KetchUp3D%20"))
    }));
    assert!(!h.app.project.modified);
    assert!(h.app.dirty_since.is_none());
    let next = h.frame(vec![], egui::Modifiers::NONE);
    assert!(
        !next
            .platform_output
            .commands
            .iter()
            .any(|command| { matches!(command, egui::OutputCommand::OpenUrl(_)) })
    );
}
