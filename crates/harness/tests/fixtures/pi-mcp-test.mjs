import assert from 'node:assert/strict';
import http from 'node:http';
import {createInterface} from 'node:readline';

if (process.argv[2] === 'server') {
  createInterface({input: process.stdin}).on('line', line => {
    const msg = JSON.parse(line);
    let result;
    if (msg.method === 'initialize') result = {
      protocolVersion: '2024-11-05',
      capabilities: {tools: {}},
      serverInfo: {name: 'stdio-fixture', version: '1'},
    };
    else if (msg.method === 'tools/list') result = {
      tools: ['whoami', 'create_chat', 'create_chats', 'read_chat', 'send_message', 'wait_for_turn']
        .map(name => ({
          name,
          description: 'Fixture delegation tool',
          inputSchema: {type: 'object', properties: {mode: {type: 'string'}}},
        })),
    };
    else if (msg.method === 'tools/call') {
      if (msg.params.arguments.mode === 'hang') return;
      if (msg.params.arguments.mode === 'crash') process.exit(1);
      result = {
        content: [{type: 'text', text: process.env.ZERON_CHAT_ID}],
        isError: msg.params.arguments.mode === 'error',
      };
    } else return;
    process.stdout.write(JSON.stringify({jsonrpc: '2.0', id: msg.id, result}) + '\n');
  });
} else {
  // Exercise the same Streamable HTTP request/response shape as a remote MCP.
  const remote = http.createServer((request, response) => {
    let raw = '';
    request.setEncoding('utf8');
    request.on('data', chunk => { raw += chunk; });
    request.on('end', () => {
      const message = JSON.parse(raw);
      assert.equal(request.headers['x-test-token'], 'http-fixture-token');
      if (message.method === 'notifications/initialized') {
        response.writeHead(202);
        response.end();
        return;
      }
      let result;
      if (message.method === 'initialize') {
        result = {
          protocolVersion: '2024-11-05',
          capabilities: {tools: {}},
          serverInfo: {name: 'http-fixture', version: '1'},
        };
        response.setHeader('mcp-session-id', 'fixture-http-session');
      } else if (message.method === 'tools/list') {
        assert.equal(request.headers['mcp-session-id'], 'fixture-http-session');
        result = {tools: [{
          name: 'whoami',
          description: 'Return the remote fixture identity',
          inputSchema: {type: 'object', properties: {}},
        }]};
      } else if (message.method === 'tools/call') {
        assert.equal(request.headers['mcp-session-id'], 'fixture-http-session');
        assert.equal(message.params.name, 'whoami');
        result = {content: [{type: 'text', text: 'sheet-http-fixture'}]};
      } else {
        response.writeHead(400);
        response.end();
        return;
      }
      response.setHeader('content-type', 'application/json');
      response.end(JSON.stringify({jsonrpc: '2.0', id: message.id, result}));
    });
  });
  await new Promise(resolve => remote.listen(0, '127.0.0.1', resolve));
  const address = remote.address();
  const {default: extension} = await import(process.argv[2]);
  try {
    for (const chat of ['first', 'second']) {
      process.env.ZERON_PI_MCP_SERVERS = JSON.stringify([
        {
          name: 'zeron',
          command: process.execPath,
          args: [process.argv[1], 'server'],
          env: {ZERON_CHAT_ID: chat},
          enabled: true,
        },
        {
          name: 'sheet',
          url: `http://127.0.0.1:${address.port}/mcp`,
          headers: {'X-Test-Token': 'http-fixture-token'},
          enabled: true,
        },
      ]);
      const handlers = {}, tools = {};
      extension({
        on: (event, fn) => handlers[event] = fn,
        registerTool: tool => tools[tool.name] = tool,
      });
      try {
        await handlers.session_start();
        for (const name of ['create_chat', 'create_chats', 'read_chat', 'send_message', 'wait_for_turn']) {
          assert.equal((await tools[`zeron_${name}`].execute('delegation', {})).content[0].text, chat);
        }
        const tool = tools.zeron_whoami;
        assert.equal(tool.parameters.type, 'object');
        const result = await tool.execute('call', {});
        assert.equal(result.content[0].text, chat);
        assert.equal(tools.sheet_whoami.parameters.type, 'object');
        const remoteResult = await tools.sheet_whoami.execute('remote-call', {});
        assert.equal(remoteResult.content[0].text, 'sheet-http-fixture');
        await assert.rejects(tool.execute('call', {mode: 'error'}), new RegExp(chat));
        const controller = new AbortController();
        const waiting = tool.execute('call', {mode: 'hang'}, controller.signal);
        controller.abort();
        await assert.rejects(waiting, /cancelled/);
        await assert.rejects(tool.execute('call', {mode: 'crash'}), /exited/);
        assert.equal(handlers.message_end, undefined, 'native RPC reports provider failures directly');
        // Restarting must not let the old child fail new RPCs; HTTP tools reconnect too.
        await handlers.session_start();
        assert.equal((await tools.zeron_whoami.execute('call', {})).content[0].text, chat);
        assert.equal((await tools.sheet_whoami.execute('remote-call', {})).content[0].text, 'sheet-http-fixture');
      } finally {
        handlers.session_shutdown();
      }
      await assert.rejects(tools.zeron_whoami.execute('call', {}), /not connected/);
    }
  } finally {
    await new Promise((resolve, reject) => remote.close(error => error ? reject(error) : resolve()));
  }
}
