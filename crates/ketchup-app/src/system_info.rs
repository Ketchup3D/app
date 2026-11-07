//! Support diagnostics captured on demand and copied only by user action.
use crate::{modal::Modal, natural_text::TextUi as _};
use eframe::egui;
use std::fmt::Write as _;

#[derive(Clone, Default)]
struct Report {
    text: String,
    copied: bool,
}

fn id() -> egui::Id {
    egui::Id::unique("system_information_report")
}

pub(crate) fn open(ctx: &egui::Context) {
    ctx.data_mut(|data| data.insert_temp(id(), Report::default()));
}

// Transient action, consumed in the same UI frame once eframe::Frame is available.
fn issue_request_id() -> egui::Id {
    egui::Id::unique("github_issue_request")
}

pub(crate) fn request_issue(ctx: &egui::Context) {
    ctx.data_mut(|data| data.insert_temp(issue_request_id(), true));
}

fn issue_url(report: &str) -> String {
    // Choose a fence longer than any backtick sequence supplied by a GPU driver or browser.
    let fence = "`".repeat(
        report
            .split(|c| c != '`')
            .map(str::len)
            .max()
            .unwrap_or(0)
            .max(2)
            + 1,
    );
    let body = format!(
        "Describe your feature suggestion or bug here.\n\n---\n\n## System Information\n\n{fence}text\n{report}\n{fence}\n"
    );
    let mut url = String::from("https://github.com/Ketchup3D/app/issues/new?body=");
    // Percent-encode UTF-8 bytes as a query value, including newlines and reserved characters.
    for byte in body.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            url.push(char::from(byte));
        } else {
            let _ = write!(url, "%{byte:02X}");
        }
    }
    url
}

fn collect(ctx: &egui::Context, frame: &eframe::Frame) -> String {
    let mut text = format!(
        "KetchUp3D {}\nBuild: {}\n",
        env!("CARGO_PKG_VERSION"),
        if cfg!(debug_assertions) {
            "Debug"
        } else {
            "Release"
        }
    );
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = writeln!(
            text,
            "Runtime: Desktop\nOS: {}\nArchitecture: {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        #[cfg(target_os = "linux")]
        if let Ok(release) = std::fs::read_to_string("/etc/os-release")
            && let Some(name) = release
                .lines()
                .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        {
            let _ = writeln!(text, "OS version: {}", name.trim_matches('"'));
        }
        if let Ok(threads) = std::thread::available_parallelism() {
            let _ = writeln!(text, "Available logical CPUs: {threads}");
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = writeln!(
            text,
            "Runtime: Browser (WebAssembly)\nBrowser / OS: {}",
            frame.info().web_info.user_agent
        );
    }
    let _ = writeln!(text, "Platform: {:?}", ctx.os());
    let bounds = ctx.viewport_rect();
    let scale = ctx.pixels_per_point();
    let _ = writeln!(
        text,
        "Window: {:.0} × {:.0} pixels\nUI scale: {scale:.2} pixels/point\nUI zoom: {:.0}%\nTheme: {}",
        bounds.width() * scale,
        bounds.height() * scale,
        ctx.zoom_factor() * 100.,
        if ctx.global_style().visuals.dark_mode {
            "Dark"
        } else {
            "Light"
        }
    );
    if let Some(render) = frame.wgpu_render_state() {
        let adapter = render.adapter.get_info();
        let _ = writeln!(
            text,
            "Renderer: wgpu\nGraphics backend: {:?}\nGPU: {}\nGPU type: {:?}\nGPU vendor/device: {:04x}/{:04x}\nDriver: {}\nDriver details: {}\nSurface format: {:?}",
            adapter.backend,
            adapter.name,
            adapter.device_type,
            adapter.vendor,
            adapter.device,
            adapter.driver,
            adapter.driver_info,
            render.target_format
        );
    } else {
        text.push_str("Graphics adapter information: unavailable\n");
    }
    text
}

pub(crate) fn show(ctx: &egui::Context, frame: &eframe::Frame) {
    if ctx
        .data_mut(|data| data.remove_temp::<bool>(issue_request_id()))
        .unwrap_or(false)
    {
        ctx.open_url(egui::OpenUrl::new_tab(issue_url(&collect(ctx, frame))));
    }
    let Some(mut report) = ctx.data_mut(|data| data.get_temp::<Report>(id())) else {
        return;
    };
    if report.text.is_empty() {
        report.text = collect(ctx, frame);
    }
    let response = Modal::new("system_information", 560.).show(ctx, |modal| {
        modal.header(|ui| {
            ui.text_heading("System information");
        });
        modal.body(|ui| {
            ui.text_label("Include these details when reporting a bug.");
            ui.add_space(8.);
            egui::ScrollArea::vertical()
                .max_height(400.)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(&report.text).monospace())
                            .selectable(true)
                            .wrap(),
                    );
                });
        });
        modal.footer(|ui| {
            // Footer flows right to left: Close is the rightmost action.
            if ui.button("Close").clicked() {
                ui.close();
            }
            if ui
                .button(if report.copied {
                    "Copied"
                } else {
                    "Copy to clipboard"
                })
                .clicked()
            {
                ctx.copy_text(report.text.clone());
                report.copied = true;
            }
        });
    });
    ctx.data_mut(|data| {
        if response.should_close() {
            data.remove::<Report>(id());
        } else {
            data.insert_temp(id(), report);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_body_round_trips_utf8_and_reserved_characters() {
        let report = "GPU: Érable & #1? 100% + / =\nDriver: ```custom```\n";
        let url = issue_url(report);
        let encoded = url
            .strip_prefix("https://github.com/Ketchup3D/app/issues/new?body=")
            .unwrap();
        assert!(!encoded.contains(['&', '#', '+', '?', '\n', ' ']));
        let mut decoded = Vec::new();
        let mut bytes = encoded.bytes();
        while let Some(byte) = bytes.next() {
            if byte == b'%' {
                let high = char::from(bytes.next().unwrap()).to_digit(16).unwrap();
                let low = char::from(bytes.next().unwrap()).to_digit(16).unwrap();
                decoded.push((high * 16 + low) as u8);
            } else {
                decoded.push(byte);
            }
        }
        let body = String::from_utf8(decoded).unwrap();
        assert!(body.starts_with("Describe your feature suggestion or bug here.\n\n---\n\n"));
        assert!(body.ends_with(&format!(
            "## System Information\n\n````text\n{report}\n````\n"
        )));
    }
}
