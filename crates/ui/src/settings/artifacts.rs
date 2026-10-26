//! Settings → Artifacts: browse and preview files in the selected workspace.
//!
//! Artifacts stay in their workspace. This page uses the existing bounded
//! workspace-directory and file-read RPCs; it does not scan arbitrary local
//! paths or copy workspace contents into settings.
use gpui::{Context, Entity, Render, SharedString, Task, Window, div, prelude::*, px};

use zeron_proto::{
    WorkspaceDirectoryPage, WorkspaceEntry, WorkspaceEntryKind, WorkspaceFileText,
    WorkspaceTarget, WorkspaceTextEncoding,
};
use zeron_rpc::methods;

use crate::state::AppState;
use crate::settings::widgets::{self, ActionTone};
use crate::theme::Theme;

pub struct ArtifactsPage {
    state: Entity<AppState>,
    target: Option<WorkspaceTarget>,
    target_device_id: Option<String>,
    target_label: Option<String>,
    /// Identity of the workspace currently represented by the listing. Prevents stale
    /// paths/previews from leaking across chat/project switches while Settings stays open.
    target_key: Option<String>,
    directory: String,
    entries: Vec<WorkspaceEntry>,
    preview_path: Option<String>,
    preview_text: Option<String>,
    preview_notice: Option<String>,
    loading: bool,
    /// Monotonic request token so slow responses cannot overwrite a newer workspace or preview.
    request_generation: u64,
    error: Option<String>,
    scroll: widgets::PageScroll,
    task: Option<Task<()>>,
}

impl ArtifactsPage {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let (target, target_device_id, target_label) = {
            let state_ref = state.read(cx);
            let local_device_id = state_ref.local_device_id.clone();
            if let Some(chat_id) = state_ref.selected_chat.clone() {
                let selected_chat = state_ref.selected_chat_row();
                let device = selected_chat.map(|chat| chat.device_id.clone());
                let label = selected_chat
                    .map(|chat| format!("Chat: {}", chat.title))
                    .unwrap_or_else(|| format!("Chat: {chat_id}"));
                let target_device = device.filter(|id| local_device_id.as_deref().is_some_and(|local| local != id.as_str()));
                (Some(WorkspaceTarget {
                    chat_id: Some(chat_id),
                    space_id: None,
                    checkout_path: None,
                }), target_device, Some(label))
            } else if let Some(space) = state_ref.selected_space_row() {
                let target_device = local_device_id
                    .as_deref()
                    .is_some_and(|local| local != space.device_id.as_str())
                    .then(|| space.device_id.clone());
                (Some(WorkspaceTarget {
                    chat_id: None,
                    space_id: Some(space.id.clone()),
                    checkout_path: None,
                }), target_device, Some(format!("Project: {}", space.path)))
            } else {
                (None, None, None)
            }
        };
        let mut page = Self {
            state,
            target,
            target_device_id,
            target_label,
            target_key: None,
            directory: String::new(),
            entries: Vec::new(),
            preview_path: None,
            preview_text: None,
            preview_notice: None,
            loading: false,
            request_generation: 0,
            error: None,
            scroll: widgets::PageScroll::default(),
            task: None,
        };
        page.refresh(cx);
        page
    }

    fn request_target(target: &WorkspaceTarget) -> serde_json::Value {
        serde_json::to_value(target).unwrap_or_else(|_| serde_json::json!({}))
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.request_generation = self.request_generation.wrapping_add(1);
        let generation = self.request_generation;
        let Some(engine) = self.state.read(cx).engine().cloned() else {
            self.entries.clear();
            self.target_label = None;
            self.preview_path = None;
            self.preview_text = None;
            self.preview_notice = None;
            self.error = Some("Engine not connected.".into());
            self.loading = false;
            cx.notify();
            return;
        };
        // Settings can stay open while the selected chat/project changes.
        // Re-resolve the target on every refresh so the page follows that choice.
        let (target, target_device_id, target_label) = {
            let state = self.state.read(cx);
            let local_device_id = state.local_device_id.clone();
            if let Some(chat_id) = state.selected_chat.clone() {
                let selected_chat = state.selected_chat_row();
                let device = selected_chat.map(|chat| chat.device_id.clone());
                let label = selected_chat
                    .map(|chat| format!("Chat: {}", chat.title))
                    .unwrap_or_else(|| format!("Chat: {chat_id}"));
                let target_device = device.filter(|id| local_device_id.as_deref() != Some(id.as_str()));
                (Some(WorkspaceTarget {
                    chat_id: Some(chat_id),
                    space_id: None,
                    checkout_path: None,
                }), target_device, Some(label))
            } else if let Some(space) = state.selected_space_row() {
                let target_device = local_device_id
                    .as_deref()
                    .is_some_and(|local| local != space.device_id.as_str())
                    .then(|| space.device_id.clone());
                (Some(WorkspaceTarget {
                    chat_id: None,
                    space_id: Some(space.id.clone()),
                    checkout_path: None,
                }), target_device, Some(format!("Project: {}", space.path)))
            } else {
                (None, None, None)
            }
        };
        self.target = target;
        self.target_device_id = target_device_id;
        self.target_label = target_label;
        let Some(target) = self.target.clone() else {
            // Never leave the last workspace visible after its selection disappears.
            self.entries.clear();
            self.directory.clear();
            self.preview_path = None;
            self.preview_text = None;
            self.preview_notice = None;
            self.target_key = None;
            self.target_label = None;
            self.error = Some("Select a project or chat in the main window first. Artifacts are shown from that workspace; this page does not search your whole computer.".into());
            self.loading = false;
            cx.notify();
            return;
        };
        let target_key = format!(
            "{:?}|{:?}|{:?}|{:?}",
            target.chat_id, target.space_id, target.checkout_path, self.target_device_id
        );
        if self.target_key.as_deref() != Some(target_key.as_str()) {
            self.target_key = Some(target_key);
            self.directory.clear();
            self.entries.clear();
            self.preview_path = None;
            self.preview_text = None;
            self.preview_notice = None;
        }
        let directory = self.directory.clone();
        let mut params = Self::request_target(&target);
        params["directory"] = serde_json::Value::String(directory.clone());
        params["includeIgnored"] = serde_json::Value::Bool(false);
        if let Some(target) = &self.target_device_id {
            params["targetDeviceId"] = serde_json::Value::String(target.clone());
        }
        self.loading = true;
        self.error = None;
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = engine
                .client()
                .call(methods::LIST_WORKSPACE_DIRECTORY, params)
                .await
                .map_err(|error| error.to_string())
                .and_then(|value| serde_json::from_value::<WorkspaceDirectoryPage>(value).map_err(|error| error.to_string()));
            this.update(cx, |page, cx| {
                if page.request_generation != generation {
                    return;
                }
                page.loading = false;
                match result {
                    Ok(listing) => {
                        page.entries = listing.entries;
                        page.error = None;
                    }
                    Err(error) => {
                        page.entries.clear();
                        page.error = Some(error);
                    }
                }
                cx.notify();
            }).ok();
        }));
        cx.notify();
    }

    fn open_directory(&mut self, path: String, cx: &mut Context<Self>) {
        self.directory = path;
        self.preview_path = None;
        self.preview_text = None;
        self.preview_notice = None;
        self.refresh(cx);
    }

    fn go_parent(&mut self, cx: &mut Context<Self>) {
        self.directory = self.directory
            .rsplit_once('/')
            .map(|(parent, _)| parent.to_owned())
            .unwrap_or_default();
        self.preview_path = None;
        self.preview_text = None;
        self.preview_notice = None;
        self.refresh(cx);
    }

    fn open_file(&mut self, path: String, cx: &mut Context<Self>) {
        self.request_generation = self.request_generation.wrapping_add(1);
        let generation = self.request_generation;
        let Some(engine) = self.state.read(cx).engine().cloned() else {
            self.preview_notice = Some("Engine not connected.".into());
            cx.notify();
            return;
        };
        let Some(target) = self.target.clone() else {
            self.preview_notice = Some("No workspace selected.".into());
            cx.notify();
            return;
        };
        self.preview_path = Some(path.clone());
        self.preview_text = None;
        self.preview_notice = Some("Loading preview…".into());
        let mut params = Self::request_target(&target);
        params["path"] = serde_json::Value::String(path.clone());
        if let Some(target) = &self.target_device_id {
            params["targetDeviceId"] = serde_json::Value::String(target.clone());
        }
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = engine
                .client()
                .call(methods::READ_WORKSPACE_FILE, params)
                .await
                .map_err(|error| error.to_string())
                .and_then(|value| serde_json::from_value::<WorkspaceFileText>(value).map_err(|error| error.to_string()));
            this.update(cx, |page, cx| {
                if page.request_generation != generation {
                    return;
                }
                match result {
                    Ok(file) => {
                        page.preview_path = Some(file.path.clone());
                        match (file.encoding, file.text) {
                            (WorkspaceTextEncoding::Utf8 | WorkspaceTextEncoding::Utf8Bom, Some(text)) => {
                                page.preview_text = Some(text);
                                page.preview_notice = Some(if file.truncated {
                                    "Preview truncated by the workspace file reader.".into()
                                } else {
                                    format!("{} bytes", file.size)
                                });
                            }
                            _ => {
                                page.preview_text = None;
                                page.preview_notice = Some("This file is binary or uses an encoding that cannot be previewed as text.".into());
                            }
                        }
                    }
                    Err(error) => {
                        page.preview_text = None;
                        page.preview_notice = Some(format!("Couldn't preview file: {error}"));
                    }
                }
                cx.notify();
            }).ok();
        }));
        cx.notify();
    }
}

impl Render for ArtifactsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>().clone();
        let refresh = widgets::action_button(&theme, ActionTone::Quiet)
            .id("artifacts-refresh")
            .tab_index(0)
            .role(gpui::Role::Button)
            .aria_label("Refresh workspace artifacts")
            .on_click(cx.listener(|page, _, _, cx| page.refresh(cx)))
            .child(if self.loading { "Loading…" } else { "Refresh" });
        let parent_button = widgets::action_button(&theme, ActionTone::Quiet)
            .id("artifacts-parent")
            .tab_index(0)
            .role(gpui::Role::Button)
            .on_click(cx.listener(|page, _, _, cx| page.go_parent(cx)))
            .child("← Parent");
        let path_label = if self.directory.is_empty() { "/".to_owned() } else { format!("/{}", self.directory) };
        let target_label = self.target_label.clone().unwrap_or_else(|| "No workspace selected".to_owned());

        let mut entry_rows = Vec::new();
        for entry in &self.entries {
            let path = entry.path.clone();
            let kind = entry.kind;
            let name = entry.name.clone();
            let detail = match kind {
                WorkspaceEntryKind::Directory => "Folder".to_owned(),
                WorkspaceEntryKind::Symlink => "Link".to_owned(),
                WorkspaceEntryKind::File => entry.size.map(|size| format!("{size} bytes")).unwrap_or_else(|| "File".to_owned()),
            };
            let icon_label = match kind {
                WorkspaceEntryKind::Directory => "▸",
                WorkspaceEntryKind::Symlink => "↗",
                WorkspaceEntryKind::File => "·",
            };
            entry_rows.push(
                widgets::card_row(&theme, entry_rows.is_empty())
                    .cursor_pointer()
                    .on_click(cx.listener(move |page, _, _, cx| {
                        match kind {
                            WorkspaceEntryKind::Directory => page.open_directory(path.clone(), cx),
                            WorkspaceEntryKind::File => page.open_file(path.clone(), cx),
                            WorkspaceEntryKind::Symlink => {
                                page.preview_notice = Some("Symbolic links are shown for reference but are not followed by this browser.".into());
                                cx.notify();
                            }
                        }
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(widgets::row_title(&theme, format!("{icon_label}  {name}")))
                            .child(widgets::meta_line(&theme, vec![div().child(detail).into_any_element()])),
                    )
                    .into_any_element(),
            );
        }

        let preview_panel = self.preview_path.as_ref().map(|path| {
            let preview = self.preview_text.clone().unwrap_or_else(|| {
                self.preview_notice.clone().unwrap_or_else(|| "No preview available.".into())
            });
            widgets::section(
                &theme,
                "Preview",
                widgets::section_card(&theme)
                    .mt_0()
                    .child(widgets::row_title(&theme, path.clone()))
                    .child(
                        div()
                            .mt(px(8.0))
                            .w_full()
                            .max_h(px(360.0))
                            .overflow_y_scroll()
                            .rounded(px(8.0))
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.input_glass_bg())
                            .p(px(10.0))
                            .text_size(crate::typography::ui_rems(12.0))
                            .child(SharedString::from(preview)),
                    ),
            ).into_any_element()
        });

        div()
            .id("artifacts-settings-page-host")
            .relative()
            .size_full()
            .child(
                crate::edge_fade::edge_faded(
                    16.0,
                    true,
                    true,
                    div()
                        .id("artifacts-settings-page")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll.scroll)
                        .child(
                            widgets::page_column()
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .justify_between()
                                        .child(widgets::page_header(&theme, "Artifacts", None))
                                        .child(refresh),
                                )
                                .child(widgets::section(
                                    &theme,
                                    "Workspace files",
                                    widgets::section_card(&theme)
                                        .mt_0()
                                        .child(
                                            div()
                                                .text_color(theme.text_muted)
                                                .text_size(crate::typography::ui_rems(12.0))
                                                .child("Browse and preview outputs in the currently selected chat or project. Files remain in the workspace; opening this page does not copy or modify them."),
                                        )
                                        .child(
                                            div()
                                                .mt(px(8.0))
                                                .text_color(theme.text_muted)
                                                .text_size(crate::typography::ui_rems(12.0))
                                                .child(SharedString::from(target_label)),
                                        )
                                        .child(
                                            div()
                                                .mt(px(10.0))
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap(px(8.0))
                                                .child(parent_button)
                                                .child(div().text_color(theme.text).child(path_label)),
                                        )
                                        .children(entry_rows)
                                        .children(self.error.as_ref().map(|error| {
                                            div().mt(px(8.0)).text_color(theme.danger_muted).child(SharedString::from(error.clone()))
                                        }))
                                        .children(if self.entries.is_empty() && !self.loading && self.error.is_none() {
                                            Some(div().mt(px(8.0)).text_color(theme.text_muted).child("No files in this directory."))
                                        } else { None }),
                                ))
                                .children(preview_panel)
                                .children(self.preview_notice.as_ref().filter(|_| self.preview_path.is_none()).map(|notice| {
                                    div().text_color(theme.text_muted).child(SharedString::from(notice.clone()))
                                })),
                        ),
                )
                .fade_overflow_y(&self.scroll.scroll),
            )
    }
}
