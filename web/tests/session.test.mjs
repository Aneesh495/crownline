import { test } from 'node:test';
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { launch } from '../server.mjs';

async function withServer(t) {
  const server = await launch({ port: 0 });
  t.after(() => { server.closeAllConnections(); return new Promise(resolve => server.close(resolve)); });
  const base = 'http://127.0.0.1:' + server.address().port;
  async function api(path, body, token, headers = {}) {
    const response = await fetch(base + path, {
      method: body === undefined ? 'GET' : 'POST',
      headers: { ...(body === undefined ? {} : { 'content-type': 'application/json' }), ...(token ? { authorization: 'Bearer ' + token } : {}), ...headers },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    return { status: response.status, json: await response.json() };
  }
  return { api, base };
}

test('competitive room authenticates both seats and isolates private operations', async t => {
  const { api } = await withServer(t);
  const { status, json: black } = await api('/api/session', { mode: 'room', clock: { initial_ms: 180_000, increment_ms: 2_000 } });
  assert.equal(status, 201);
  const path = '/api/session/' + black.id;
  assert.equal(black.waiting, true);
  assert.equal((await api(path + '/state')).status, 401);
  assert.equal((await api(path + '/join', { invite: 'x'.repeat(43) })).status, 403);
  const { json: white } = await api(path + '/join', { invite: black.invite });
  assert.equal(white.role, 'White');
  assert.equal((await api(path + '/join', { invite: black.invite })).status, 403);
  const move = { actor: 'White', revision: 0, command_id: randomUUID(), command: { kind: 'move', turn: '9-13' } };
  assert.equal((await api(path + '/act', move, white.token)).status, 422);
  const [played, duplicate] = await Promise.all([api(path + '/act', move, black.token), api(path + '/act', move, black.token)]);
  assert.equal(played.status, 200);
  assert.deepEqual(played, duplicate);
  assert.equal(played.json.state.pdn, null);
  assert.equal(played.json.state.revision, 1);
  assert.equal((await api(path + '/act', { ...move, actor: 'Black' }, black.token)).status, 409);
  assert.equal((await api(path + '/act', { ...move, command_id: randomUUID() }, black.token)).status, 409);
  assert.equal((await api(path + '/study', { op: 'analyze' }, white.token)).status, 403);
  assert.equal((await api(path + '/act', { ...move, command: { kind: 'takeback' } }, white.token)).status, 403);
  const rejoined = await api(path + '/state', undefined, white.token);
  assert.equal(rejoined.json.state.fen, played.json.state.fen);
  const resign = { revision: 1, command_id: randomUUID(), command: { kind: 'resign' } };
  assert.equal((await api(path + '/act', resign, white.token)).json.state.outcome.winner, 'Black');
  assert.equal((await api(path + '/rematch', {}, black.token)).json.waiting, true);
  assert.equal((await api(path + '/rematch', {}, white.token)).json.state.turns.length, 0);
  assert.equal((await api(path + '/act', move, black.token)).status, 409);
});

test('two local identities are isolated and native bot plays a legal turn', async t => {
  const { api, base } = await withServer(t);
  assert.equal((await api('/api/session', { mode: 'local' }, null, { origin: 'https://bad.invalid' })).status, 403);
  const { get } = await import('node:http');
  const wrongHost = await new Promise((resolve, reject) => {
    get(base + '/api/session', { headers: { host: 'example.invalid' } }, response => {
      response.resume();
      resolve(response.statusCode);
    }).on('error', reject);
  });
  assert.equal(wrongHost, 403);
  const { json: a } = await api('/api/session', { mode: 'local', fen: 'B:BK14:W10,11,18,19' });
  const { json: b } = await api('/api/session', { mode: 'local' });
  const path = '/api/session/' + a.id;
  assert.equal((await api(path + '/state', undefined, b.token)).status, 401);
  assert.equal(a.state.legal.length, 2);
  const study = await api(path + '/study', { op: 'analyze', millis: 20, nodes: 1000, depth: 4 }, a.token);
  assert.equal(study.status, 200);
  assert.ok(a.state.legal.some(turn => turn.text === study.json.best.text));
  const bot = await api(path + '/study', { op: 'bot', revision: 0, millis: 20, nodes: 1000, depth: 4, command_id: randomUUID() }, a.token);
  assert.equal(bot.status, 200);
  assert.equal(bot.json.state.outcome.reason, 'elimination');
  const takeback = await api(path + '/act', { revision: bot.json.state.revision, actor: 'White', command_id: randomUUID(), command: { kind: 'takeback' } }, a.token);
  assert.equal(takeback.status, 200);
  assert.equal(takeback.json.state.fen, a.state.fen);
  assert.equal(takeback.json.state.turns.length, 0);
});

test('a stalled upload leaves independent authenticated sessions responsive', async t => {
  const { api, base } = await withServer(t);
  const { request } = await import('node:http');
  const stalled = request(base + '/api/session', { method: 'POST', headers: { 'content-type': 'application/json', 'content-length': 80 } });
  stalled.on('error', () => {});
  t.after(() => stalled.destroy());
  stalled.write('{"mode":');
  const ready = await api('/api/session', { mode: 'local' });
  assert.equal(ready.status, 201);
  assert.equal((await api('/api/session/' + ready.json.id + '/state', undefined, ready.json.token)).json.state.legal.length, 7);
  stalled.destroy();
});
