//! Approved project access, optimistic revisions and server-arbitrated selection leases.
use super::*;
use crate::account::ApiReply;
use serde_json::{Value, json};
use std::sync::mpsc::Receiver;

#[derive(Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[serde(default)]
pub(super) struct Preferences {
    pub open: bool,
    pub invitation: String,
    pub local_author_id: String,
}
#[derive(Clone, serde::Deserialize)]
struct AccessRequest {
    id: String,
    name: String,
    role: String,
    status: String,
    #[serde(default)]
    project: String,
}
#[derive(Clone, serde::Deserialize)]
struct Lease {
    #[serde(default)]
    id: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    expires_at: u64,
    #[serde(default)]
    has_avatar: bool,
    connection: String,
    name: String,
    entities: Vec<u64>,
}
#[derive(Clone, serde::Deserialize)]
struct Remote {
    id: String,
    role: String,
    revision: u64,
    updated_at: u64,
    #[serde(default)]
    server_time: u64,
    document: Option<Value>,
    #[serde(default)]
    requests: Vec<AccessRequest>,
    #[serde(default)]
    locks: Vec<Lease>,
    #[serde(default)]
    actor: Option<ketchup_core::HistoryAuthor>,
}
enum Kind {
    Guest,
    Create,
    Read,
    Sync {
        sent: Option<Value>,
        selection: BTreeSet<u64>,
    },
    Access,
    NotificationDecision,
}
struct Pending {
    kind: Kind,
    id: Option<String>,
    receiver: Receiver<Result<ApiReply, String>>,
}
pub(super) struct Sharing {
    pub preferences: Preferences,
    connection: String,
    guest: Option<String>,
    pending: Option<Pending>,
    notifications: Vec<AccessRequest>,
    socket: Option<crate::realtime::Socket>,
    socket_ready: bool,
    socket_actor: Option<ketchup_core::HistoryAuthor>,
    access_confirmed: bool,
    ticket_pending: Option<Receiver<Result<ApiReply, String>>>,
    socket_requests:
        std::collections::BTreeMap<u64, std::sync::mpsc::Sender<Result<ApiReply, String>>>,
    socket_outbox: Vec<Value>,
    socket_sequence: u64,
    socket_retry_at: f64,
    heartbeat_at: f64,
    heartbeat_sent: f64,
    socket_last_message: f64,
    deferred_state: Option<Value>,
    selection_dirty: bool,
    active: Option<String>,
    role: String,
    baseline: Option<Value>,
    remote: Option<Remote>,
    conflict: Option<Remote>,
    error: Option<String>,
    last_ack: f64,
    desired: BTreeSet<u64>,
    confirmed: BTreeSet<u64>,
    frame_document: Option<Document>,
    frame_selection: Option<BTreeSet<u64>>,
    save_after_check: Option<bool>,
    save_ready: Option<bool>,
    force_read: bool,
    lock_conflict: bool,
    acknowledged_account: Option<String>,
    request_started: f64,
    presence_sample_time: f64,
    avatars: std::collections::BTreeMap<String, egui::TextureHandle>,
    avatar_pending: Option<(String, Receiver<Result<ApiReply, String>>)>,
    avatar_failed: BTreeSet<String>,
}
impl Default for Sharing {
    fn default() -> Self {
        Self {
            preferences: Default::default(),
            connection: ketchup_core::MaterialId::new_v4().to_string(),
            guest: None,
            pending: None,

            notifications: vec![],

            socket: None,
            socket_ready: false,
            socket_actor: None,
            access_confirmed: false,
            ticket_pending: None,
            socket_requests: Default::default(),
            socket_outbox: vec![],
            socket_sequence: 0,
            socket_retry_at: 0.,
            heartbeat_at: 0.,
            heartbeat_sent: 0.,
            socket_last_message: f64::NEG_INFINITY,
            deferred_state: None,
            selection_dirty: true,
            active: None,
            role: "none".into(),
            baseline: None,
            remote: None,
            conflict: None,
            error: None,

            last_ack: f64::NEG_INFINITY,
            desired: BTreeSet::new(),
            confirmed: BTreeSet::new(),
            frame_document: None,
            frame_selection: None,
            save_after_check: None,
            save_ready: None,
            force_read: false,
            lock_conflict: false,
            acknowledged_account: None,
            request_started: 0.,
            presence_sample_time: f64::NEG_INFINITY,
            avatars: Default::default(),
            avatar_pending: None,
            avatar_failed: Default::default(),
        }
    }
}
fn shared_content(doc: &Document) -> Value {
    json!({"name":doc.name,"model":doc.model})
}
fn remote_content(remote: &Value) -> Value {
    json!({"name":remote["name"], "model":remote["model"]})
}
fn valid_id(id: &str) -> bool {
    id.len() == 48
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
pub(super) fn now_timestamp() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Date::now() / 1000.) as u64
    }
}
impl KetchupApp {
    fn persist_shared_state(&mut self) {
        // UI fixtures must never write personal recovery files.
        #[cfg(not(test))]
        self.persist();
    }
    pub(super) fn current_history_author(&mut self) -> ketchup_core::HistoryAuthor {
        if let Some(author) = self
            .sharing
            .remote
            .as_ref()
            .and_then(|remote| remote.actor.as_ref())
        {
            let mut author = author.clone();
            author.verified = false;
            return author;
        }
        if let Some(user) = &self.account.user {
            return ketchup_core::HistoryAuthor {
                id: format!("user:{}", user.id),
                name: user.name.clone(),
                kind: "account".into(),
                verified: false,
            };
        }
        if self.sharing.preferences.local_author_id.is_empty() {
            self.sharing.preferences.local_author_id =
                ketchup_core::MaterialId::new_v4().to_string();
        }
        ketchup_core::HistoryAuthor {
            id: format!("local:{}", self.sharing.preferences.local_author_id),
            name: "Local session".into(),
            kind: "local".into(),
            verified: false,
        }
    }
    fn sharing_request(
        &mut self,
        ctx: &egui::Context,
        kind: Kind,
        path: &str,
        body: Option<Value>,
    ) {
        if !self.account.sharing_available() {
            return;
        }
        self.sharing.request_started = ctx.input(|i| i.time);
        self.sharing.pending = Some(Pending {
            kind,
            id: self.sharing.active.clone(),
            receiver: if path == "/api/guest" {
                self.account
                    .project_request(ctx, path, body, self.sharing.guest.clone())
            } else {
                let (sender, receiver) = std::sync::mpsc::channel();
                self.sharing.socket_sequence += 1;
                let seq = self.sharing.socket_sequence;
                self.sharing.socket_requests.insert(seq, sender);
                let command = json!({"type":"request","seq":seq,"path":path,"body":body});
                if self.sharing.socket_ready {
                    if let Some(socket) = &self.sharing.socket {
                        let _ = socket.send(command);
                    }
                } else {
                    self.sharing.socket_outbox.push(command);
                }
                ctx.request_repaint();
                receiver
            },
        });
    }
    pub(super) fn share_menu_label(&self) -> String {
        let pending = self
            .sharing
            .notifications
            .iter()
            .filter(|request| request.status == "pending")
            .count();
        if pending == 0 {
            "Share…".into()
        } else {
            format!("Share… ({pending})")
        }
    }
    pub(super) fn start_sharing(&mut self, ctx: &egui::Context) {
        if !self.account.sharing_available() {
            return;
        }
        self.sharing.preferences.open = true;
        if self.doc.shared_project.is_some() {
            self.sharing.force_read = true;
            return;
        }
        if self.account.user.is_none() {
            self.account.open = true;
            return;
        }
        if self.sharing.pending.is_some() {
            return;
        }
        let project_id = *self
            .doc
            .project_id
            .get_or_insert_with(ketchup_core::MaterialId::new_v4);
        self.persist_shared_state();
        let document = serde_json::to_value(&self.doc).unwrap_or(Value::Null);
        self.sharing_request(
            ctx,
            Kind::Create,
            "/api/projects",
            Some(json!({"project_id":project_id,"document":document})),
        );
    }
    pub(super) fn release_shared_selection(&mut self, ctx: &egui::Context) {
        self.sharing.socket_outbox.clear();
        for (_, sender) in std::mem::take(&mut self.sharing.socket_requests) {
            let _ = sender.send(Err("Project changed".into()));
        }
        if let Some(id) = &self.sharing.active {
            if let Some(socket) = &self.sharing.socket {
                let _=socket.send(json!({"type":"request","seq":0,"path":format!("/api/projects/{id}/disconnect"),"body":{"connection":self.sharing.connection}}));
            }
        }
        let _ = ctx;
        self.sharing.selection_dirty = true;
        self.sharing.deferred_state = None;
        self.sharing.avatars.clear();
        self.sharing.avatar_failed.clear();
        self.sharing.avatar_pending = None;
        self.sharing.presence_sample_time = f64::NEG_INFINITY;
        self.sharing.frame_document = None;
        self.sharing.frame_selection = None;
        self.sharing.lock_conflict = false;
        self.sharing.access_confirmed = false;
        self.sharing.active = None;
        self.sharing.pending = None;
        self.sharing.baseline = None;
        self.sharing.remote = None;
        self.sharing.conflict = None;
        self.sharing.role = "none".into();
        self.sharing.confirmed.clear();
        self.sharing.desired.clear();
        self.sharing.save_after_check = None;
        self.sharing.save_ready = None;
    }
    fn apply_shared_remote(&mut self, ctx: &egui::Context, remote: &Remote) -> Result<(), String> {
        self.sharing.frame_document = None;
        self.sharing.frame_selection = None;
        let document = remote
            .document
            .as_ref()
            .ok_or("Access is awaiting approval")?;
        let mut doc = Document::from_json(&document.to_string())?;
        // Navigation/display/selection are local. Only project content is replicated.
        doc.camera = self.doc.camera.clone();
        doc.display = self.doc.display.clone();
        doc.viewport_cameras = self.doc.viewport_cameras.clone();
        doc.selection.clear();
        doc.selection_context = None;
        doc.shared_project = Some(ketchup_core::SharedProject {
            server: self.account.server.clone(),
            id: remote.id.clone(),
            revision: remote.revision,
            server_updated_at: remote.updated_at,
            file_updated_at: self
                .doc
                .shared_project
                .as_ref()
                .map_or(0, |l| l.file_updated_at),
        });
        let path = self.project.path.clone();
        // Do not replace the whole project: preserve file operation / unsaved prompts.
        let edit_context = self.doc.model.edit_context;
        let previous_selection = self.sharing.desired.clone();
        self.doc.project_author = doc.project_author;
        self.doc.model = doc.model;
        self.doc.model.edit_context =
            edit_context.filter(|id| self.doc.model.objects.contains_key(id));
        self.doc.name = doc.name;
        self.doc.history = doc.history;
        self.doc.shared_project = doc.shared_project;
        self.sharing.desired.retain(|id| {
            self.doc.model.entity_in_context(*id) == Some(*id) && !self.doc.model.is_hidden(*id)
        });
        self.sharing.confirmed.retain(|id| {
            self.doc.model.entity_in_context(*id) == Some(*id) && !self.doc.model.is_hidden(*id)
        });
        self.selected = self.sharing.desired.clone();
        self.doc.selection = self.sharing.desired.clone();
        self.doc.selection_context = self.doc.model.edit_context;
        if previous_selection != self.sharing.desired {
            self.sharing.selection_dirty = true;
        }
        self.project.path = path;
        self.entity_cache = renderer::EntityCache::default();
        self.face_cache = renderer::FaceCache::default();
        self.picking_cache = crate::picking::PickingCache::default();
        self.changed(ctx);
        Ok(())
    }
    fn accept_shared_reply(
        &mut self,
        ctx: &egui::Context,
        remote: Remote,
        kind: Kind,
    ) -> Result<(), String> {
        if !matches!(remote.role.as_str(), "owner" | "edit" | "view" | "none") {
            return Err("Invalid server access response".into());
        }
        if !matches!(&kind, Kind::Create)
            && self
                .sharing
                .active
                .as_ref()
                .is_some_and(|id| id != &remote.id)
        {
            return Err("The access response belongs to another project".into());
        }
        // This role is authoritative server metadata, independent of whether the
        // accompanying geometry can be loaded. Model validation and a fresh
        // confirmed lease remain mandatory before any edit is authorized.
        self.sharing.access_confirmed = true;
        self.sharing.role = remote.role.clone();
        self.sharing.remote = Some(remote.clone());
        self.sharing.presence_sample_time = self.sharing.request_started;
        let incoming = remote.document.as_ref().map(remote_content);
        if remote.role != "none" && remote.document.is_none() {
            self.sharing.baseline = None;
            self.sharing.confirmed.clear();
            self.sharing.last_ack = f64::NEG_INFINITY;
            return Err(format!(
                "The server confirmed {} access but did not provide the project data",
                remote.role
            ));
        }
        if let Some(document) = &remote.document {
            let acknowledged = match Document::from_json(&document.to_string()) {
                Ok(document) => document,
                Err(error) => {
                    self.sharing.baseline = None;
                    self.sharing.confirmed.clear();
                    self.sharing.last_ack = f64::NEG_INFINITY;
                    return Err(format!(
                        "The server confirmed {} access, but the project data could not be loaded: {error}",
                        remote.role
                    ));
                }
            };
            self.doc.history.merge_attribution(&acknowledged.history);
            self.doc.project_author = acknowledged.project_author;
        }
        let local = shared_content(&self.doc);
        if matches!(&kind, Kind::Create) && self.doc.shared_project.is_none() {
            self.doc.shared_project = Some(ketchup_core::SharedProject {
                server: self.account.server.clone(),
                id: remote.id.clone(),
                revision: remote.revision,
                server_updated_at: remote.updated_at,
                file_updated_at: 0,
            });
            self.changed(ctx);
            self.persist_shared_state();
        }
        let current_link = self.doc.shared_project.clone();
        let is_create = matches!(&kind, Kind::Create);
        let sent = match &kind {
            Kind::Sync { sent, .. } => sent.as_ref(),
            _ => None,
        };
        let baseline = sent.or(self.sharing.baseline.as_ref());
        let local_dirty = baseline.is_some_and(|b| *b != local);
        let invitation = current_link.as_ref().is_some_and(|l| l.revision == 0);
        let initial_conflict = baseline.is_none() && !is_create && !invitation;
        if let Some(incoming) = incoming {
            let remote_changed = baseline.is_some_and(|b| *b != incoming);
            if incoming != local
                && !invitation
                && ((local_dirty && remote_changed)
                    || initial_conflict
                    || is_create
                    || self.sharing.lock_conflict)
            {
                self.sharing.conflict = Some(remote.clone());
                self.sharing.preferences.open = true;
                self.sharing.save_after_check = None;
            } else {
                // A successful upload acknowledges only its snapshot; retain edits
                // made while it was in flight. A read only replaces a clean baseline.
                let needs_apply = incoming != local
                    && !is_create
                    && ((!local_dirty && baseline.is_some()) || invitation);
                if needs_apply {
                    self.apply_shared_remote(ctx, &remote)?;
                }
                self.sharing.baseline = Some(incoming);
                self.doc.shared_project = Some(ketchup_core::SharedProject {
                    server: self.account.server.clone(),
                    id: remote.id.clone(),
                    revision: remote.revision,
                    server_updated_at: remote.updated_at,
                    file_updated_at: current_link.as_ref().map_or(0, |l| l.file_updated_at),
                });
                if current_link != self.doc.shared_project {
                    self.persist_shared_state();
                }
                if invitation {
                    self.remember_recent_project(None);
                }
                if let Kind::Sync { selection, .. } = &kind {
                    self.sharing.selection_dirty = *selection != self.sharing.desired;
                    self.sharing.confirmed = selection.clone();
                    // This reply may acknowledge an older click. Keep the latest
                    // local intent visible while its own lease is in flight.
                    self.selected = self.sharing.desired.clone();
                    self.sharing.last_ack = self.sharing.request_started;
                }
                if let Some(save_as) = self.sharing.save_after_check.take() {
                    self.sharing.save_ready = Some(save_as);
                }
                if is_create {
                    self.changed(ctx);
                    self.persist_shared_state();
                }
            }
        }
        if remote.role == "none" {
            self.sharing.baseline = None;
            self.sharing.confirmed.clear();
            self.selected.clear();
        }
        self.sharing.lock_conflict = false;
        self.sharing.active = Some(remote.id.clone());
        self.sharing.role = remote.role.clone();
        self.sharing.remote = Some(remote);
        Ok(())
    }
    pub(super) fn poll_sharing(&mut self, ctx: &egui::Context) {
        if !self.account.sharing_available() {
            if self.sharing.active.is_some() || self.sharing.socket.is_some() {
                self.close_realtime("Server unavailable");
                self.release_shared_selection(ctx);
            }
            return;
        }
        if self.sharing.guest.is_none() {
            self.sharing.guest = self.account.guest_credential();
        }
        let now = ctx.input(|i| i.time);
        let account = self.account.user.as_ref().map(|u| u.id.clone());
        if account != self.sharing.acknowledged_account {
            let restore_selection =
                self.sharing.last_ack == f64::NEG_INFINITY && self.sharing.role == "none";
            self.close_realtime("Account changed");
            self.sharing.acknowledged_account = account;
            self.sharing.pending = None;

            self.sharing.notifications.clear();
            self.sharing.remote = None;
            self.sharing.role = "none".into();
            self.sharing.access_confirmed = false;
            if !restore_selection {
                self.sharing.desired.clear();
            }
            self.selected.clear();

            self.sharing.force_read = true;
            self.sharing.confirmed.clear();
            self.sharing.last_ack = f64::NEG_INFINITY;
        }
        self.pump_realtime(ctx, now);
        if !self.account.sharing_available() {
            self.release_shared_selection(ctx);
            return;
        }
        if let Some(pending) = &self.sharing.pending {
            let result = match pending.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Project request interrupted".into()))
                }
                Err(_) => None,
            };
            if let Some(result) = result {
                let pending = self.sharing.pending.take().unwrap();
                // A file switch cannot apply a response for the previous project.
                if pending.id == self.sharing.active {
                    let result = result.and_then(|reply| {
                        if !(200..300).contains(&reply.status) {
                            if reply.status == 401 || reply.status == 403 {
                                self.sharing.role = "none".into();
                                self.sharing.access_confirmed = false;
                                self.sharing.confirmed.clear();
                                self.selected.clear();
                                self.sharing.last_ack = f64::NEG_INFINITY;
                                self.sharing.force_read = true;
                                if reply.status == 401 {
                                    self.account.refresh(ctx);
                                }
                            }
                            if reply.status == 409 || reply.status == 423 || reply.status == 422 {
                                self.sharing.force_read = true;
                            }
                            if reply.status == 423 || reply.status == 422 {
                                self.sharing.lock_conflict =
                                    matches!(&pending.kind, Kind::Sync { sent: Some(_), .. });
                                self.sharing.selection_dirty = true;
                                self.sharing.desired.clear();
                                self.sharing.confirmed.clear();
                                self.selected.clear();
                            }
                            self.sharing.save_after_check = None;
                            return Err(reply.body["error"]
                                .as_str()
                                .unwrap_or("Project request failed")
                                .to_owned());
                        }
                        if matches!(&pending.kind, Kind::NotificationDecision) {
                            return Ok(());
                        }
                        if matches!(&pending.kind, Kind::Guest) {
                            let token = reply.body["token"]
                                .as_str()
                                .ok_or("Missing guest identity")?;
                            self.account.save_guest_credential(token)?;
                            self.sharing.guest = Some(token.to_owned());
                            return Ok(());
                        }
                        let remote: Remote =
                            serde_json::from_value(reply.body).map_err(|e| e.to_string())?;
                        self.accept_shared_reply(ctx, remote, pending.kind)
                    });
                    self.sharing.error = result.err();
                }
            }
        }
        let link = self.doc.shared_project.clone();
        if let Some(link) = &link {
            if self.sharing.active.as_ref() != Some(&link.id) {
                self.release_shared_selection(ctx);
                if link.server.trim_end_matches('/') != self.account.server.trim_end_matches('/')
                    || !valid_id(&link.id)
                {
                    self.sharing.error = Some("This file belongs to a different or invalid server. Open Share to inspect it; credentials were not sent.".into());
                    return;
                }
                self.sharing.active = Some(link.id.clone());
                self.sharing.desired = self.doc.selection.clone();
                self.sharing.force_read = true;
            }
        } else if self.sharing.active.is_some() {
            self.release_shared_selection(ctx);
        }
        if let Some(id) = self.sharing.active.clone() {
            if self.sharing.pending.is_none()
                && (self.sharing.force_read
                    || self.sharing.baseline.is_none()
                    || self.sharing.selection_dirty
                    || self
                        .sharing
                        .baseline
                        .as_ref()
                        .is_some_and(|b| *b != shared_content(&self.doc)))
            {
                if self.account.user.is_none() && self.sharing.guest.is_none() {
                    self.sharing_request(ctx, Kind::Guest, "/api/guest", Some(json!({})));
                } else if self.sharing.force_read
                    || (self.sharing.baseline.is_none() && self.sharing.remote.is_none())
                {
                    self.sharing.force_read = false;
                    self.sharing_request(ctx, Kind::Read, &format!("/api/projects/{id}"), None);
                } else if self.sharing.conflict.is_none()
                    && self.sharing.role != "none"
                    && self.sharing.baseline.is_some()
                {
                    let content = shared_content(&self.doc);
                    let dirty = self
                        .sharing
                        .baseline
                        .as_ref()
                        .is_some_and(|b| *b != content);
                    let can_write = matches!(self.sharing.role.as_str(), "owner" | "edit");
                    if !dirty && !self.sharing.selection_dirty {
                        return;
                    }
                    let sent = (dirty && can_write).then_some(content);
                    let mut body = json!({"connection":self.sharing.connection,"selection":self.sharing.desired,
                        "base_revision":link.as_ref().map_or(0, |l| l.revision)});
                    if sent.is_some() {
                        body["document"] = serde_json::to_value(&self.doc).unwrap_or(Value::Null);
                    }
                    self.sharing_request(
                        ctx,
                        Kind::Sync {
                            sent,
                            selection: self.sharing.desired.clone(),
                        },
                        &format!("/api/projects/{id}/sync"),
                        Some(body),
                    );
                }
            }
        }
        if self.sharing.pending.is_none() {
            if let Some(body) = self.sharing.deferred_state.take() {
                self.apply_socket_state(ctx, body, now);
            }
        }
        self.poll_presence_avatars(ctx, now);
        if let Some(save_as) = self.sharing.save_ready.take() {
            self.save_project_checked(ctx, save_as);
        }
    }
    pub(super) fn shared_save_check(&mut self, ctx: &egui::Context, save_as: bool) -> bool {
        if !self.account.sharing_available() || self.doc.shared_project.is_none() {
            return false;
        }
        if self.sharing.conflict.is_some() {
            self.sharing.preferences.open = true;
            return true;
        }
        self.sharing.save_after_check = Some(save_as);
        self.sharing.force_read = true;
        self.say(
            ctx,
            "Checking server revision and file timestamp before saving…",
        );
        true
    }
    pub(super) fn shared_selection_to_remember(&self) -> Option<&BTreeSet<u64>> {
        (self.account.sharing_available() && self.doc.shared_project.is_some() && self.sharing.active.is_some())
            .then_some(&self.sharing.desired)
    }
    pub(super) fn shared_edit_allowed(&self, ctx: &egui::Context) -> bool {
        !self.account.sharing_available() || self.doc.shared_project.is_none()
            || (matches!(self.sharing.role.as_str(), "owner" | "edit")
                && self.sharing.conflict.is_none()
                && self.sharing.baseline.is_some()
                && ctx.input(|i| i.time) - self.sharing.last_ack < 10.
                // Creation, grouping and deletion can change selection as a
                // result of an authorized edit. Check its starting selection.
                && self.sharing.frame_selection.as_ref().unwrap_or(&self.selected)
                    == &self.sharing.confirmed
                && self.sharing.desired == self.sharing.confirmed)
    }
    pub(super) fn shared_edit_block_reason(&self, ctx: &egui::Context) -> &'static str {
        if !matches!(self.sharing.role.as_str(), "owner" | "edit") {
            "Editing access has not been confirmed. Check your account and File → Share."
        } else if self.sharing.conflict.is_some() {
            "Resolve the project version conflict in File → Share before editing."
        } else if self.sharing.access_confirmed
            && self.sharing.socket_ready
            && self.sharing.baseline.is_none()
        {
            "The server confirmed editing access, but project data is unavailable. Review the error in File → Share."
        } else if self.sharing.baseline.is_none()
            || ctx.input(|i| i.time) - self.sharing.last_ack >= 10.
        {
            "Waiting for the server connection before editing. Your editing access is unchanged."
        } else {
            "Waiting for the server to confirm the object's selection lock."
        }
    }
    pub(super) fn shared_fastener_edit_allowed(
        &self,
        ctx: &egui::Context,
        pair: &BTreeSet<u64>,
    ) -> bool {
        !self.account.sharing_available() || self.doc.shared_project.is_none()
            || (matches!(self.sharing.role.as_str(), "owner" | "edit")
                && self.sharing.conflict.is_none()
                && self.sharing.baseline.is_some()
                && ctx.input(|i| i.time) - self.sharing.last_ack < 10.
                && *pair == self.sharing.confirmed
                && self.sharing.desired == self.sharing.confirmed)
    }
    pub(super) fn sharing_before_frame(&mut self, ctx: &egui::Context) {
        if self.account.sharing_available() && self.doc.shared_project.is_some() {
            if ctx.input(|i| i.time) - self.sharing.last_ack > 10. {
                if !self.sharing.confirmed.is_empty() {
                    self.sharing.selection_dirty = true;
                    ctx.request_repaint();
                }
                self.sharing.confirmed.clear();
            }
            self.sharing.frame_selection = Some(self.selected.clone());
            self.sharing.frame_document = Some(self.doc.clone());
        }
    }
    pub(super) fn sharing_after_frame(&mut self, ctx: &egui::Context) {
        let Some(before) = self.sharing.frame_document.take() else {
            return;
        };
        if self
            .sharing
            .baseline
            .as_ref()
            .is_some_and(|b| *b != shared_content(&self.doc))
        {
            ctx.request_repaint();
        }
        let writable = self.shared_edit_allowed(ctx);
        // Compare with the start of this frame, not the last server lease.
        // A pending selection is local intent, not a new click on every frame.
        let selection_changed = self
            .sharing
            .frame_selection
            .take()
            .is_some_and(|previous| previous != self.selected);
        if selection_changed {
            self.sharing.desired = self.selected.clone();
            self.sharing.selection_dirty = true;
            ctx.request_repaint();
        }
        if !writable && shared_content(&before) != shared_content(&self.doc) {
            self.doc.model = before.model;
            self.doc.name = before.name;
            self.doc.history = before.history;
            self.changed(ctx);
            self.persist_shared_state();
            self.say(ctx, self.shared_edit_block_reason(ctx));
        }
    }
    fn sharing_access_label(&self) -> &str {
        if self.sharing.access_confirmed {
            &self.sharing.role
        } else {
            "Waiting for server confirmation"
        }
    }
    fn sharing_identity_status(&self) -> String {
        let account = self
            .account
            .user
            .as_ref()
            .map_or("Not signed in", |user| user.name.as_str());
        let actor = self.sharing.socket_actor.as_ref().or_else(|| {
            self.sharing
                .remote
                .as_ref()
                .and_then(|remote| remote.actor.as_ref())
        });
        let connection = actor
            .map(|actor| {
                format!(
                    "{} ({})",
                    actor.name,
                    if actor.kind == "guest" {
                        "Guest"
                    } else {
                        "Account"
                    }
                )
            })
            .unwrap_or_else(|| {
                if self.sharing.socket_ready {
                    "Connected; server identity not reported".into()
                } else {
                    "Not connected".into()
                }
            });
        format!("Account: {account} · Sharing connection: {connection}")
    }
    pub(super) fn share_dialog(&mut self, ctx: &egui::Context) {
        if !self.account.sharing_available() || !self.sharing.preferences.open {
            return;
        }
        let mut action = None;
        let response = Modal::new("project_share", 560.).show(ctx, |modal| {
            modal.header(|ui| { ui.text_heading("Share project"); });
            modal.body(|ui| {
                if let Some(link) = &self.doc.shared_project {
                    let url = format!("{}/share/{}", link.server.trim_end_matches('/'), link.id);
                    ui.label("Persistent invitation link · author approval required for reading and editing");
                    ui.horizontal_wrapped(|ui| { ui.hyperlink(&url); if ui.button("Copy link").clicked() { ctx.copy_text(url); } });
                    if ui.button("Keep local as separate project").clicked() { action = Some(("detach".into(), String::new())); }
                    if let Some(author) = &self.doc.project_author { ui.label(format!("Original project author: {}", author.name)); }
                    let access = self.sharing_access_label();
                    ui.label(format!("Access: {} · reference revision: {}", access, link.revision));
                    ui.label(self.sharing_identity_status());
                    if self.account.user.is_none() && ui.button("Sign in to your account").clicked() { action=Some(("sign_in".into(),String::new())); }
                    if let (Some(user),Some(actor))=(&self.account.user,&self.sharing.socket_actor) {
                        if actor.id != format!("user:{}",user.id) {
                            ui.colored_label(ui.visuals().warn_fg_color,"The sharing connection uses a different identity. Refresh access to reconnect with your current account.");
                        }
                    }
                    if ui.button("Refresh access").clicked() { action = Some(("refresh".into(),String::new())); }
                    ui.label(format!("Server timestamp: {} · file timestamp: {} (UTC epoch seconds)", link.server_updated_at, link.file_updated_at));
                    if self.sharing.role == "none" || self.sharing.role == "view" {
                        ui.horizontal(|ui| {
                            if ui.add_enabled(self.sharing.access_confirmed, egui::Button::new("Request viewing access")).clicked() { action = Some(("view".to_owned(), String::new())); }
                            if ui.add_enabled(self.sharing.access_confirmed, egui::Button::new("Request editing access")).clicked() { action = Some(("edit".to_owned(), String::new())); }
                        });
                    }
                    if let Some(remote) = &self.sharing.remote {
                        for request in &remote.requests {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(format!("{} · {} · {}", request.name, request.role, request.status));
                                if remote.role == "owner" && request.status == "pending" {
                                    if ui.button("Approve").clicked() { action = Some(("approve".into(), request.id.clone())); }
                                    if ui.button("Deny").clicked() { action = Some(("deny".into(), request.id.clone())); }
                                }
                            });
                        }
                        for lease in &remote.locks {
                            if lease.connection != self.sharing.connection && presence_live(lease, remote, self.sharing.presence_sample_time, ctx.input(|i|i.time)) {
                                ui.label(format!("Occupied by {}: {:?}", lease.name, lease.entities));
                            }
                        }
                    }
                } else {
                    ui.label("Share uploads this project to your configured KetchUp3D Server. Visitors must request access.");
                    if ui.button("Create invitation link").clicked() { action = Some(("create".into(), String::new())); }
                }
                ui.separator();
                ui.label("Open an invitation link");
                ui.text_edit_singleline(&mut self.sharing.preferences.invitation);
                if ui.button("Open shared project").clicked() { action = Some(("open".into(), String::new())); }
                let header = egui::CollapsingHeader::new(format!("Access notifications ({})", self.sharing.notifications.len()))
                    .id_salt("share_notifications").open(sidebar::restored_header_open(ui, "sharing_notifications", false)).show(ui, |ui| {
                        for request in &self.sharing.notifications {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(format!("{} · {} · {}", request.name, request.role, request.status));
                                if request.status == "pending" {
                                    if ui.button("Approve").clicked() { action = Some(("notification_approve".into(), format!("{}/{}", request.project, request.id))); }
                                    if ui.button("Deny").clicked() { action = Some(("notification_deny".into(), format!("{}/{}", request.project, request.id))); }
                                }
                                if ui.button("Review project").clicked() { action = Some(("review".into(), request.project.clone())); }
                            });
                        }
                    });
                sidebar::remember_section(ctx, header.header_response.id, "sharing_notifications");
                if let Some(conflict) = &self.sharing.conflict {
                    ui.colored_label(ui.visuals().error_fg_color, format!("Server revision {} ({}) differs from this file. Local changes have been kept. Choose a version; another concurrent update will be checked again.", conflict.revision, conflict.updated_at));
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Use server version").clicked() { action = Some(("server".into(), String::new())); }
                        if matches!(conflict.role.as_str(), "owner" | "edit") && ui.button("Publish local version").clicked() { action = Some(("local".into(), String::new())); }
                        if ui.button("Keep local as separate project").clicked() { action = Some(("detach".into(), String::new())); }
                    });
                }
                if self.sharing.pending.is_some() { ui.spinner(); }
                if let Some(error) = &self.sharing.error { ui.colored_label(ui.visuals().error_fg_color, error); }
            });
            modal.footer(|ui| ui.button("Close").clicked())
        });
        if response.inner || response.should_close() {
            self.sharing.preferences.open = false;
        }
        if let Some((action, argument)) = action {
            if self.sharing.pending.is_some() && action != "refresh" && action != "sign_in" {
                return;
            }
            self.handle_share_action(ctx, &action, &argument);
        }
    }
    fn handle_share_action(&mut self, ctx: &egui::Context, action: &str, argument: &str) {
        if !self.account.sharing_available() {
            return;
        }
        match action {
            "create" => self.start_sharing(ctx),
            "sign_in" => {
                self.sharing.preferences.open = false;
                self.account.open = true;
                ctx.request_repaint();
            }
            "refresh" => {
                self.close_realtime("Refreshing sharing identity");
                self.sharing.pending = None;
                self.sharing.socket_retry_at = 0.;
                self.sharing.force_read = true;
                self.sharing.access_confirmed = false;
                self.sharing.role = "none".into();
                self.sharing.baseline = None;
                self.sharing.remote = None;
                self.sharing.error = None;
                self.account.refresh(ctx);
                ctx.request_repaint();
            }
            "open" | "review" => {
                let raw = if action == "review" {
                    argument
                } else {
                    self.sharing.preferences.invitation.trim()
                };
                let prefix = format!("{}/share/", self.account.server.trim_end_matches('/'));
                let id = raw.strip_prefix(&prefix).unwrap_or(raw).to_owned();
                if !valid_id(&id) {
                    self.sharing.error =
                        Some("Use an invitation from the configured server.".into());
                    return;
                }
                if self.project.modified {
                    self.sharing.error =
                        Some("Save your current project before opening an invitation.".into());
                    return;
                }
                // An empty baseline marks a new invitation, not a conflicting file.
                self.replace_project(Document::default(), None, ctx);
                self.doc.shared_project = Some(ketchup_core::SharedProject {
                    server: self.account.server.clone(),
                    id: id.clone(),
                    revision: 0,
                    server_updated_at: 0,
                    file_updated_at: 0,
                });
                self.sharing.active = Some(id);
                self.sharing.force_read = true;
                self.sharing.baseline = None;
                self.project.path = None;
            }
            "server" | "local" => {
                if let Some(remote) = self.sharing.conflict.take() {
                    if action == "server" {
                        if let Err(error) = self.apply_shared_remote(ctx, &remote) {
                            self.sharing.error = Some(error);
                            return;
                        }
                    } else if let Some(link) = &mut self.doc.shared_project {
                        link.revision = remote.revision;
                        link.server_updated_at = remote.updated_at;
                    }
                    self.sharing.baseline = remote.document.as_ref().map(remote_content);

                    ctx.request_repaint();
                }
            }
            "detach" => {
                self.release_shared_selection(ctx);
                self.doc.shared_project = None;
                self.doc.project_id = None;
                self.project.path = None;
                self.changed(ctx);
            }
            "view" | "edit" => {
                if let Some(id) = self.sharing.active.clone() {
                    if self.account.user.is_none() && self.sharing.guest.is_none() {
                        return;
                    }
                    self.sharing_request(
                        ctx,
                        Kind::Access,
                        &format!("/api/projects/{id}/requests"),
                        Some(json!({"role":action})),
                    );
                }
            }
            "notification_approve" | "notification_deny" => {
                if let Some((project, request)) = argument.split_once('/') {
                    self.sharing_request(
                        ctx,
                        Kind::NotificationDecision,
                        &format!("/api/projects/{project}/requests/{request}"),
                        Some(json!({"approve":action == "notification_approve"})),
                    );
                }
            }
            "approve" | "deny" => {
                if let Some(id) = self.sharing.active.clone() {
                    self.sharing_request(
                        ctx,
                        Kind::Access,
                        &format!("/api/projects/{id}/requests/{argument}"),
                        Some(json!({"approve":action == "approve"})),
                    );
                }
            }
            _ => {}
        }
    }
}

impl KetchupApp {
    pub(super) fn open_shared_startup(&mut self, _ctx: &egui::Context) {
        #[cfg(target_arch = "wasm32")]
        let invitation = js_sys::Function::new_no_args(
            "return new URLSearchParams(location.search).get('share')",
        )
        .call0(&eframe::wasm_bindgen::JsValue::NULL)
        .ok()
        .and_then(|v| v.as_string());
        #[cfg(not(target_arch = "wasm32"))]
        let invitation = std::env::args().skip(1).find_map(|arg| {
            arg.strip_prefix(&format!(
                "{}/share/",
                self.account.server.trim_end_matches('/')
            ))
            .map(str::to_owned)
        });
        if let Some(id) = invitation.filter(|id| valid_id(id)) {
            self.sharing.preferences.invitation = id;
            self.sharing.preferences.open = true;
        }
    }
    pub(super) fn shared_status(&self, now: f64) -> Option<String> {
        if !self.account.sharing_available() {
            return None;
        }
        self.doc.shared_project.as_ref()?;
        if self.sharing.conflict.is_some() {
            return Some("Sync conflict · File → Share".into());
        }
        if let Some(error) = &self.sharing.error {
            return Some(format!("Sync: {error}"));
        }
        let occupied: Vec<_> = self
            .sharing
            .remote
            .as_ref()
            .into_iter()
            .flat_map(|r| &r.locks)
            .filter(|l| {
                l.connection != self.sharing.connection
                    && self.sharing.remote.as_ref().is_some_and(|r| {
                        presence_live(l, r, self.sharing.presence_sample_time, now)
                    })
            })
            .map(|l| l.name.as_str())
            .collect();
        if !occupied.is_empty() {
            return Some(format!(
                "Objects occupied by {} · File → Share",
                occupied.join(", ")
            ));
        }
        if self.sharing.desired != self.sharing.confirmed {
            return Some("Waiting for exclusive selection…".into());
        }
        Some(format!("Shared · {}", self.sharing.role))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> KetchupApp {
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.account.server = "https://sharing.example".into();
        app.account.server_reachable = true;
        app.doc.shared_project = Some(ketchup_core::SharedProject {
            server: app.account.server.clone(),
            id: "a".repeat(48),
            revision: 1,
            server_updated_at: 100,
            file_updated_at: 100,
        });
        app.sharing.active = Some("a".repeat(48));
        app
    }
    fn remote(app: &KetchupApp, name: &str, revision: u64) -> Remote {
        let mut doc = app.doc.clone();
        doc.name = name.into();
        Remote {
            id: "a".repeat(48),
            role: "owner".into(),
            revision,
            updated_at: 100 + revision,
            server_time: 100 + revision,
            document: Some(serde_json::to_value(doc).unwrap()),
            requests: vec![],
            locks: vec![],
            actor: None,
        }
    }
    #[test]
    fn unavailable_server_bypasses_sharing_without_removing_project_link() {
        let ctx = egui::Context::default();
        let mut app = app();
        app.account.server_reachable = false;
        let link = app.doc.shared_project.clone();
        assert!(app.shared_edit_allowed(&ctx));
        assert!(app.shared_fastener_edit_allowed(&ctx, &BTreeSet::from([1, 2])));
        assert!(!app.shared_save_check(&ctx, false));
        assert!(app.shared_selection_to_remember().is_none());
        assert!(app.shared_status(0.).is_none());
        app.start_sharing(&ctx);
        assert!(!app.sharing.preferences.open);
        app.sharing_before_frame(&ctx);
        assert!(app.sharing.frame_document.is_none());
        app.poll_sharing(&ctx);
        assert!(app.sharing.pending.is_none());
        assert!(app.sharing.active.is_none());
        assert_eq!(app.doc.shared_project, link);
        app.account.server_reachable = true;
        assert!(!app.shared_edit_allowed(&ctx));
    }

    #[test]
    fn fastener_changes_require_current_edit_approval_and_matching_lease() {
        let ctx = egui::Context::default();
        let mut app = app();
        let pair = BTreeSet::from([1, 2]);
        assert!(!app.shared_fastener_edit_allowed(&ctx, &pair));
        app.sharing.role = "edit".into();
        app.sharing.last_ack = ctx.input(|i| i.time);
        app.sharing.baseline = Some(shared_content(&app.doc));
        app.sharing.confirmed = pair.clone();
        app.sharing.desired = pair.clone();
        assert!(app.shared_fastener_edit_allowed(&ctx, &pair));
        assert!(!app.shared_fastener_edit_allowed(&ctx, &BTreeSet::from([1, 3])));
        app.sharing.role = "view".into();
        assert!(!app.shared_fastener_edit_allowed(&ctx, &pair));
        app.sharing.role = "edit".into();
        app.sharing.last_ack = -11.;
        assert!(!app.shared_fastener_edit_allowed(&ctx, &pair));
    }
    #[test]
    fn file_divergence_requires_a_choice_even_with_matching_revision_and_timestamp() {
        let ctx = egui::Context::default();
        let mut app = app();
        let remote = remote(&app, "Server changes", 1);
        app.accept_shared_reply(&ctx, remote, Kind::Read).unwrap();
        assert!(app.sharing.conflict.is_some());
        assert_eq!(app.doc.name, "Untitled model");
        assert!(app.sharing.preferences.open);
    }
    #[test]
    fn concurrent_changes_preserve_local_work_and_reference_revision() {
        let ctx = egui::Context::default();
        let mut app = app();
        app.sharing.baseline = Some(shared_content(&app.doc));
        app.doc.name = "Local changes".into();
        let remote = remote(&app, "Remote changes", 2);
        app.accept_shared_reply(&ctx, remote, Kind::Read).unwrap();
        assert!(app.sharing.conflict.is_some());
        assert_eq!(app.doc.name, "Local changes");
        assert_eq!(app.doc.shared_project.as_ref().unwrap().revision, 1);
        app.handle_share_action(&ctx, "local", "");
        assert!(app.sharing.conflict.is_none());
        assert_eq!(app.doc.shared_project.as_ref().unwrap().revision, 2);
        assert_eq!(app.doc.name, "Local changes");
        assert_ne!(app.sharing.baseline, Some(shared_content(&app.doc)));
    }
    #[test]
    fn an_upload_acknowledges_only_its_snapshot() {
        let ctx = egui::Context::default();
        let mut app = app();
        app.sharing.baseline = Some(shared_content(&app.doc));
        app.doc.name = "Sent changes".into();
        let sent = shared_content(&app.doc);
        let remote = remote(&app, "Sent changes", 2);
        app.doc.name = "Newer local changes".into();
        app.accept_shared_reply(
            &ctx,
            remote,
            Kind::Sync {
                sent: Some(sent.clone()),
                selection: BTreeSet::new(),
            },
        )
        .unwrap();
        assert_eq!(app.doc.name, "Newer local changes");
        assert_eq!(app.sharing.baseline, Some(sent));
        assert!(app.sharing.conflict.is_none());
        assert_eq!(app.doc.shared_project.as_ref().unwrap().revision, 2);
    }
    #[test]
    fn remote_updates_preserve_local_camera_and_display() {
        let ctx = egui::Context::default();
        let mut app = app();
        app.sharing.baseline = Some(shared_content(&app.doc));
        let remote = remote(&app, "Updated remotely", 2);
        let camera = app.doc.camera.clone();
        let display = app.doc.display.clone();
        app.accept_shared_reply(&ctx, remote, Kind::Read).unwrap();
        assert_eq!(app.doc.name, "Updated remotely");
        assert_eq!(app.doc.camera, camera);
        assert_eq!(app.doc.display, display);
    }
    #[test]
    fn selection_stays_visible_but_editing_waits_for_the_server_lease() {
        let ctx = egui::Context::default();
        let mut app = app();
        app.sharing_before_frame(&ctx);
        app.selected.insert(42);
        app.sharing_after_frame(&ctx);
        assert_eq!(app.selected, BTreeSet::from([42]));
        assert!(app.sharing.desired.contains(&42));
        assert!(app.sharing.confirmed.is_empty());
        assert!(!app.shared_edit_allowed(&ctx));
        app.sharing_before_frame(&ctx);
        app.sharing_after_frame(&ctx);
        assert_eq!(app.selected, BTreeSet::from([42]));
        assert_eq!(app.sharing.desired, BTreeSet::from([42]));
        let remote = remote(&app, "Untitled model", 1);
        app.accept_shared_reply(
            &ctx,
            remote,
            Kind::Sync {
                sent: None,
                selection: BTreeSet::from([42]),
            },
        )
        .unwrap();
        assert_eq!(app.selected, BTreeSet::from([42]));
    }
    #[test]
    fn delayed_selection_reply_does_not_replace_a_newer_click_or_deselection() {
        let ctx = egui::Context::default();
        for latest in [BTreeSet::from([43]), BTreeSet::new()] {
            let mut app = app();
            app.sharing.baseline = Some(shared_content(&app.doc));
            app.sharing.desired = latest.clone();
            app.selected = latest.clone();
            let reply = remote(&app, "Untitled model", 1);
            app.accept_shared_reply(
                &ctx,
                reply,
                Kind::Sync {
                    sent: None,
                    selection: BTreeSet::from([42]),
                },
            )
            .unwrap();
            assert_eq!(app.sharing.confirmed, BTreeSet::from([42]));
            assert_eq!(app.selected, latest);
            assert!(app.sharing.selection_dirty);
            app.sharing_before_frame(&ctx);
            app.sharing_after_frame(&ctx);
            assert_eq!(app.sharing.desired, latest);
            assert!(!app.shared_edit_allowed(&ctx));
        }
    }
    #[test]
    fn authorized_creation_can_select_its_new_geometry_without_requiring_new_approval() {
        let ctx = egui::Context::default();
        for role in ["owner", "edit"] {
            let mut app = app();
            app.sharing.role = role.into();
            app.sharing.last_ack = 0.;
            app.sharing.baseline = Some(shared_content(&app.doc));
            app.sharing_before_frame(&ctx);
            let before = app.doc.model.clone();
            let edge = app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
            app.selected = BTreeSet::from([edge]);
            app.finish_edit("Add edge", before, &ctx);
            assert!(app.doc.model.edges.contains_key(&edge));
            app.sharing_after_frame(&ctx);
            assert!(app.doc.model.edges.contains_key(&edge));
            assert_eq!(app.sharing.desired, BTreeSet::from([edge]));
        }
    }
    #[test]
    fn read_only_and_unconfirmed_edits_are_rolled_back() {
        let ctx = egui::Context::default();
        let mut app = app();
        app.sharing.role = "view".into();
        app.sharing_before_frame(&ctx);
        app.doc.name = "Not permitted".into();
        app.sharing_after_frame(&ctx);
        assert_eq!(app.doc.name, "Untitled model");
        app.sharing.role = "edit".into();
        app.sharing.last_ack = f64::NEG_INFINITY;
        app.sharing_before_frame(&ctx);
        app.doc.name = "Not yet leased".into();
        app.sharing_after_frame(&ctx);
        assert_eq!(app.doc.name, "Untitled model");
    }
    #[test]
    fn approval_of_an_invitation_applies_the_project_without_a_false_conflict() {
        let ctx = egui::Context::default();
        let mut app = app();
        app.doc.shared_project.as_mut().unwrap().revision = 0;
        let remote = remote(&app, "Approved project", 7);
        app.accept_shared_reply(&ctx, remote, Kind::Read).unwrap();
        assert_eq!(app.doc.name, "Approved project");
        assert!(app.sharing.conflict.is_none());
    }
    #[test]
    fn shared_file_linkage_survives_roundtrip_and_old_files_default_to_unlinked() {
        let app = app();
        let json = serde_json::to_string(&app.doc).unwrap();
        let restored = Document::from_json(&json).unwrap();
        assert_eq!(restored.shared_project, app.doc.shared_project);
        let mut old = serde_json::to_value(&app.doc).unwrap();
        old.as_object_mut().unwrap().remove("shared_project");
        assert!(
            Document::from_json(&old.to_string())
                .unwrap()
                .shared_project
                .is_none()
        );
        // `model.connections` is a legitimate catalog, not a socket credential.
        assert!(!json.contains("\"token\":") && !json.contains("\"connection\":"));
    }
}

fn presence_live(lease: &Lease, remote: &Remote, sample_time: f64, now: f64) -> bool {
    now - sample_time < lease.expires_at.saturating_sub(remote.server_time).min(15) as f64
}
fn presence_anchor(model: &Model, entity: u64) -> Option<DVec3> {
    let expanded = model.expand_entities(&BTreeSet::from([entity]));
    let mut points = Vec::new();
    for id in expanded {
        if model.is_hidden(id) {
            continue;
        }
        if let Some(edge) = model.edges.get(&id) {
            let (a, b) = model.points(edge);
            points.extend([a, b]);
        }
        if let Some(face) = model.faces.get(&id) {
            points.extend(
                face.vertices
                    .iter()
                    .filter_map(|v| model.vertices.get(v).map(|v| v.position)),
            );
        }
        if let Some(dimension) = model.dimensions.get(&id) {
            points.extend(dimension.layout(model));
        }
        if let Some(text) = model.texts.get(&id) {
            points.extend(text.layout(model));
        }
    }
    if let Some(vertex) = model.vertices.get(&entity) {
        points.push(vertex.position);
    }
    let first = *points.first()?;
    let (lo, hi) = points
        .into_iter()
        .fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
    Some(DVec3::new((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5, hi.z))
}
impl KetchupApp {
    fn poll_presence_avatars(&mut self, ctx: &egui::Context, now: f64) {
        let live: BTreeSet<_> = self
            .sharing
            .remote
            .as_ref()
            .into_iter()
            .flat_map(|r| {
                r.locks
                    .iter()
                    .filter(|l| {
                        l.connection != self.sharing.connection
                            && presence_live(l, r, self.sharing.presence_sample_time, now)
                    })
                    .map(|l| l.id.clone())
            })
            .collect();
        self.sharing.avatars.retain(|id, _| live.contains(id));
        self.sharing.avatar_failed.retain(|id| live.contains(id));
        if let Some((id, receiver)) = &self.sharing.avatar_pending {
            let result = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Avatar request ended".into()))
                }
                Err(_) => None,
            };
            if let Some(result) = result {
                let id = id.clone();
                self.sharing.avatar_pending = None;
                let texture = result.and_then(|reply| {
                    if reply.status != 200 {
                        return Err("Avatar unavailable".into());
                    }
                    let bytes: Vec<u8> = serde_json::from_value(reply.body["bytes"].clone())
                        .map_err(|e| e.to_string())?;
                    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
                        .with_guessed_format()
                        .map_err(|e| e.to_string())?;
                    let mut limits = image::Limits::default();
                    limits.max_image_width = Some(4096);
                    limits.max_image_height = Some(4096);
                    reader.limits(limits);
                    let image = reader
                        .decode()
                        .map_err(|e| e.to_string())?
                        .thumbnail(64, 64)
                        .to_rgba8();
                    Ok(ctx.load_texture(
                        format!("presence-avatar-{id}"),
                        egui::ColorImage::from_rgba_unmultiplied(
                            [image.width() as usize, image.height() as usize],
                            image.as_raw(),
                        ),
                        Default::default(),
                    ))
                });
                if live.contains(&id) {
                    match texture {
                        Ok(texture) => {
                            self.sharing.avatars.insert(id, texture);
                        }
                        Err(_) => {
                            self.sharing.avatar_failed.insert(id);
                        }
                    }
                }
            }
        }
        if self.sharing.avatar_pending.is_none() {
            if let Some(remote) = &self.sharing.remote {
                if let Some(lease) = remote.locks.iter().find(|l| {
                    live.contains(&l.id)
                        && l.has_avatar
                        && !self.sharing.avatars.contains_key(&l.id)
                        && !self.sharing.avatar_failed.contains(&l.id)
                }) {
                    let receiver = self.account.project_request(
                        ctx,
                        &format!("/api/projects/{}/avatar/{}", remote.id, lease.id),
                        None,
                        self.sharing.guest.clone(),
                    );
                    self.sharing.avatar_pending = Some((lease.id.clone(), receiver));
                }
            }
        }
    }
    pub(super) fn paint_shared_presence(&self, ui: &egui::Ui, rect: Rect) {
        let Some(remote) = &self.sharing.remote else {
            return;
        };
        let now = ui.ctx().input(|i| i.time);
        let colors = Palette::for_ui(ui);
        let painter = ui.painter().with_clip_rect(rect);
        let mut occupied: Vec<Rect> = Vec::new();
        for lease in &remote.locks {
            if lease.connection == self.sharing.connection
                || !presence_live(lease, remote, self.sharing.presence_sample_time, now)
            {
                continue;
            }
            for entity in &lease.entities {
                let Some(anchor) = presence_anchor(&self.doc.model, *entity) else {
                    continue;
                };
                let Some(pixel) = self.doc.camera.project(anchor, dv(rect.size())) else {
                    continue;
                };
                let point = rect.min + pv(pixel);
                if !rect.contains(point) {
                    continue;
                }
                let name: String = lease.name.chars().take(40).collect();
                let label = format!(
                    "Being edited by {name}{}",
                    if lease.kind == "guest" {
                        " · Guest"
                    } else {
                        ""
                    }
                );
                let galley = painter.layout_no_wrap(label, FontId::proportional(12.), colors.ink);
                let size = egui::vec2((galley.size().x + 46.).min(rect.width() - 8.).max(40.), 32.);
                let mut origin = point + egui::vec2(12., -44.);
                origin.x = origin.x.clamp(
                    rect.left() + 4.,
                    (rect.right() - size.x - 4.).max(rect.left() + 4.),
                );
                origin.y = origin.y.clamp(
                    rect.top() + 4.,
                    (rect.bottom() - size.y - 4.).max(rect.top() + 4.),
                );
                let mut card = Rect::from_min_size(origin, size);
                for other in &occupied {
                    if card.intersects(*other) {
                        card = card.translate(egui::vec2(0., 34.));
                    }
                }
                if !rect.contains_rect(card) {
                    continue;
                }
                occupied.push(card);
                painter.line_segment([point, card.left_center()], Stroke::new(1., colors.muted));
                painter.rect_filled(card, 8., colors.paper);
                painter.rect_stroke(
                    card,
                    8.,
                    Stroke::new(1., colors.border),
                    egui::StrokeKind::Inside,
                );
                let avatar =
                    Rect::from_min_size(card.min + egui::vec2(4., 4.), egui::vec2(24., 24.));
                if let Some(texture) = self.sharing.avatars.get(&lease.id) {
                    egui::Image::new((texture.id(), avatar.size()))
                        .corner_radius(12)
                        .paint_at(ui, avatar);
                } else {
                    painter.circle_filled(avatar.center(), 12., colors.muted);
                    let initials: String = lease
                        .name
                        .split_whitespace()
                        .take(2)
                        .filter_map(|s| s.chars().next())
                        .collect::<String>()
                        .to_uppercase();
                    painter.text(
                        avatar.center(),
                        Align2::CENTER_CENTER,
                        if initials.is_empty() { "?" } else { &initials },
                        FontId::proportional(10.),
                        colors.paper,
                    );
                }
                painter.with_clip_rect(card.shrink(2.)).galley(
                    card.min + egui::vec2(34., 8.),
                    galley,
                    colors.ink,
                );
            }
        }
    }
}

#[cfg(test)]
mod presence_tests {
    use super::*;
    #[test]
    fn lease_display_uses_monotonic_elapsed_time_and_server_expiry() {
        let lease = Lease {
            id: "x".into(),
            kind: "guest".into(),
            expires_at: 115,
            has_avatar: false,
            connection: "other".into(),
            name: "Guest".into(),
            entities: vec![],
        };
        let remote = Remote {
            id: "a".repeat(48),
            role: "view".into(),
            revision: 1,
            updated_at: 100,
            server_time: 100,
            document: None,
            requests: vec![],
            locks: vec![],
            actor: None,
        };
        assert!(presence_live(&lease, &remote, 10., 24.9));
        assert!(!presence_live(&lease, &remote, 10., 25.));
        assert!(!presence_live(&lease, &remote, f64::NEG_INFINITY, 10.));
    }
    #[test]
    fn presence_anchor_follows_object_geometry_and_missing_entities_disappear() {
        let mut model = Model::default();
        let edge = model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
        assert_eq!(presence_anchor(&model, edge), Some(DVec3::new(0.5, 0., 0.)));
        for vertex in model.vertices.values_mut() {
            vertex.position += DVec3::Y;
        }
        assert_eq!(presence_anchor(&model, edge), Some(DVec3::new(0.5, 1., 0.)));
        assert!(presence_anchor(&model, 999).is_none());
    }
    #[test]
    fn one_share_entry_carries_pending_access_notifications() {
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        assert_eq!(app.share_menu_label(), "Share…");
        app.sharing.notifications.push(AccessRequest {
            id: "request".into(),
            name: "Guest".into(),
            role: "view".into(),
            status: "pending".into(),
            project: "a".repeat(48),
        });
        assert_eq!(app.share_menu_label(), "Share… (1)");
    }
}

impl KetchupApp {
    fn close_realtime(&mut self, reason: &str) {
        self.sharing.socket = None;
        self.sharing.socket_ready = false;
        self.sharing.socket_actor = None;
        self.sharing.ticket_pending = None;
        for (_, sender) in std::mem::take(&mut self.sharing.socket_requests) {
            let _ = sender.send(Err(reason.into()));
        }
        self.sharing.socket_outbox.clear();
        self.sharing.deferred_state = None;
        self.sharing.selection_dirty = true;
        self.sharing.confirmed.clear();
        self.sharing.last_ack = f64::NEG_INFINITY;
        self.sharing.presence_sample_time = f64::NEG_INFINITY;
    }
    fn apply_socket_state(&mut self, ctx: &egui::Context, body: Value, now: f64) {
        let Ok(remote) = serde_json::from_value::<Remote>(body) else {
            self.sharing.error = Some("Invalid WebSocket project state".into());
            return;
        };
        if self.sharing.active.as_ref() != Some(&remote.id) {
            return;
        }
        if self
            .doc
            .shared_project
            .as_ref()
            .is_some_and(|l| remote.revision < l.revision)
        {
            return;
        }
        let started = self.sharing.request_started;
        self.sharing.request_started = now;
        if let Err(error) = self.accept_shared_reply(ctx, remote, Kind::Read) {
            self.sharing.error = Some(error);
        }
        self.sharing.request_started = started;
        ctx.request_repaint();
    }
    fn pump_realtime(&mut self, ctx: &egui::Context, now: f64) {
        if let Some(receiver) = &self.sharing.ticket_pending {
            let result = match receiver.try_recv() {
                Ok(r) => Some(r),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("WebSocket ticket request interrupted".into()))
                }
                Err(_) => None,
            };
            if let Some(result) = result {
                self.sharing.ticket_pending = None;
                match result {
                    Ok(reply) if reply.status == 200 => {
                        let url = reply.body["url"].as_str().unwrap_or("");
                        let ticket = reply.body["ticket"].as_str().unwrap_or("");
                        if !crate::realtime::valid_endpoint(&self.account.server, url)
                            || ticket.is_empty()
                        {
                            self.sharing.error = Some("Invalid WebSocket endpoint".into());
                            self.sharing.socket_retry_at = now + 5.;
                        } else {
                            self.sharing.socket_last_message = now;
                            self.sharing.socket = Some(crate::realtime::Socket::connect(
                                ctx,
                                url.into(),
                                ticket.into(),
                                self.sharing.connection.clone(),
                            ));
                        }
                    }
                    Ok(reply) => {
                        if reply.status == 401 || reply.status == 403 {
                            self.account.refresh(ctx);
                        }
                        self.sharing.error = Some(
                            reply.body["error"]
                                .as_str()
                                .unwrap_or("WebSocket unavailable")
                                .into(),
                        );
                        self.sharing.socket_retry_at = now + 5.;
                    }
                    Err(error) => {
                        self.sharing.error = Some(error);
                        self.sharing.socket_retry_at = now + 5.;
                    }
                }
            }
        }
        let mut events = Vec::new();
        if let Some(socket) = &self.sharing.socket {
            loop {
                match socket.events.try_recv() {
                    Ok(event) => events.push(event),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(_) => {
                        events.push(Err("WebSocket disconnected".into()));
                        break;
                    }
                }
            }
        }
        for event in events {
            let value = match event {
                Ok(v) => v,
                Err(error) => {
                    self.account.server_reachable = false;
                    self.close_realtime(&error);
                    self.sharing.error = Some(error);
                    self.sharing.socket_retry_at = now + 1.;
                    self.sharing.force_read = true;
                    self.sharing.connection = ketchup_core::MaterialId::new_v4().to_string();
                    break;
                }
            };
            self.sharing.socket_last_message = now;
            match value["type"].as_str().unwrap_or("") {
                "ready" => {
                    self.sharing.socket_actor = serde_json::from_value(value["actor"].clone()).ok();
                    self.sharing.socket_ready = true;
                    self.sharing.error = None;
                    self.sharing.heartbeat_at = now + 5.;
                    self.sharing.force_read = true;
                }
                "response" => {
                    if let Some(seq) = value["seq"].as_u64() {
                        if let Some(sender) = self.sharing.socket_requests.remove(&seq) {
                            let _ = sender.send(Ok(ApiReply {
                                status: value["status"].as_u64().unwrap_or(500) as u16,
                                body: value["body"].clone(),
                            }));
                        }
                    }
                }
                "state" => {
                    self.sharing.deferred_state = Some(value["body"].clone());
                }
                "notifications" => {
                    if let Ok(notices) = serde_json::from_value::<Vec<AccessRequest>>(
                        value["body"]["notifications"].clone(),
                    ) {
                        if notices.iter().any(|r| {
                            !self
                                .sharing
                                .notifications
                                .iter()
                                .any(|old| old.id == r.id && old.status == r.status)
                        }) {
                            self.say(ctx, "New project access notification — File → Share");
                        }
                        self.sharing.notifications = notices;
                    }
                }
                "heartbeat" => {
                    self.sharing.last_ack = self.sharing.heartbeat_sent;
                }
                _ => {}
            }
        }
        if self.sharing.socket.is_some() && now - self.sharing.socket_last_message > 15. {
            self.account.server_reachable = false;
            self.close_realtime("WebSocket connection timed out");
            self.sharing.error = Some("WebSocket connection timed out".into());
            self.sharing.connection = ketchup_core::MaterialId::new_v4().to_string();
            self.sharing.force_read = true;
            self.sharing.socket_retry_at = now + 1.;
        }
        if self.sharing.socket.is_some() && !self.sharing.socket_ready {
            ctx.request_repaint_after(Duration::from_secs_f64(
                (15. - (now - self.sharing.socket_last_message)).max(0.01),
            ));
        }
        if self.sharing.socket_ready {
            if let Some(socket) = &self.sharing.socket {
                for command in std::mem::take(&mut self.sharing.socket_outbox) {
                    if let Err(error) = socket.send(command) {
                        self.sharing.error = Some(error);
                    }
                }
                if now >= self.sharing.heartbeat_at {
                    let _ = socket.send(json!({"type":"heartbeat"}));
                    self.sharing.heartbeat_sent = now;
                    self.sharing.heartbeat_at = now + 5.;
                }
            }
            ctx.request_repaint_after(Duration::from_secs_f64(
                (self.sharing.heartbeat_at - now).max(0.01),
            ));
        } else if self.sharing.socket.is_none()
            && self.sharing.ticket_pending.is_none()
            && (self.account.user.is_some() || self.sharing.guest.is_some())
        {
            if now >= self.sharing.socket_retry_at {
                self.sharing.ticket_pending = Some(self.account.project_request(
                    ctx,
                    "/api/socket-ticket",
                    Some(json!({})),
                    self.sharing.guest.clone(),
                ));
            } else {
                ctx.request_repaint_after(Duration::from_secs_f64(
                    (self.sharing.socket_retry_at - now).max(0.01),
                ));
            }
        }
    }
}

#[cfg(test)]
mod websocket_state_tests {
    use super::*;
    #[test]
    fn switching_projects_cancels_unsent_socket_commands() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let (sender, receiver) = std::sync::mpsc::channel();
        app.sharing.socket_requests.insert(1, sender);
        app.sharing
            .socket_outbox
            .push(json!({"path":"/api/projects/old/sync"}));
        app.sharing.deferred_state = Some(json!({"id":"old"}));
        app.release_shared_selection(&ctx);
        assert!(app.sharing.socket_outbox.is_empty());
        assert!(app.sharing.socket_requests.is_empty());
        assert!(receiver.try_recv().unwrap().is_err());
        assert!(app.sharing.deferred_state.is_none());
    }
    #[test]
    fn disconnect_invalidates_presence_and_pending_replies() {
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.sharing.socket_ready = true;
        app.sharing.last_ack = 123.;
        app.sharing.presence_sample_time = 123.;
        app.sharing.deferred_state = Some(json!({"role":"owner"}));
        app.close_realtime("Disconnected");
        assert!(!app.sharing.socket_ready);
        assert!(app.sharing.selection_dirty);
        assert_eq!(app.sharing.last_ack, f64::NEG_INFINITY);
        assert_eq!(app.sharing.presence_sample_time, f64::NEG_INFINITY);
        assert!(app.sharing.deferred_state.is_none());
    }
    #[test]
    fn remote_changes_preserve_an_unaffected_confirmed_selection() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let edge = app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
        app.sharing.confirmed.insert(edge);
        app.sharing.desired.insert(edge);
        app.selected.insert(edge);
        app.sharing.selection_dirty = false;
        let mut remote_doc = app.doc.clone();
        remote_doc.name = "Remote name".into();
        let remote = Remote {
            id: "a".repeat(48),
            role: "edit".into(),
            revision: 2,
            updated_at: 100,
            server_time: 100,
            document: Some(serde_json::to_value(remote_doc).unwrap()),
            requests: vec![],
            locks: vec![],
            actor: None,
        };
        app.apply_shared_remote(&ctx, &remote).unwrap();
        assert_eq!(app.selected, BTreeSet::from([edge]));
        assert_eq!(app.sharing.desired, BTreeSet::from([edge]));
        assert!(!app.sharing.selection_dirty);
    }
}

#[cfg(test)]
mod access_status_tests {
    use super::*;
    fn app() -> KetchupApp {
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        // Simulate an available server without contacting a personal server.
        // A refresh can only attempt the unused loopback port.
        app.account.server = "http://127.0.0.1:0".into();
        app.account.server_reachable = true;
        app.doc.shared_project = Some(ketchup_core::SharedProject {
            server: app.account.server.clone(),
            id: "a".repeat(48),
            revision: 1,
            server_updated_at: 100,
            file_updated_at: 100,
        });
        app.sharing.active = Some("a".repeat(48));
        app
    }
    fn response(app: &KetchupApp, role: &str) -> Remote {
        Remote {
            id: "a".repeat(48),
            role: role.into(),
            revision: 1,
            updated_at: 100,
            server_time: 100,
            document: Some(serde_json::to_value(&app.doc).unwrap()),
            requests: vec![],
            locks: vec![],
            actor: None,
        }
    }
    #[test]
    fn decoding_failure_does_not_misrepresent_confirmed_owner_access() {
        let ctx = egui::Context::default();
        let mut app = app();
        assert_eq!(
            app.sharing_access_label(),
            "Waiting for server confirmation"
        );
        app.sharing.baseline = Some(shared_content(&app.doc));
        app.sharing.last_ack = 0.;
        let mut remote = response(&app, "owner");
        remote.document.as_mut().unwrap()["model"]["edges"] = json!("invalid map");
        let error = app
            .accept_shared_reply(&ctx, remote, Kind::Read)
            .unwrap_err();
        assert!(error.contains("server confirmed owner access"));
        assert_eq!(app.sharing_access_label(), "owner");
        assert!(app.sharing.access_confirmed);
        assert!(app.sharing.baseline.is_none());
        // A heartbeat cannot turn an invalid/unloaded baseline into an editing grant.
        app.sharing.last_ack = 0.;
        app.sharing.socket_ready = true;
        assert!(!app.shared_edit_allowed(&ctx));
        assert!(
            app.shared_edit_block_reason(&ctx)
                .contains("project data is unavailable")
        );
        assert_eq!(app.sharing.remote.as_ref().unwrap().role, "owner");
    }
    #[test]
    fn recorded_project_author_never_grants_access_to_a_guest() {
        let ctx = egui::Context::default();
        let mut app = app();
        app.doc.project_author = Some(ketchup_core::HistoryAuthor {
            id: "user:owner".into(),
            name: "Original owner".into(),
            kind: "account".into(),
            verified: true,
        });
        let mut remote = response(&app, "none");
        remote.document = None;
        remote.actor = Some(ketchup_core::HistoryAuthor {
            id: "guest:visitor".into(),
            name: "Guest visitor".into(),
            kind: "guest".into(),
            verified: false,
        });
        app.accept_shared_reply(&ctx, remote, Kind::Read).unwrap();
        assert_eq!(app.sharing_access_label(), "none");
        assert!(!app.shared_edit_allowed(&ctx));
        assert!(
            app.sharing_identity_status()
                .contains("Guest visitor (Guest)")
        );
        assert_eq!(app.doc.project_author.as_ref().unwrap().id, "user:owner");
    }
    #[test]
    fn a_missing_document_never_reuses_a_previously_editable_baseline() {
        let ctx = egui::Context::default();
        let mut app = app();
        app.sharing.baseline = Some(shared_content(&app.doc));
        app.sharing.last_ack = 0.;
        let mut remote = response(&app, "owner");
        remote.document = None;
        assert!(app.accept_shared_reply(&ctx, remote, Kind::Read).is_err());
        assert_eq!(app.sharing_access_label(), "owner");
        assert!(app.sharing.baseline.is_none());
        assert!(!app.shared_edit_allowed(&ctx));
    }
    #[test]
    fn refresh_preserves_local_work_and_selection_but_requires_new_authorization() {
        let ctx = egui::Context::default();
        let mut app = app();
        app.doc.name = "Unsaved local work".into();
        app.selected = BTreeSet::from([42]);
        app.sharing.desired = app.selected.clone();
        app.sharing.role = "owner".into();
        app.sharing.access_confirmed = true;
        app.sharing.baseline = Some(shared_content(&app.doc));
        app.sharing.last_ack = 0.;
        let before = app.doc.clone();
        app.handle_share_action(&ctx, "refresh", "");
        assert_eq!(app.doc, before);
        assert_eq!(app.selected, BTreeSet::from([42]));
        assert_eq!(app.sharing.desired, BTreeSet::from([42]));
        assert!(app.sharing.force_read);
        assert_eq!(
            app.sharing_access_label(),
            "Waiting for server confirmation"
        );
        assert!(!app.shared_edit_allowed(&ctx));
    }
}
