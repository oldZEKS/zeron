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
            scroll: widgets::PageScroll::default(),
            task: None,
        };
        page.refresh(cx);
        page
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.state.read(cx).engine().cloned() else {
            self.error = Some("Engine not connected.".into());
            self.loading = false;
            cx.notify();
            return;
        };
        let (chat_id, space_id) = {
            let state = self.state.read(cx);
            if let Some(chat_id) = state.selected_chat.clone() {
                (Some(chat_id), None)
            } else {
                (None, state.selected_space_row().map(|space| space.id.clone()))
            }
        };
        self.loading = true;
        self.error = None;
        self.task = Some(cx.spawn(async move |this, cx| {
            let mut all = Vec::new();
            let mut successes = 0usize;
            let mut failures = Vec::new();
            for (harness, label) in SKILL_HARNESSES {
                let mut params = serde_json::json!({ "harness": harness });
                if let Some(chat_id) = &chat_id {
                    params["chatId"] = serde_json::Value::String(chat_id.clone());
                } else if let Some(space_id) = &space_id {
                    params["spaceId"] = serde_json::Value::String(space_id.clone());
                }
                match engine.client().call(methods::LIST_SKILLS, params).await {
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
            this.update(cx, |page, cx| {
                page.skills = all;
                page.loaded_harnesses = successes;
                page.loading = false;
                page.error = if successes == 0 {
                    Some(format!(
                        "Couldn't load skills from any agent. {}",
                        failures.into_iter().take(2).collect::<Vec<_>>().join(" ")
                    ))
                } else {
                    None
                };
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
            let skills = self.skills.iter().filter(|(id, _)| *id == harness).collect::<Vec<_>>();
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
                                .children(self.error.as_ref().map(|message| {
                                    div()
                                        .text_color(theme.danger_muted)
                                        .child(SharedString::from(message.clone()))
                                })),
                        ),
                )
                .fade_overflow_y(&self.scroll.scroll),
            )
    }
}
