//! Load per-run MCP servers into this Pi process only. No global packages
//! or user configuration are modified.
use crate::{HarnessError, process::Command, scratch::ScratchDir};

pub(super) fn configure(
    cmd: &mut Command,
    servers: &[zeron_proto::McpServer],
) -> Result<ScratchDir, HarnessError> {
    let scratch = ScratchDir::new("pi-mcp")?;
    let extension = scratch.path().join("zeron-mcp.mjs");
    std::fs::write(&extension, include_str!("mcp.mjs"))?;
    cmd.arg("--extension").arg(extension).env(
        "ZERON_PI_MCP_SERVERS",
        serde_json::to_string(servers).expect("serializable MCP configuration"),
    );
    Ok(scratch)
}
