// Per-run MCP bridge for Pi. Every configured server is an owned child of
// this Pi session; no global MCP settings or packages are modified.
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

export default function (pi) {
  const servers = JSON.parse(process.env.ZERON_PI_MCP_SERVERS ?? "[]");
  const connections = new Map();

  function close(connection) {
    connection.fail(new Error(`MCP server "${connection.config.name}" disconnected`));
    connection.lines?.close();
    connection.child?.kill();
    connection.child = undefined;
  }

  function connect(config) {
    const connection = {
      config,
      child: undefined,
      lines: undefined,
      nextId: 0,
      pending: new Map(),
      fail(error) {
        for (const request of [...this.pending.values()]) request.reject(error);
      },
      rpc(method, params, signal) {
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
            }) + "\n");
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
          child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n", error => {
            if (error) this.pending.get(id)?.reject(error);
          });
        });
      },
      async start() {
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

        await this.rpc("initialize", {
          protocolVersion: "2024-11-05",
          capabilities: {},
          clientInfo: { name: "zeron-pi-mcp-bridge", version: "1" },
        });
        owned.stdin.write(JSON.stringify({
          jsonrpc: "2.0",
          method: "notifications/initialized",
        }) + "\n");
        const result = await this.rpc("tools/list", {});
        const tools = Array.isArray(result?.tools) ? result.tools : [];
        for (const tool of tools) {
          const safeServer = String(config.name).replace(/[^a-zA-Z0-9_]/g, "_");
          const safeTool = String(tool.name).replace(/[^a-zA-Z0-9_]/g, "_");
          pi.registerTool({
            name: `${safeServer}_${safeTool}`,
            label: `${config.name}: ${tool.name}`,
            description: tool.description ?? `Call MCP tool ${tool.name} on ${config.name}`,
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
                    .map(item => item.text).join("\n") || `MCP tool ${tool.name} failed`,
                );
              }
              return { content: response?.content ?? [], details: response?.structuredContent ?? {} };
            },
          });
        }
      },
    };
    return connection;
  }

  pi.on("session_start", async () => {
    for (const connection of connections.values()) close(connection);
    connections.clear();
    for (const config of servers) {
      if (!config?.name || !config?.command || config.enabled === false) continue;
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
