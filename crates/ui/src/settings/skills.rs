//! Settings → Skills: live inventory of skills advertised by the installed agents.
//!
//! The engine owns discovery. This page queries its existing ListSkills RPC,
//! so the inventory reflects the selected chat/project instead of a fabricated
//! list or a second independent filesystem scanner.
use gpui::{Context, Entity, Render, SharedString, Task, Window, div, prelude::*, px};

use zeron_proto::{HarnessId, invocation::Skill};
use zeron_rpc::methods;

use crate::state::AppState;
use crate::settings::widgets::{self, ActionTone};
use crate::theme::Theme;

const SKILL_HARNESSES: [(HarnessId, &str); 9] = [
    (HarnessId::Antigravity, "Antigravity"),
    (HarnessId::ClaudeCode, "Claude Code"),
    (HarnessId::Codex, "Codex"),
    (HarnessId::Cursor, "Cursor"),
    (HarnessId::Devin, "Devin"),
    (HarnessId::Grok, "Grok"),
    (HarnessId::Hermes, "Hermes"),
    (HarnessId::Pi, "Pi"),
    (HarnessId::Opencode, "OpenCode"),
];

pub struct SkillsPage {
    state: Entity<AppState>,
    skills: Vec<(HarnessId, Skill)>,
    loading: bool,
    loaded_harnesses: usize,
    error: Option<String>,
    warning: Option<String>,
    scroll: widgets::PageScroll,
    task: Option<Task<()>>,
}

impl SkillsPage {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let mut page = Self {
            state,
            skills: Vec::new(),
            loading: false,
            loaded_harnesses: 0,
            error: None,
            warning: None,
            scroll: widgets::PageScroll::default(),
            task: None,
        };
        page.refresh(cx);
        page
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.state.read(cx).engine().cloned() else {
            self.warning = None;
            self.error = Some("Engine not connected.".into());
            self.loading = false;
            cx.notify();
            return;
        };
        let (chat_id, space_id, target_device_id) = {
            let state = self.state.read(cx);
            let local_device_id = state.local_device_id.clone();
            if let Some(chat_id) = state.selected_chat.clone() {
                let device = state.selected_chat_row().map(|chat| chat.device_id.clone());
                let target = device.filter(|id| local_device_id.as_deref().is_some_and(|local| local != id.as_str()));
                (Some(chat_id), None, target)
            } else if let Some(space) = state.selected_space_row() {
                let target = (local_device_id.as_deref() != Some(space.device_id.as_str()))
                    .then(|| space.device_id.clone());
                (None, Some(space.id.clone()), target)
            } else {
                (None, None, None)
            }
        };
        self.loading = true;
        self.error = None;
        self.warning = None;
        self.task = Some(cx.spawn(async move |this, cx| {
            let mut all = Vec::new();
            let mut successes = 0usize;
            let mut failures = Vec::new();
            // Discover the supported catalogs concurrently so a slow or unavailable
            // provider does not serialize the entire inventory refresh.
            let results = futures::future::join_all(
                SKILL_HARNESSES.into_iter().map(|(harness, label)| {
                    let engine = engine.clone();
                    let chat_id = chat_id.clone();
                    let space_id = space_id.clone();
                    let target_device_id = target_device_id.clone();
                    async move {
                        let mut params = serde_json::json!({ "harness": harness });
                        if let Some(chat_id) = chat_id {
                            params["chatId"] = serde_json::Value::String(chat_id);
                        } else if let Some(space_id) = space_id {
                            params["spaceId"] = serde_json::Value::String(space_id);
                        }
                        if let Some(target) = target_device_id {
                            params["targetDeviceId"] = serde_json::Value::String(target);
                        }
                        let result = engine.client().call(methods::LIST_SKILLS, params).await;
                        (harness, label, result)
                    }
                }),
            )
            .await;
            for (harness, label, result) in results {
                match result {
                    Ok(value) => match serde_json::from_value::<Vec<Skill>>(value) {
                        Ok(skills) => {
                            successes += 1;
                            all.extend(skills.into_iter().map(|skill| (harness, skill)));
                        }
                        Err(error) => failures.push(format!("{label}: {error}")),
                    },
                    Err(error) => failures.push(format!("{label}: {error}")),
                }
            }
            let warning = if successes > 0 && !failures.is_empty() {
                let details = failures.iter().take(3).cloned().collect::<Vec<_>>().join(" · ");
                let remaining = failures.len().saturating_sub(3);
                Some(if remaining > 0 {
                    format!("Some agent catalogs could not be read: {details} (+{remaining} more).")
                } else {
                    format!("Some agent catalogs could not be read: {details}.")
                })
            } else {
                None
            };
            let error_message = if successes == 0 {
                Some(format!(
                    "Couldn't load skills from any agent. {}",
                    failures.into_iter().take(3).collect::<Vec<_>>().join(" ")
                ))
            } else {
                None
            };
            this.update(cx, move |page, cx| {
                page.skills = all;
                page.loaded_harnesses = successes;
                page.loading = false;
                page.warning = warning;
                page.error = error_message;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }
}

impl Render for SkillsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>().clone();
        let refresh = widgets::action_button(&theme, ActionTone::Quiet)
            .id("skills-refresh")
            .tab_index(0)
            .role(gpui::Role::Button)
            .aria_label("Refresh discovered skills")
            .on_click(cx.listener(|page, _, _, cx| page.refresh(cx)))
            .child(if self.loading { "Refreshing…" } else { "Refresh" });

        let mut sections = Vec::new();
        for (harness, label) in SKILL_HARNESSES {
            let mut skills = self.skills.iter().filter(|(id, _)| *id == harness).collect::<Vec<_>>();
            skills.sort_by(|left, right| left.1.name.cmp(&right.1.name));
            if skills.is_empty() {
                continue;
            }
            let mut card = widgets::section_card(&theme).mt_0();
            for (index, (_, skill)) in skills.iter().enumerate() {
                let description = if skill.description.trim().is_empty() {
                    skill.path.clone()
                } else {
                    skill.description.clone()
                };
                card = card.child(
                    widgets::card_row(&theme, index == 0)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(widgets::row_title(&theme, skill.name.clone()))
                                .child(widgets::meta_line(
                                    &theme,
                                    vec![div().child(SharedString::from(description)).into_any_element()],
                                )),
                        )
                        .child(
                            div()
                                .text_size(crate::typography::ui_rems(11.5))
                                .text_color(if skill.enabled { theme.success_muted } else { theme.text_muted })
                                .child(if skill.enabled { "Available" } else { "Disabled" }),
                        ),
                );
            }
            sections.push(
                widgets::section(&theme, label, card).into_any_element()
            );
        }

        let inventory_note = if self.loaded_harnesses > 0 {
            format!(
                "{} skills found across {} agent catalogs. Discovery follows the selected chat or project when one is active.",
                self.skills.len(),
                self.loaded_harnesses
            )
        } else if self.loading {
            "Querying the installed agent skill catalogs…".to_owned()
        } else {
            "No skill catalog has been loaded yet.".to_owned()
        };

        div()
            .id("skills-settings-page-host")
            .relative()
            .size_full()
            .child(
                crate::edge_fade::edge_faded(
                    16.0,
                    true,
                    true,
                    div()
                        .id("skills-settings-page")
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
                                        .child(widgets::page_header(&theme, "Skills", None))
                                        .child(refresh),
                                )
                                .child(
                                    widgets::section(
                                        &theme,
                                        "Discovered skills",
                                        widgets::section_card(&theme)
                                            .mt_0()
                                            .child(
                                                div()
                                                    .text_color(theme.text_muted)
                                                    .text_size(crate::typography::ui_rems(12.0))
                                                    .child(inventory_note),
                                            ),
                                    ),
                                )
                                .children(sections)
                                .children(self.warning.as_ref().map(|message| {
                                    div()
                                        .mt(px(6.0))
                                        .text_color(theme.text_muted)
                                        .text_size(crate::typography::ui_rems(12.0))
                                        .child(SharedString::from(message.clone()))
                                }))
                                .children(self.error.as_ref().map(|message| {
                                    div()
                                        .mt(px(6.0))
                                        .text_color(theme.danger_muted)
                                        .child(SharedString::from(message.clone()))
                                })),
                        ),
                )
                .fade_overflow_y(&self.scroll.scroll),
            )
    }
}
