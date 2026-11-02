use crate::{
    modal::{Modal, primary_button},
    natural_text::TextUi as _,
};
use eframe::egui::{self, TextureHandle};
use std::sync::mpsc::{self, Receiver};

#[derive(Clone, Debug, serde::Deserialize)]
pub(crate) struct User {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub email: String,
}
#[derive(serde::Deserialize)]
struct Session {
    authenticated: bool,
    user: Option<User>,
    #[serde(default)]
    csrf: Option<String>,
}
#[derive(serde::Deserialize)]
#[cfg(not(target_arch = "wasm32"))]
struct Login {
    token: String,
    authorize_path: String,
}
enum Request {
    Session,
    #[cfg(not(target_arch = "wasm32"))]
    Login,
    Picture,
    Logout,
}

pub(crate) struct Account {
    pub(crate) server: String,
    pub(crate) server_reachable: bool,
    pub open: bool,
    pub menu_open: bool,
    confirm_logout: bool,
    pub user: Option<User>,
    pub texture: Option<TextureHandle>,
    error: Option<String>,
    pending: Option<(Request, Receiver<Result<Vec<u8>, String>>)>,
    token: Option<String>,
    csrf: Option<String>,
    waiting: bool,
    next_poll: f64,
    login_deadline: f64,
}
impl Default for Account {
    fn default() -> Self {
        let server = crate::server_config::configured_server();
        let error = server.as_ref().err().cloned();
        Self {
            server: server.unwrap_or_default(),
            server_reachable: false,
            open: false,
            menu_open: false,
            confirm_logout: false,
            user: None,
            texture: None,
            error,
            pending: None,
            token: None,
            csrf: None,
            waiting: false,
            next_poll: 0.,
            login_deadline: 0.,
        }
    }
}
impl Account {
    pub fn has_server(&self) -> bool {
        !self.server.is_empty()
    }
    pub(crate) fn sharing_available(&self) -> bool {
        self.has_server() && self.server_reachable
    }
    pub fn start(&mut self, ctx: &egui::Context) {
        if self.server.is_empty() {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.token =
                credential_path(&self.server).and_then(|path| std::fs::read_to_string(path).ok());
        }
        self.refresh(ctx);
    }
    fn request(
        &mut self,
        ctx: &egui::Context,
        kind: Request,
        method: &str,
        url: String,
        token: Option<String>,
    ) {
        self.error = None;
        let csrf = if matches!(&kind, Request::Logout) {
            self.csrf.clone()
        } else {
            None
        };
        self.pending = Some((kind, request(ctx, method, url, token, csrf)));
    }
    pub(crate) fn refresh(&mut self, ctx: &egui::Context) {
        if self.server.is_empty() {
            return;
        }
        if !valid_server(&self.server) {
            self.error = Some(
                "Invalid account server configuration: use HTTPS, or HTTP on localhost.".into(),
            );
            return;
        }
        self.request(
            ctx,
            Request::Session,
            "GET",
            format!("{}/api/session", self.server.trim_end_matches('/')),
            self.token.clone(),
        );
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        let result = self
            .pending
            .as_ref()
            .and_then(|(_, receiver)| receiver.try_recv().ok());
        if let Some(result) = result {
            let (kind, _) = self.pending.take().unwrap();
            let session_request = matches!(&kind, Request::Session);
            let result = result.and_then(|bytes| match kind {
                Request::Session => {
                    let session: Session = serde_json::from_slice(&bytes)
                        .map_err(|_| "Invalid server session response".to_owned())?;
                    if session.authenticated && session.user.is_none() {
                        return Err("Invalid server profile".into());
                    }
                    self.server_reachable = true;
                    self.csrf = session.csrf;
                    self.user = session.user.filter(|_| session.authenticated);
                    self.texture = None;
                    if self.user.is_some() {
                        self.waiting = false;
                        self.open = false;
                        #[cfg(not(target_arch = "wasm32"))]
                        if let Some(token) = &self.token {
                            save_credential(&self.server, token)?;
                        }
                        self.request(
                            ctx,
                            Request::Picture,
                            "GET",
                            format!("{}/api/avatar", self.server.trim_end_matches('/')),
                            self.token.clone(),
                        );
                    }
                    Ok(())
                }
                #[cfg(not(target_arch = "wasm32"))]
                Request::Login => {
                    let login: Login = serde_json::from_slice(&bytes)
                        .map_err(|_| "Invalid server login response".to_owned())?;
                    if !login.authorize_path.starts_with("/client/connect?id=") {
                        return Err("Invalid login address".into());
                    }
                    self.token = Some(login.token);
                    self.waiting = true;
                    self.login_deadline = ctx.input(|i| i.time) + 600.;
                    ctx.open_url(egui::OpenUrl::new_tab(format!(
                        "{}{}",
                        self.server.trim_end_matches('/'),
                        login.authorize_path
                    )));
                    Ok(())
                }
                Request::Logout => {
                    let session: Session = serde_json::from_slice(&bytes)
                        .map_err(|_| "Invalid logout response".to_owned())?;
                    if session.authenticated {
                        return Err("The server did not sign out.".into());
                    }
                    self.clear_session();
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some(path) = credential_path(&self.server) {
                        match std::fs::remove_file(path) {
                            Ok(()) => {}
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                            Err(_) => {
                                return Err(
                                    "Signed out, but could not remove the saved credential.".into(),
                                );
                            }
                        }
                    }
                    Ok(())
                }
                Request::Picture => {
                    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes));
                    reader = reader
                        .with_guessed_format()
                        .map_err(|_| "Cannot read account picture".to_owned())?;
                    let mut limits = image::Limits::default();
                    limits.max_image_width = Some(4096);
                    limits.max_image_height = Some(4096);
                    reader.limits(limits);
                    let image = reader
                        .decode()
                        .map_err(|_| "Cannot load account picture".to_owned())?
                        .thumbnail(96, 96)
                        .to_rgba8();
                    self.texture = Some(ctx.load_texture(
                        "account-avatar",
                        egui::ColorImage::from_rgba_unmultiplied(
                            [image.width() as usize, image.height() as usize],
                            image.as_raw(),
                        ),
                        Default::default(),
                    ));
                    Ok(())
                }
            });
            if let Err(error) = result {
                self.error = Some(error);
                self.waiting = false;
                if session_request {
                    self.server_reachable = false;
                    self.user = None;
                    self.texture = None;
                }
            }
            self.next_poll = ctx.input(|i| i.time) + 2.;
        }
        if self.waiting && ctx.input(|i| i.time) >= self.login_deadline {
            self.waiting = false;
            self.error = Some("Login timed out. Continue with Google to try again.".into());
        }
        if (self.waiting || !self.server_reachable) && self.has_server()
            && self.pending.is_none() && ctx.input(|i| i.time) >= self.next_poll {
            self.next_poll = ctx.input(|i| i.time) + 10.;
            self.refresh(ctx);
        }
        if self.waiting || (!self.server_reachable && self.has_server()) {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
    }
    fn clear_session(&mut self) {
        self.user = None;
        self.texture = None;
        self.token = None;
        self.csrf = None;
        self.waiting = false;
        self.open = false;
        self.menu_open = false;
        self.confirm_logout = false;
        self.error = None;
    }
    pub fn avatar_menu(&mut self, avatar: &egui::Response) {
        if !self.sharing_available() {
            self.menu_open = false;
            return;
        }
        if avatar.clicked() {
            self.menu_open = !self.menu_open;
        }
        let mut open = self.menu_open;
        let mut logout = false;
        egui::Popup::menu(avatar)
            .style(crate::app::KetchupApp::menu_style)
            .id(egui::Id::unique("account_menu"))
            .open_bool(&mut open)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                if let Some(user) = &self.user {
                    let name = egui::WidgetText::from(egui::RichText::new(&user.name).strong())
                        .into_galley(
                            ui,
                            Some(egui::TextWrapMode::Wrap),
                            180.,
                            egui::TextStyle::Body,
                        );
                    let email = egui::WidgetText::from(egui::RichText::new(&user.email).weak())
                        .into_galley(
                            ui,
                            Some(egui::TextWrapMode::Wrap),
                            180.,
                            egui::TextStyle::Body,
                        );
                    let text_height = name.size().y + ui.spacing().item_spacing.y + email.size().y;
                    let header_height = text_height.max(48.);
                    ui.horizontal(|ui| {
                        ui.set_min_height(header_height);
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(48., 48.), egui::Sense::hover());
                        if let Some(texture) = &self.texture {
                            egui::Image::new((texture.id(), rect.size()))
                                .corner_radius(24)
                                .paint_at(ui, rect);
                        } else {
                            ui.painter().circle_filled(
                                rect.center(),
                                24.,
                                ui.visuals().widgets.inactive.bg_fill,
                            );
                            let initial = user
                                .name
                                .chars()
                                .next()
                                .unwrap_or('?')
                                .to_uppercase()
                                .to_string();
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                initial,
                                egui::FontId::proportional(22.),
                                ui.visuals().text_color(),
                            );
                        }
                        ui.vertical(|ui| {
                            ui.set_width(180.);
                            ui.add_space((header_height - text_height) / 2.);
                            ui.add(crate::natural_text::Label::new(name));
                            ui.add(crate::natural_text::Label::new(email));
                        });
                    });
                    ui.add_space(6.);
                    ui.separator();
                    ui.add_space(6.);
                }
                let signing_out = matches!(&self.pending, Some((Request::Logout, _)));
                if ui
                    .add_enabled(
                        !signing_out,
                        egui::Button::new(if signing_out {
                            "Logging out…"
                        } else {
                            "Log out"
                        }),
                    )
                    .clicked()
                {
                    logout = true;
                }
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
            });
        self.menu_open = open;
        if logout {
            self.confirm_logout = true;
            self.menu_open = false;
        }
    }
    fn show_logout_confirmation(&mut self, ctx: &egui::Context) {
        if !self.sharing_available() || !self.confirm_logout {
            return;
        }
        if self.user.is_none() {
            self.confirm_logout = false;
            return;
        }
        let mut logout = false;
        let response = Modal::new("account_logout_confirmation", 400.).show(ctx, |modal| {
            modal.header(|ui| {
                ui.text_heading("Log out");
            });
            modal.body(|ui| {
                ui.text_label("Are you sure you want to log out?");
            });
            modal.footer(|ui| {
                if ui.button("Cancel").clicked() {
                    ui.close();
                }
                if primary_button(
                    ui,
                    "Log out",
                    !matches!(&self.pending, Some((Request::Logout, _))),
                )
                .clicked()
                {
                    logout = true;
                    ui.close();
                }
            });
        });
        if logout || response.should_close() {
            self.confirm_logout = false;
        }
        if logout {
            // Show progress and any request error in the account menu, allowing a retry.
            self.menu_open = true;
            self.request(
                ctx,
                Request::Logout,
                "POST",
                format!("{}/api/logout", self.server.trim_end_matches('/')),
                self.token.clone(),
            );
        }
    }
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.sharing_available() {
            self.open = false;
            self.menu_open = false;
            self.confirm_logout = false;
            return;
        }
        self.show_logout_confirmation(ctx);
        if !self.open {
            return;
        }
        let response = Modal::new("account_login", 400.).show(ctx, |modal| {
            modal.header(|ui| {
                ui.text_heading(if self.user.is_some() {
                    "Account"
                } else {
                    "Log in to KetchUp"
                });
            });
            modal.body(|ui| {
                if let Some(user) = &self.user {
                    ui.text_label(&user.name);
                } else {
                    ui.text_label("Connect your account using Google in your browser.");
                }
                if self.pending.is_some() {
                    ui.spinner();
                }
                if self.waiting {
                    ui.text_label("Complete login and authorize KetchUp in your browser…");
                }
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
            });
            modal.footer(|ui| {
                if ui.button("Close").clicked() {
                    ui.close();
                }
                if primary_button(
                    ui,
                    "Continue with Google",
                    !self.server.is_empty() && self.pending.is_none() && !self.waiting,
                )
                .clicked()
                {
                    let server = self.server.trim_end_matches('/');
                    if !valid_server(server) {
                        self.error =
                            Some("Use HTTPS, or HTTP on localhost for development.".into());
                        return;
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    self.request(
                        ctx,
                        Request::Login,
                        "POST",
                        format!("{server}/api/client/start"),
                        None,
                    );
                    #[cfg(target_arch = "wasm32")]
                    {
                        let url = format!("{server}/auth/google?popup=1");
                        let open = js_sys::Function::new_with_args(
                            "url",
                            "return !!window.open(url, 'ketchup-sign-in', 'popup,width=500,height=680');",
                        );
                        let opened = open
                            .call1(&js_sys::global(), &url.as_str().into())
                            .ok()
                            .and_then(|result| result.as_bool())
                            .unwrap_or(false);
                        if !opened {
                            ctx.open_url(egui::OpenUrl::new_tab(url));
                        }
                        self.waiting = true;
                        self.login_deadline = ctx.input(|i| i.time) + 600.;
                    }
                }
            });
        });
        if response.should_close() {
            self.open = false;
        }
    }
}
fn valid_server(server: &str) -> bool {
    // Restrict credentials to TLS, with loopback HTTP for local development.
    let authority = server
        .split_once("://")
        .map(|(_, rest)| rest.split('/').next().unwrap_or(""));
    !server.contains(['?', '#', '@'])
        && (server.starts_with("https://") && authority.is_some_and(|a| !a.is_empty())
            || server.starts_with("http://")
                && authority.is_some_and(|a| {
                    a == "localhost"
                        || a == "127.0.0.1"
                        || a.starts_with("localhost:")
                        || a.starts_with("127.0.0.1:")
                }))
}
#[cfg(not(target_arch = "wasm32"))]
fn credential_path(server: &str) -> Option<std::path::PathBuf> {
    // Scope saved credentials to the selected server.
    let key = server
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325u64, |hash, b| {
            (hash ^ *b as u64).wrapping_mul(0x100000001b3)
        });
    Some(
        crate::storage::data_dir()
            .ok()?
            .join(format!("account-{key:016x}.token")),
    )
}
#[cfg(not(target_arch = "wasm32"))]
fn save_credential(server: &str, token: &str) -> Result<(), String> {
    use std::io::Write;
    let path = credential_path(server).ok_or("Cannot locate account storage")?;
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    file.write_all(token.as_bytes()).map_err(|e| e.to_string())
}
fn request(
    ctx: &egui::Context,
    method: &str,
    url: String,
    token: Option<String>,
    csrf: Option<String>,
) -> Receiver<Result<Vec<u8>, String>> {
    let (sender, receiver) = mpsc::channel();
    if !(url.starts_with("https://") || valid_server(&url)) {
        let _ = sender.send(Err("Invalid server address".into()));
        return receiver;
    }
    let ctx = ctx.clone();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let method = method.to_owned();
        let _ = csrf; // Native logout authenticates with its bearer credential.
        std::thread::spawn(move || {
            let result = (|| {
                let agent = ureq::Agent::config_builder()
                    .timeout_global(Some(std::time::Duration::from_secs(15)))
                    .max_redirects(0)
                    .build()
                    .new_agent();
                let authorization = token.map(|token| format!("Bearer {token}"));
                let mut response = if method == "POST" {
                    let mut request = agent.post(&url);
                    if let Some(token) = &authorization {
                        request = request.header("Authorization", token);
                    }
                    request.send_empty()
                } else {
                    let mut request = agent.get(&url);
                    if let Some(token) = &authorization {
                        request = request.header("Authorization", token);
                    }
                    request.call()
                }
                .map_err(|_| "Cannot connect to the configured server. Please retry.".to_owned())?;
                response
                    .body_mut()
                    .with_config()
                    .limit(4 * 1024 * 1024)
                    .read_to_vec()
                    .map_err(|_| "Cannot read server response".into())
            })();
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }
    #[cfg(target_arch = "wasm32")]
    {
        use eframe::wasm_bindgen::JsValue;
        let fetch = js_sys::Function::new_with_args(
            "url, method, csrf",
            "return (async () => { const r = await fetch(url, {method, body: method === 'POST' && csrf ? new URLSearchParams({csrf}) : undefined, credentials: 'include', cache: 'no-store', signal: AbortSignal.timeout(15000)}); if (!r.ok) throw new Error('Connection failed'); const bytes = await r.arrayBuffer(); if (bytes.byteLength > 4194304) throw new Error('Response too large'); return new Uint8Array(bytes); })();",
        );
        let promise = fetch.call3(
            &JsValue::NULL,
            &JsValue::from_str(&url),
            &JsValue::from_str(method),
            &csrf
                .map(|value| JsValue::from_str(&value))
                .unwrap_or(JsValue::NULL),
        );
        let _ = token;
        wasm_bindgen_futures::spawn_local(async move {
            let result = match promise {
                Ok(value) => wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(&value))
                    .await
                    .map(|value| js_sys::Uint8Array::new(&value).to_vec())
                    .map_err(|_| "Cannot connect to the configured server. Please retry.".into()),
                Err(_) => Err("Cannot connect to server".into()),
            };
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }
    receiver
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn logout_clears_profile_credentials_and_open_account_controls() {
        let mut account = Account::default();
        account.user = Some(User {
            id: "ada".into(),
            name: "Ada".into(),
            email: "ada@example.com".into(),
        });
        account.token = Some("test-credential".into());
        account.csrf = Some("test-csrf".into());
        account.open = true;
        account.menu_open = true;
        account.waiting = true;
        account.clear_session();
        assert!(account.user.is_none() && account.texture.is_none());
        assert!(account.token.is_none() && account.csrf.is_none());
        assert!(!account.open && !account.menu_open && !account.waiting);
    }
    #[test]
    fn restrict_server_addresses() {
        for url in [
            "https://ketchup.example",
            "http://localhost:8081",
            "http://127.0.0.1:8081",
        ] {
            assert!(valid_server(url));
        }
        for url in [
            "http://example.com",
            "http://localhost.evil",
            "https://user@example.com",
            "file:///tmp/token",
        ] {
            assert!(!valid_server(url));
        }
    }
    #[test]
    fn anonymous_and_authenticated_profiles() {
        let anonymous: Session =
            serde_json::from_str(r#"{"authenticated":false,"user":null}"#).unwrap();
        assert!(!anonymous.authenticated && anonymous.user.is_none());
        let session: Session = serde_json::from_str(
            r#"{"authenticated":true,"user":{"name":"Ada","picture":"https://example.com/photo"}}"#,
        )
        .unwrap();
        assert_eq!(session.user.unwrap().name, "Ada");
    }
}

/// Project transport preserves status codes so revision conflicts and lock denials
/// remain distinguishable from connectivity failures. Never follows redirects with credentials.
pub(crate) struct ApiReply {
    pub status: u16,
    pub body: serde_json::Value,
}
impl Account {
    pub(crate) fn project_request(
        &self,
        ctx: &egui::Context,
        path: &str,
        body: Option<serde_json::Value>,
        guest: Option<String>,
    ) -> Receiver<Result<ApiReply, String>> {
        project_request(
            ctx,
            format!("{}{}", self.server.trim_end_matches('/'), path),
            body,
            if self.user.is_some() {
                self.token.clone()
            } else {
                None
            },
            self.csrf.clone(),
            guest,
        )
    }
    pub(crate) fn guest_credential(&self) -> Option<String> {
        let key = format!("{}/guest", self.server);
        #[cfg(not(target_arch = "wasm32"))]
        {
            credential_path(&key).and_then(|p| std::fs::read_to_string(p).ok())
        }
        #[cfg(target_arch = "wasm32")]
        {
            web_sys::window()?
                .local_storage()
                .ok()??
                .get_item(&key)
                .ok()?
        }
    }
    pub(crate) fn save_guest_credential(&self, value: &str) -> Result<(), String> {
        let key = format!("{}/guest", self.server);
        #[cfg(not(target_arch = "wasm32"))]
        {
            save_credential(&key, value)
        }
        #[cfg(target_arch = "wasm32")]
        {
            web_sys::window()
                .ok_or("No browser")?
                .local_storage()
                .map_err(|_| "Storage unavailable")?
                .ok_or("Storage unavailable")?
                .set_item(&key, value)
                .map_err(|_| "Cannot save guest identity".into())
        }
    }
}
fn project_request(
    ctx: &egui::Context,
    url: String,
    body: Option<serde_json::Value>,
    token: Option<String>,
    csrf: Option<String>,
    guest: Option<String>,
) -> Receiver<Result<ApiReply, String>> {
    let (sender, receiver) = mpsc::channel();
    if !valid_server(&url) {
        let _ = sender.send(Err("Invalid project server configuration".into()));
        return receiver;
    }
    let ctx = ctx.clone();
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(move || {
        let result = (|| {
            let agent = ureq::Agent::config_builder()
                .timeout_global(Some(std::time::Duration::from_secs(15)))
                .max_redirects(0)
                .http_status_as_error(false)
                .build()
                .new_agent();
            let bearer = token.map(|v| format!("Bearer {v}"));
            let mut response = if let Some(body) = body {
                let mut request = agent.post(&url).header("Content-Type", "application/json");
                if let Some(value) = &bearer {
                    request = request.header("Authorization", value);
                }
                if let Some(value) = &guest {
                    request = request.header("X-Ketchup-Guest", value);
                }
                request.send(body.to_string().as_bytes())
            } else {
                let mut request = agent.get(&url);
                if let Some(value) = &bearer {
                    request = request.header("Authorization", value);
                }
                if let Some(value) = &guest {
                    request = request.header("X-Ketchup-Guest", value);
                }
                request.call()
            }
            .map_err(|e| format!("Project server unavailable: {e}"))?;
            let status = response.status().as_u16();
            let bytes = response
                .body_mut()
                .with_config()
                .limit(17 * 1024 * 1024)
                .read_to_vec()
                .map_err(|e| e.to_string())?;
            Ok(ApiReply {
                status,
                body: serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
            })
        })();
        let _ = csrf;
        let _ = sender.send(result);
        ctx.request_repaint();
    });
    #[cfg(target_arch = "wasm32")]
    {
        use eframe::wasm_bindgen::JsValue;
        let headers = serde_json::json!({"Content-Type":"application/json"});
        let mut headers = headers;
        if let Some(value) = csrf {
            headers["X-CSRF-Token"] = value.into();
        }
        if let Some(value) = guest {
            headers["X-Ketchup-Guest"] = value.into();
        }
        let fetch = js_sys::Function::new_with_args(
            "url, body, headers",
            r#"
            return (async () => {
                const r = await fetch(url, {method: body === null ? 'GET' : 'POST', body: body ?? undefined,
                    headers: JSON.parse(headers), credentials: 'include', redirect: 'error', cache: 'no-store', signal: AbortSignal.timeout(15000)});
                const text = await r.text(); if (text.length > 17825792) throw Error('Project response too large');
                return JSON.stringify({status:r.status, body:JSON.parse(text)});
            })();"#,
        );
        let promise = fetch.call3(
            &JsValue::NULL,
            &JsValue::from_str(&url),
            &body
                .map(|b| JsValue::from_str(&b.to_string()))
                .unwrap_or(JsValue::NULL),
            &JsValue::from_str(&headers.to_string()),
        );
        let _ = token;
        wasm_bindgen_futures::spawn_local(async move {
            let result = async {
                let value = promise.map_err(|_| "Cannot contact project server".to_owned())?;
                let value = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(&value))
                    .await
                    .map_err(|_| "Cannot contact project server".to_owned())?;
                let reply: serde_json::Value =
                    serde_json::from_str(&value.as_string().ok_or("Invalid project response")?)
                        .map_err(|e| e.to_string())?;
                Ok(ApiReply {
                    status: reply["status"].as_u64().ok_or("Missing HTTP status")? as u16,
                    body: reply["body"].clone(),
                })
            }
            .await;
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }
    receiver
}

#[cfg(test)]
mod availability_tests {
    use super::*;

    #[test]
    fn unavailable_server_closes_account_ui_without_sending_requests() {
        let ctx = egui::Context::default();
        let mut account = Account::default();
        account.server = "https://sharing.example".into();
        account.user = Some(User {
            id: "test".into(), name: "Test User".into(), email: String::new(),
        });
        account.open = true;
        account.menu_open = true;
        account.confirm_logout = true;
        account.show(&ctx);
        assert!(!account.open);
        assert!(!account.menu_open);
        assert!(!account.confirm_logout);
        assert!(account.pending.is_none());
        assert!(account.user.is_some());
    }

    #[test]
    fn sharing_requires_a_valid_session_response_even_for_guests() {
        let ctx = egui::Context::default();
        let mut account = Account::default();
        account.server = "https://sharing.example".into();
        assert!(!account.sharing_available());
        let (sender, receiver) = mpsc::channel();
        sender.send(Ok(br#"{"authenticated":false,"user":null}"#.to_vec())).unwrap();
        account.pending = Some((Request::Session, receiver));
        account.poll(&ctx);
        assert!(account.sharing_available());
        let (sender, receiver) = mpsc::channel();
        sender.send(Err("Server unreachable".into())).unwrap();
        account.pending = Some((Request::Session, receiver));
        account.poll(&ctx);
        assert!(!account.sharing_available());
    }
}
