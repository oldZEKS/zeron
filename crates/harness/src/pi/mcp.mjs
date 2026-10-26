// Per-run MCP bridge for Pi. Supports stdio child processes and remote
// Streamable HTTP endpoints without changing the user's global Pi settings.
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

const PROTOCOL_FALLBACK = "2024-11-05";

export default function (pi) {
  const servers = JSON.parse(process.env.ZERON_PI_MCP_SERVERS ?? "[]");
  const connections = new Map();

  function close(connection) {
    connection.fail(new Error(`MCP server "${connection.config.name}" disconnected`));
    connection.lines?.close();
    connection.child?.kill();
    connection.child = undefined;
  }

  function requestHeaders(connection, method) {
    const headers = new Headers(connection.config.headers ?? {});
    headers.set("accept", "application/json, text/event-stream");
    if (method !== "initialize") {
      headers.set("content-type", "application/json");
      if (connection.sessionId) headers.set("Mcp-Session-Id", connection.sessionId);
      if (connection.protocolVersion) {
        headers.set("MCP-Protocol-Version", connection.protocolVersion);
      }
    } else {
      headers.set("content-type", "application/json");
    }
    return headers;
  }

  function parseHttpResponse(text, contentType, id) {
    if (!text.trim()) return {};
    let messages = [];
    if (contentType.includes("text/event-stream")) {
      // Streamable HTTP responses may contain progress notifications before
      // the JSON-RPC result. Return the frame matching this request id.
      for (const event of text.split(/\\r?\\n\\r?\\n/)) {
        const data = event.split(/\\r?\\n/)
          .filter(line => line.startsWith("data:"))
          .map(line => line.slice(5).trimStart())
          .join("\\n");
        if (!data || data === "[DONE]") continue;
        try {
          const parsed = JSON.parse(data);
          messages.push(...(Array.isArray(parsed) ? parsed : [parsed]));
        } catch {
          // Ignore non-JSON SSE events; only JSON-RPC frames complete a call.
        }
      }
    } else {
      const parsed = JSON.parse(text);
      messages = Array.isArray(parsed) ? parsed : [parsed];
    }
    const response = messages.find(message => message?.id === id);
    if (!response) throw new Error("MCP HTTP response did not contain the matching JSON-RPC id");
    if (response.error) {
      throw new Error(response.error.message ?? "MCP remote request failed");
    }
    return response.result ?? {};
  }

  function connect(config) {
    const connection = {
      config,
      child: undefined,
      lines: undefined,
      nextId: 0,
      pending: new Map(),
      sessionId: undefined,
      protocolVersion: PROTOCOL_FALLBACK,
      fail(error) {
        for (const request of [...this.pending.values()]) request.reject(error);
      },
      async httpRpc(method, params, signal) {
        if (signal?.aborted) throw new Error("MCP request cancelled");
        const id = ++this.nextId;
        const timeout = AbortSignal.timeout(method === "tools/call" ? 660_000 : 15_000);
        const requestSignal = signal ? AbortSignal.any([signal, timeout]) : timeout;
        let response;
        try {
          response = await fetch(config.url, {
            method: "POST",
            headers: requestHeaders(this, method),
            body: JSON.stringify({ jsonrpc: "2.0", id, method, params }),
            signal: requestSignal,
          });
        } catch (error) {
          if (signal?.aborted) throw new Error("MCP request cancelled");
          throw new Error(`MCP HTTP request to "${config.name}" failed: ${error?.message ?? error}`);
        }
        const sessionId = response.headers.get("mcp-session-id");
        if (sessionId) this.sessionId = sessionId;
        if (!response.ok) {
          // Do not include request headers in diagnostics: they may contain
          // bearer credentials. The response body can echo server secrets too.
          throw new Error(`MCP server "${config.name}" returned HTTP ${response.status} for ${method}`);
        }
        if (response.status === 202) {
          throw new Error(`MCP server "${config.name}" accepted ${method} without a response`);
        }
        const result = parseHttpResponse(
          await response.text(),
          response.headers.get("content-type") ?? "",
          id,
        );
        if (method === "initialize" && result.protocolVersion) {
          this.protocolVersion = result.protocolVersion;
        }
        return result;
      },
      async notify(method, params = {}) {
        if (config.url) {
          const response = await fetch(config.url, {
            method: "POST",
            headers: requestHeaders(this, method),
            body: JSON.stringify({ jsonrpc: "2.0", method, params }),
          });
          const sessionId = response.headers.get("mcp-session-id");
          if (sessionId) this.sessionId = sessionId;
          if (!response.ok) {
            throw new Error(`MCP server "${config.name}" returned HTTP ${response.status} for ${method}`);
          }
          return;
        }
        if (!this.child?.stdin?.writable) return;
        this.child.stdin.write(JSON.stringify({ jsonrpc: "2.0", method, params }) + "\\n");
      },
      rpc(method, params, signal) {
        if (config.url) return this.httpRpc(method, params, signal);
        const child = this.child;
        if (!child?.stdin?.writable) {
          return Promise.reject(new Error(`MCP server "${config.name}" is not connected`));
        }
        if (signal?.aborted) return Promise.reject(new Error("MCP request cancelled"));
        return new Promise((resolve, reject) => {
          const id = ++this.nextId;
          const cleanup = () => {
            clearTimeout(timer);
            signal?.removeEventListener("abort", abort);
            this.pending.delete(id);
          };
          const abort = () => {
            child.stdin.write(JSON.stringify({
              jsonrpc: "2.0",
              method: "notifications/cancelled",
              params: { requestId: id },
            }) + "\\n");
            cleanup();
            reject(new Error("MCP request cancelled"));
          };
          const timer = setTimeout(() => {
            cleanup();
            reject(new Error(`MCP server "${config.name}" timed out on ${method}`));
          }, method === "tools/call" ? 660_000 : 15_000);
          this.pending.set(id, {
            resolve: value => { cleanup(); resolve(value); },
            reject: error => { cleanup(); reject(error); },
          });
          signal?.addEventListener("abort", abort, { once: true });
          child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\\n", error => {
            if (error) this.pending.get(id)?.reject(error);
          });
        });
      },
      async listTools() {
        const tools = [];
        let cursor;
        // MCP tool catalogs may be paginated; don't silently expose only page 1.
        for (let page = 0; page < 50; page++) {
          const result = await this.rpc("tools/list", cursor ? { cursor } : {});
          if (Array.isArray(result?.tools)) tools.push(...result.tools);
          cursor = result?.nextCursor;
          if (!cursor) return tools;
        }
        throw new Error(`MCP server "${config.name}" returned too many tool pages`);
      },
      async start() {
        if (!config.url) {
          this.child = spawn(config.command, config.args ?? [], {
            env: { ...process.env, ...(config.env ?? {}) },
            stdio: ["pipe", "pipe", "inherit"],
          });
          const owned = this.child;
          const failed = error => { if (this.child === owned) this.fail(error); };
          owned.on("error", failed);
          owned.on("exit", () => failed(new Error(`MCP server "${config.name}" exited`)));
          owned.stdin.on("error", failed);
          this.lines = createInterface({ input: owned.stdout });
          this.lines.on("line", line => {
            let message;
            try { message = JSON.parse(line); } catch { return; }
            const request = this.pending.get(message.id);
            if (!request) return;
            if (message.error) request.reject(new Error(message.error.message));
            else request.resolve(message.result);
          });
        }

        const init = await this.rpc("initialize", {
          protocolVersion: PROTOCOL_FALLBACK,
          capabilities: {},
          clientInfo: { name: "zeron-pi-mcp-bridge", version: "1" },
        });
        if (init?.protocolVersion) this.protocolVersion = init.protocolVersion;
        await this.notify("notifications/initialized");
        const tools = await this.listTools();
        for (const tool of tools) {
          const safeServer = String(config.name).replace(/[^a-zA-Z0-9_]/g, "_");
          const safeTool = String(tool.name).replace(/[^a-zA-Z0-9_]/g, "_");
          pi.registerTool({
            name: `${safeServer}_${safeTool}`,
            label: `${config.name}: ${tool.name}`,
            description: `MCP server "${config.name}", tool "${tool.name}". ${tool.description ?? "Invoke this server tool when relevant to the user's task."}`,
            parameters: tool.inputSchema ?? { type: "object", properties: {} },
            async execute(_id, args, signal) {
              const response = await connection.rpc(
                "tools/call",
                { name: tool.name, arguments: args },
                signal,
              );
              if (response?.isError) {
                throw new Error(
                  response.content?.filter(item => item.type === "text")
                    .map(item => item.text).join("\\n") || `MCP tool ${tool.name} failed`,
                );
              }
              return { content: response?.content ?? [], details: response?.structuredContent ?? {} };
            },
          });
        }
        console.error(`Connected MCP server "${config.name}" (${tools.length} tools)`);
      },
    };
    return connection;
  }

  pi.on("session_start", async () => {
    for (const connection of connections.values()) close(connection);
    connections.clear();
    for (const config of servers) {
      if (!config?.name || config.enabled === false) continue;
      if (!config.url && !config.command) {
        console.error(`MCP server "${config.name}" has neither url nor command`);
        continue;
      }
      const connection = connect(config);
      connections.set(config.name, connection);
      try {
        await connection.start();
      } catch (error) {
        close(connection);
        console.error(`Could not initialize MCP server "${config.name}": ${error?.message ?? error}`);
      }
    }
  });
  pi.on("session_shutdown", () => {
    for (const connection of connections.values()) close(connection);
    connections.clear();
  });
  process.once("exit", () => {
    for (const connection of connections.values()) close(connection);
    connections.clear();
  });
}
