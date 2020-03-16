import { createServer } from 'node:http';
import { createHash, randomBytes, timingSafeEqual } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { NativeSession } from './native.mjs';

const files = new Map([
  ['/', ['index.html', 'text/html; charset=utf-8']],
  ['/board.js', ['board.js', 'text/javascript; charset=utf-8']],
  ['/style.css', ['style.css', 'text/css; charset=utf-8']],
]);
const DATA_LIMIT = 300 * 1024;
const SESSION_TTL = 2 * 60 * 60 * 1000;
const SESSION_CAP = 48;
const scopes = new Set(['Black', 'White']);
const commands = new Set([
  'move', 'claim_repetition', 'claim_no_progress', 'offer_draw', 'accept_draw', 'resign',
  'pause', 'resume', 'takeback',
]);

function random(size) {
  return randomBytes(size).toString('base64url');
}

function digest(secret) {
  return createHash('sha256').update(secret).digest();
}

function matches(secret, expected) {
  return typeof secret === 'string' && secret.length === 43 && expected && timingSafeEqual(digest(secret), expected);
}

class Problem extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}

async function jsonBody(req) {
  if (req.headers['content-type']?.split(';')[0] !== 'application/json') {
    throw new Problem(415, 'Send application/json');
  }
  const length = Number(req.headers['content-length'] || 0);
  if (length > DATA_LIMIT) throw new Problem(413, 'Request exceeds the size limit');
  const chunks = [];
  let size = 0;
  for await (const chunk of req) {
    size += chunk.length;
    if (size > DATA_LIMIT) throw new Problem(413, 'Request exceeds the size limit');
    chunks.push(chunk);
  }
  let result;
  try {
    result = JSON.parse(Buffer.concat(chunks).toString('utf8'));
  } catch {
    throw new Problem(400, 'Invalid JSON');
  }
  if (!result || typeof result !== 'object' || Array.isArray(result)) {
    throw new Problem(400, 'Send a JSON object');
  }
  return result;
}

function reply(res, status, value) {
  const body = JSON.stringify(value);
  res.writeHead(status, { 'Content-Type': 'application/json; charset=utf-8', 'Content-Length': Buffer.byteLength(body) });
  res.end(body);
}

function headers(res) {
  res.setHeader('Content-Security-Policy', "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; frame-ancestors 'none'; form-action 'self'");
  res.setHeader('X-Content-Type-Options', 'nosniff');
  res.setHeader('Referrer-Policy', 'no-referrer');
  res.setHeader('Cache-Control', 'no-store');
  res.setHeader('Cross-Origin-Resource-Policy', 'same-origin');
}

function sessionConfig(body, local) {
  return {
    op: 'new',
    fen: local ? body.fen : null,
    policy: local ? body.policy ?? 'study' : 'tournament',
    clock: body.clock ?? null,
    local,
    opening: local ? body.opening : null,
  };
}

function authorize(req, room) {
  const bearer = req.headers.authorization?.match(/^Bearer ([\w-]+)$/)?.[1];
  for (const [role, expected] of room.tokens) {
    if (matches(bearer, expected)) return role;
  }
  throw new Problem(401, 'Rejoin with the original session credential');
}

function once(room, role, body, action) {
  const id = body.command_id;
  if (typeof id !== 'string' || !/^[\w-]{16,80}$/.test(id)) {
    throw new Problem(400, 'A unique command_id is required');
  }
  const key = role + ':' + id;
  const fingerprint = digest(JSON.stringify(body));
  const previous = room.commands.get(key);
  if (previous) {
    if (!timingSafeEqual(fingerprint, previous.fingerprint)) throw new Problem(409, 'Command id was already used for different input');
    return previous.promise;
  }
  const promise = action();
  room.commands.set(key, { fingerprint, promise });
  if (room.commands.size > 64) room.commands.delete(room.commands.keys().next().value);
  return promise;
}

export async function launch({
  host = '127.0.0.1',
  port = 8767,
  publicOrigin,
  engine = fileURLToPath(new URL('../target/release/crownline', import.meta.url)),
  table,
} = {}) {
  if (publicOrigin && new URL(publicOrigin).origin !== publicOrigin) throw new Error('CROWNLINE_ORIGIN must be an exact origin');
  const assets = new Map();
  for (const [route, [name, mime]] of files) {
    assets.set(route, { body: await readFile(new URL('./public/' + name, import.meta.url)), mime });
  }

  const rooms = new Map();
  const quotas = new Map();
  const janitor = setInterval(() => {
    const now = Date.now();
    for (const [id, room] of rooms) {
      if (room.native?.closed || now - room.touched > SESSION_TTL) {
        room.native?.close();
        rooms.delete(id);
      }
    }
    for (const [ip, quota] of quotas) {
      if (now - quota.start > 60_000) quotas.delete(ip);
    }
  }, 30_000);
  janitor.unref();

  async function api(req, res, path) {
    const expected = new URL(publicOrigin ?? 'http://' + (host.includes(':') ? '[' + host + ']' : host) + ':' + server.address().port);
    const acceptedHosts = new Set([expected.host]);
    if (!publicOrigin && ['127.0.0.1', '::1', 'localhost'].includes(host)) {
      for (const local of ['127.0.0.1', 'localhost', '[::1]']) acceptedHosts.add(local + ':' + server.address().port);
    }
    const origin = req.headers.origin;
    if (!acceptedHosts.has(req.headers.host) || (origin && (origin !== expected.origin && origin !== expected.protocol + '//' + req.headers.host)) || req.headers['sec-fetch-site'] === 'cross-site') {
      throw new Problem(403, 'Cross-origin API access denied');
    }
    const ip = req.socket.remoteAddress;
    const now = Date.now();
    const quota = quotas.get(ip) ?? { start: now, count: 0 };
    if (now - quota.start > 60_000) { quota.start = now; quota.count = 0; }
    quota.count += 1;
    if (quota.count > 420 || (!quotas.has(ip) && quotas.size >= 1024)) {
      throw new Problem(429, 'API request budget exceeded');
    }
    quotas.set(ip, quota);
    if (path === '/api/session' && req.method === 'POST') {
      if (rooms.size >= SESSION_CAP) throw new Problem(503, 'Session capacity reached');
      const body = await jsonBody(req);
      if (rooms.size >= SESSION_CAP) throw new Problem(503, 'Session capacity reached');
      if (!['local', 'room'].includes(body.mode)) throw new Problem(400, 'Choose local or room');
      const local = body.mode === 'local';
      const token = random(32);
      const invite = local ? null : random(32);
      const role = local ? 'Local' : 'Black';
      const id = random(12);
      const room = {
        local, config: sessionConfig(body, local), native: local ? new NativeSession(engine, table) : undefined,
        invitation: invite && digest(invite), tokens: new Map([[role, digest(token)]]),
        touched: Date.now(), commands: new Map(), rematch: new Set(),
      };
      rooms.set(id, room);
      try {
        const state = room.native ? await room.native.request(room.config) : null;
        reply(res, 201, { id, token, invite, role, state, waiting: !state });
      } catch (error) {
        room.native?.close();
        rooms.delete(id);
        throw error;
      }
      return;
    }
    const route = path.match(/^\/api\/session\/([\w-]{16})\/(state|join|act|study|rematch|close)$/);
    if (!route) throw new Problem(404, 'Unknown API path');
    const [, id, operation] = route;
    const room = rooms.get(id);
    if (!room) throw new Problem(404, 'Session expired or unavailable');
    if (operation === 'join' && req.method === 'POST') {
      const body = await jsonBody(req);
      if (!matches(body.invite, room.invitation)) throw new Problem(403, 'Invite is invalid or already used');
      room.invitation = null;
      const native = new NativeSession(engine);
      const token = random(32);
      try {
        const state = await native.request(room.config);
        room.tokens.set('White', digest(token));
        room.native = native;
        room.touched = Date.now();
        reply(res, 200, { id, token, role: 'White', state, waiting: false });
      } catch (error) {
        native.close();
        rooms.delete(id);
        throw error;
      }
      return;
    }
    const role = authorize(req, room);
    room.touched = Date.now();
    if (operation === 'state' && req.method === 'GET') {
      const state = room.native ? await room.native.request({ op: 'state' }) : null;
      reply(res, 200, { role, state, waiting: !state });
      return;
    }
    if (req.method !== 'POST') throw new Problem(405, 'Use POST for this operation');
    const body = await jsonBody(req);
    if (operation === 'close' && room.local) {
      room.native.close();
      rooms.delete(id);
      reply(res, 200, {});
      return;
    }
    if (!room.native) throw new Problem(409, 'Waiting for the second player');
    if (operation === 'act') {
      if (!Number.isSafeInteger(body.revision) || body.revision < 0 || !commands.has(body.command?.kind)) {
        throw new Problem(400, 'Expected a valid command and revision');
      }
      const actor = room.local ? body.actor : role;
      if (!scopes.has(actor)) throw new Problem(400, 'Expected a player side');
      if (!room.local && ['pause', 'resume', 'takeback'].includes(body.command.kind)) {
        throw new Problem(403, 'Command is available in local play only');
      }
      const state = await once(room, role, body, () => room.native.request({
        op: 'act', actor, revision: body.revision, command: body.command,
      }));
      reply(res, 200, { state });
      return;
    }
    if (operation === 'rematch') {
      const state = await room.native.request({ op: 'state' });
      if (!state.outcome) throw new Problem(409, 'Finish the current game before rematching');
      room.rematch.add(role);
      if (!room.local && room.rematch.size < 2) {
        reply(res, 200, { waiting: true, state });
        return;
      }
      room.rematch.clear();
      room.commands.clear();
      reply(res, 200, { state: await room.native.request(room.config) });
      return;
    }
    if (operation === 'study' && room.local) {
      const allowed = new Set(['analyze', 'import', 'restore', 'review', 'witness', 'bot']);
      if (!allowed.has(body.op)) throw new Problem(400, 'Unknown study request');
      const value = body.op === 'bot'
        ? await once(room, role, body, () => {
          const { command_id, ...nativeRequest } = body;
          return room.native.request(nativeRequest);
        })
        : await room.native.request(body);
      reply(res, 200, value);
      return;
    }
    throw new Problem(403, 'Private analysis is isolated from competitive rooms');
  }

  const server = createServer({ requestTimeout: 10_000, headersTimeout: 5000, keepAliveTimeout: 3000 }, (req, res) => {
    headers(res);
    const route = req.url.split('?')[0];
    if (req.method === 'GET' && assets.has(route)) {
      const { mime, body } = assets.get(route);
      res.writeHead(200, { 'Content-Type': mime, 'Content-Length': body.length });
      res.end(body);
      return;
    }
    if (!route.startsWith('/api/')) {
      reply(res, 404, { error: 'Page unavailable' });
      return;
    }
    api(req, res, route).catch(error => {
      if (res.destroyed || res.headersSent) return;
      const status = error.status ?? (/stale revision|no active bot turn/.test(error.message) ? 409 : 422);
      reply(res, status, { error: error.message });
    });
  });
  server.maxConnections = 128;
  server.maxRequestsPerSocket = 128;
  server.setTimeout(22_000, socket => socket.destroy());
  server.on('close', () => {
    clearInterval(janitor);
    for (const room of rooms.values()) room.native?.close();
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(port, host, resolve);
  });
  return server;
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  const options = {
    host: process.env.CROWNLINE_HOST || '127.0.0.1',
    port: Number(process.env.CROWNLINE_PORT || 8767),
    publicOrigin: process.env.CROWNLINE_ORIGIN || undefined,
    engine: process.env.CROWNLINE_ENGINE || undefined,
    table: process.env.CROWNLINE_TABLE || undefined,
  };
  const server = await launch(options);
  process.stdout.write('Crownline at http://' + options.host + ':' + server.address().port + '\n');
  for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, () => server.close());
}
