//! Settings → MCP Servers: device-local stdio MCP configuration.
//!
//! A JSON editor is deliberately used for this first management surface so
//! command arguments and environment maps can be represented without lossy
//! per-field UI. Config is validated before it becomes active and is injected
//! only into newly started agent runs.
use std::collections::HashSet;

use gpui::{Context, Entity, Focusable as _, Render, SharedString, Window, div, prelude::*, px};

use crate::composer::ComposerInput;
use crate::settings::{self, ConfiguredMcpServer, SavePolicy};
use crate::settings::widgets::{self, ActionTone};
use crate::theme::Theme;

const EXAMPLE: &str = r#"[
  {
    "name": "filesystem",
    "command": "npx",
    "args": ["-y", "@modelcontextprotocol/server-filesystem", "C:/path/to/project"],
    "env": {},
    "enabled": false
  },
  {
    "name": "sheet",
    "url": "http://127.0.0.1:4317/mcp",
    "headers": {},
    "enabled": false
  }
]"#;

pub struct McpsPage {
    editor: Entity<ComposerInput>,
    scroll: widgets::PageScroll,
    error: Option<String>,
    saved: Option<String>,
}

/// HTTP header names must be visible ASCII tokens; values cannot inject new headers.
fn valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
        })
}

fn validate_servers(servers: &mut [ConfiguredMcpServer]) -> Result<(), String> {
    if servers.len() > 32 {
        return Err("A maximum of 32 MCP servers can be configured.".into());
    }
    let mut names = HashSet::new();
    for (index, server) in servers.iter_mut().enumerate() {
        server.name = server.name.trim().to_owned();
        server.command = server.command.trim().to_owned();
        server.url = server
            .url
            .take()
            .map(|url| url.trim().to_owned())
            .filter(|url| !url.is_empty());
        if server.name.is_empty()
            || !server.name.chars().all(|ch| {
                ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')
            })
        {
            return Err(format!(
                "Server {} needs a name containing only letters, numbers, dots, underscores, or hyphens.",
                index + 1
            ));
        }
        if server.name.eq_ignore_ascii_case("zeron") {
            return Err("The name 'zeron' is reserved for Zeron's built-in MCP server.".into());
        }
        if !names.insert(server.name.to_ascii_lowercase()) {
            return Err(format!("The server name '{}' is duplicated.", server.name));
        }

        let remote = server.url.is_some();
        if remote && !server.command.is_empty() {
            return Err(format!(
                "Server '{}' must use either 'url' (HTTP) or 'command' (stdio), not both.",
                server.name
            ));
        }
        if !remote && server.command.is_empty() {
            return Err(format!(
                "Server '{}' needs a 'command' for stdio or a 'url' for HTTP.",
                server.name
            ));
        }
        if let Some(url) = server.url.as_deref() {
            match url::Url::parse(url) {
                Ok(parsed)
                    if matches!(parsed.scheme(), "http" | "https")
                        && parsed.host_str().is_some()
                        && parsed.username().is_empty()
                        && parsed.password().is_none()
                        && parsed.fragment().is_none() => {}
                _ => {
                    return Err(format!(
                        "Server '{}' needs an absolute http:// or https:// MCP URL without embedded credentials or a fragment.",
                        server.name
                    ));
                }
            }
            if !server.args.is_empty() || !server.env.is_empty() {
                return Err(format!(
                    "HTTP server '{}' uses 'headers', not stdio 'args' or 'env'.",
                    server.name
                ));
            }
        } else if !server.headers.is_empty() {
            return Err(format!(
                "Stdio server '{}' uses 'env'; 'headers' is only for HTTP servers.",
                server.name
            ));
        }
        if server.command.contains('\\0')
            || server.args.iter().any(|arg| arg.contains('\\0'))
            || server.env.iter().any(|(key, value)| {
                key.is_empty() || key.contains('=') || key.contains('\\0') || value.contains('\\0')
            })
            || server.headers.iter().any(|(key, value)| {
                !valid_header_name(key) || value.contains(['\\r', '\\n', '\\0'])
            })
        {
            return Err(format!(
                "Server '{}' contains an invalid command, argument, environment entry, or HTTP header.",
                server.name
            ));
        }
    }
    Ok(())
}

impl McpsPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let initial = serde_json::to_string_pretty(&settings::current(cx).mcp_servers)
            .unwrap_or_else(|_| "[]".to_owned());
        let editor = cx.new(|cx| {
            let mut input = ComposerInput::new("Enter a JSON array of MCP server configurations…", cx);
            input.set_text(initial, cx);
            input
        });
        Self {
            editor,
            scroll: widgets::PageScroll::default(),
            error: None,
            saved: None,
        }
    }

    fn set_editor_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| editor.set_text(text, cx));
        self.error = None;
        self.saved = None;
        cx.notify();
    }

    fn load_saved(&mut self, cx: &mut Context<Self>) {
        let text = serde_json::to_string_pretty(&settings::current(cx).mcp_servers)
            .unwrap_or_else(|_| "[]".to_owned());
        self.set_editor_text(text, cx);
    }

    fn load_example(&mut self, cx: &mut Context<Self>) {
        self.set_editor_text(EXAMPLE.to_owned(), cx);
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let raw = self.editor.read(cx).text().to_owned();
        let mut servers: Vec<ConfiguredMcpServer> = match serde_json::from_str(&raw) {
            Ok(servers) => servers,
            Err(error) => {
                self.error = Some(format!("Invalid JSON: {error}"));
                self.saved = None;
                cx.notify();
                return;
            }
        };
        if let Err(error) = validate_servers(&mut servers) {
            self.error = Some(error);
            self.saved = None;
            cx.notify();
            return;
        }
        if settings::update(SavePolicy::Immediate, cx, |settings| {
            settings.mcp_servers = servers.clone();
        }) {
            self.error = None;
            self.saved = Some(format!(
                "Saved {} MCP server{}. Changes apply to new runs.",
                servers.len(),
                if servers.len() == 1 { "" } else { "s" }
            ));
            cx.refresh_windows();
        } else {
            self.error = Some("Settings are not ready yet. Restart Zeron and try again.".into());
            self.saved = None;
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(value: &str) -> ConfiguredMcpServer {
        serde_json::from_str(value).expect("valid fixture JSON")
    }

    #[test]
    fn accepts_sheet_streamable_http_endpoint_without_stdio_command() {
        let mut servers = vec![server(
            r#"{"name":" sheet ","url":" http://127.0.0.1:4317/mcp ","headers":{},"enabled":true}"#,
        )];
        validate_servers(&mut servers).unwrap();
        assert_eq!(servers[0].name, "sheet");
        assert_eq!(servers[0].command, "");
        assert_eq!(servers[0].url.as_deref(), Some("http://127.0.0.1:4317/mcp"));
    }

    #[test]
    fn keeps_legacy_stdio_settings_valid() {
        let mut servers = vec![server(
            r#"{"name":"filesystem","command":"npx","args":["-y","server"],"env":{},"enabled":true}"#,
        )];
        validate_servers(&mut servers).unwrap();
        assert_eq!(servers[0].command, "npx");
        assert_eq!(servers[0].url, None);
    }

    #[test]
    fn rejects_mixed_stdio_and_http_transports() {
        let mut servers = vec![server(
            r#"{"name":"sheet","command":"node","url":"http://127.0.0.1:4317/mcp","enabled":true}"#,
        )];
        assert!(validate_servers(&mut servers).unwrap_err().contains("not both"));
    }

    #[test]
    fn rejects_non_http_urls_and_embedded_credentials() {
        for url in [
            "file:///tmp/mcp",
            "ftp://127.0.0.1/mcp",
            "http://user:secret@127.0.0.1:4317/mcp",
            "http://127.0.0.1:4317/mcp#fragment",
        ] {
            let mut servers = vec![ConfiguredMcpServer {
                name: "remote".into(),
                url: Some(url.into()),
                enabled: true,
                ..Default::default()
            }];
            assert!(validate_servers(&mut servers).is_err(), "accepted {url}");
        }
    }
}

impl Render for McpsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>().clone();
        let configured = settings::current(cx).mcp_servers;
        let server_count = configured.len();
        let enabled_count = configured.iter().filter(|server| server.enabled).count();
        let mut server_rows = Vec::new();
        for (index, server) in configured.iter().enumerate() {
            let state_label = if server.enabled { "Enabled" } else { "Disabled" };
            let state_color = if server.enabled { theme.success_muted } else { theme.text_muted };
            let endpoint = match &server.url {
                Some(url) => format!("HTTP · {url}"),
                None => format!("stdio · {}", server.command),
            };
            server_rows.push(
                widgets::card_row(&theme, index == 0)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(widgets::row_title(&theme, server.name.clone()))
                            .child(widgets::meta_line(
                                &theme,
                                vec![div().child(SharedString::from(endpoint)).into_any_element()],
                            )),
                    )
                    .child(
                        div()
                            .text_size(crate::typography::ui_rems(11.5))
                            .text_color(state_color)
                            .child(state_label),
                    )
                    .into_any_element(),
            );
        }
        let saved_servers_section = widgets::section(
            &theme,
            "Saved servers",
            widgets::section_card(&theme)
                .mt_0()
                .child(
                    div()
                        .text_color(theme.text_muted)
                        .text_size(crate::typography::ui_rems(12.0))
                        .child(format!("{server_count} configured · {enabled_count} enabled. Changes apply to new agent runs.")),
                )
                .children(if server_rows.is_empty() {
                    Some(div().mt(px(8.0)).text_color(theme.text_muted).child("No servers configured yet. Add one in the JSON editor below."))
                } else {
                    None
                })
                .children(server_rows),
        );
        let editor = self.editor.clone();
        let editor_panel = div()
            .id("mcp-json-editor-panel")
            .w_full()
            .min_h(px(300.0))
            .max_h(px(520.0))
            .overflow_y_scroll()
            .rounded(px(10.0))
            .border_1()
            .border_color(theme.border)
            .bg(theme.input_glass_bg())
            .p(px(12.0))
            .child(editor);

        let save_button = widgets::action_button(&theme, ActionTone::Solid)
            .id("mcp-save-apply")
            .tab_index(0)
            .role(gpui::Role::Button)
            .aria_label("Save and apply MCP configuration")
            .on_click(cx.listener(|this, _, _, cx| this.save(cx)))
            .child("Save & Apply");
        let reload_button = widgets::action_button(&theme, ActionTone::Quiet)
            .id("mcp-reload-saved")
            .tab_index(0)
            .role(gpui::Role::Button)
            .aria_label("Reload saved MCP configuration")
            .on_click(cx.listener(|this, _, _, cx| this.load_saved(cx)))
            .child("Reload saved");
        let example_button = widgets::action_button(&theme, ActionTone::Quiet)
            .id("mcp-load-example")
            .tab_index(0)
            .role(gpui::Role::Button)
            .aria_label("Load an example MCP configuration")
            .on_click(cx.listener(|this, _, _, cx| this.load_example(cx)))
            .child("Load example");

        let mut result = div()
            .id("mcp-settings-page-host")
            .relative()
            .size_full()
            .child(
                crate::edge_fade::edge_faded(
                    16.0,
                    true,
                    true,
                    div()
                        .id("mcp-settings-page")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll.scroll)
                        .child(
                            widgets::page_column()
                                .child(widgets::page_header(&theme, "MCP Servers", None))
                                .child(saved_servers_section)
                                .child(widgets::section(
                                    &theme,
                                    "Configuration",
                                    widgets::section_card(&theme)
                                        .mt_0()
                                        .child(
                                            div()
                                                .text_color(theme.text)
                                                .child("Configure local stdio MCP servers or Streamable HTTP endpoints for supported local coding-agent runners. Enabled servers are injected into new local runs only where the provider supports the transport; remote-hosted runs do not receive this device's MCP configuration.")
                                                .mb(px(8.0)),
                                        )
                                        .child(
                                            div()
                                                .text_color(theme.text_muted)
                                                .text_size(crate::typography::ui_rems(12.0))
                                                .mb(px(12.0))
                                                .child("Use a JSON array. Stdio entries use name, command, optional args/env, and enabled. HTTP entries use name, url, optional headers, and enabled. Set exactly one of command or url. For HTTP, the endpoint must be reachable from the machine running the agent; 127.0.0.1 refers to that machine. Only enabled entries are passed to new local runs; the built-in Zeron MCP remains separate."),
                                        )
                                        .child(editor_panel)
                                        .child(
                                            div()
                                                .flex()
                                                .flex_row()
                                                .flex_wrap()
                                                .gap(px(8.0))
                                                .mt(px(12.0))
                                                .child(save_button)
                                                .child(reload_button)
                                                .child(example_button),
                                        )
                                        .children(self.error.as_ref().map(|message| {
                                            div()
                                                .mt(px(10.0))
                                                .text_color(theme.danger_muted)
                                                .child(SharedString::from(message.clone()))
                                        }))
                                        .children(self.saved.as_ref().map(|message| {
                                            div()
                                                .mt(px(10.0))
                                                .text_color(theme.text_muted)
                                                .child(SharedString::from(message.clone()))
                                        })),
                                ))
                                .child(widgets::section(
                                    &theme,
                                    "Security and scope",
                                    widgets::section_card(&theme)
                                        .mt_0()
                                        .child(
                                            div()
                                                .text_color(theme.text_muted)
                                                .text_size(crate::typography::ui_rems(12.0))
                                                .child("MCP server commands run as local child processes with your user permissions. Configure only servers you trust. Environment values are stored in the device-local ui-settings.json file; do not paste credentials you do not want stored there."),
                                        ),
                                )),
                        ),
                )
                .fade_overflow_y(&self.scroll.scroll),
            );
        result
    }
}
