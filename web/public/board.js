const $ = id => document.getElementById(id);
const controls = {
  board: $('board'), turns: $('turns'), mode: $('mode'), notice: $('notice'), pdn: $('pdn'),
  status: $('status'), hint: $('hint'), output: $('analysis-output'),
};
const startFen = 'B:B1,2,3,4,5,6,7,8,9,10,11,12:W21,22,23,24,25,26,27,28,29,30,31,32';
const difficulty = {
  quick: { millis: 45, nodes: 1500, depth: 5 },
  club: { millis: 180, nodes: 12_000, depth: 10 },
  deep: { millis: 700, nodes: 95_000, depth: 22 },
};
let session;
let state;
let pending = false;
let botTimer;
let replay;
let record;
let flipped = false;
let numbers = true;
let focusSquare = 9;
let path = [];
let dragged;
let polling = false;
let analysisController;
let clockSample = performance.now();

function readStorage(store, key) {
  try { return JSON.parse(store.getItem(key)); } catch { return null; }
}

function saveStorage(store, key, value) {
  try { store.setItem(key, JSON.stringify(value)); } catch { /* Private browsing can disable storage. */ }
}

function clearStorage(store, key) {
  try { store.removeItem(key); } catch { /* Storage may be unavailable. */ }
}

function opposite(side) { return side === 'Black' ? 'White' : 'Black'; }

function uid() {
  if (typeof crypto.randomUUID === 'function') return crypto.randomUUID();
  return Array.from(crypto.getRandomValues(new Uint8Array(18)), n => n.toString(16).padStart(2, '0')).join('');
}

async function request(url, body, { token, signal } = {}) {
  const headers = {};
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  if (token) headers.Authorization = 'Bearer ' + token;
  const response = await fetch(url, { method: body === undefined ? 'GET' : 'POST', headers, body: body === undefined ? undefined : JSON.stringify(body), signal });
  const result = await response.json();
  if (!response.ok) {
    const error = new Error(result.error);
    error.status = response.status;
    throw error;
  }
  return result;
}

function sessionUrl(op, current = session) { return '/api/session/' + current.id + '/' + op; }
function remote() { return session && session.role !== 'Local'; }
function canStudy() { return session && !remote() && !pending; }

function error(message) {
  controls.notice.textContent = message ?? '';
  controls.notice.hidden = !message;
  if ($('panel-game').hidden && message) controls.output.textContent = message;
}

function safe(task) {
  return (...args) => Promise.resolve().then(() => task(...args)).catch(problem => error(problem.message));
}

function pieceMap(fen) {
  const map = new Map();
  for (const section of fen.split(':').slice(1)) {
    const side = section[0] === 'B' ? 'Black' : 'White';
    for (const token of section.slice(1).split(',').filter(Boolean)) {
      map.set(Number(token.replace('K', '')), { side, king: token.startsWith('K') });
    }
  }
  return map;
}

function coords(square) {
  const row = Math.floor((square - 1) / 4);
  return [row, 2 * ((square - 1) % 4) + 1 - row % 2];
}

function squareAt(row, col) {
  return row >= 0 && row < 8 && col >= 0 && col < 8 && (row + col) % 2
    ? row * 4 + Math.floor(col / 2) + 1
    : null;
}

function candidates(prefix = path) {
  return (state?.legal || []).filter(turn => prefix.every((square, i) => turn.path[i] === square));
}

function currentFen() { return replay?.fen || state?.fen || startFen; }

function movingAllowed() {
  return state && !state.outcome && !replay && !pending
    && (!remote() || state.side === session.role) && !botSide(state.side) && !state.clock?.paused;
}

function displayPieces() {
  const map = pieceMap(currentFen());
  const captured = new Set();
  if (!replay && path.length > 1) {
    const piece = map.get(path[0]);
    if (piece) {
      map.delete(path[0]);
      for (let index = 1; index < path.length; index += 1) {
        const [r1, c1] = coords(path[index - 1]);
        const [r2, c2] = coords(path[index]);
        if (Math.abs(r1 - r2) === 2) captured.add(squareAt((r1 + r2) / 2, (c1 + c2) / 2));
      }
      map.set(path.at(-1), piece);
    }
  }
  return { map, captured };
}

function renderBoard() {
  const { map, captured } = displayPieces();
  const focus = controls.board.contains(document.activeElement);
  const next = new Set(movingAllowed() ? candidates().map(turn => turn.path[path.length]).filter(Boolean) : []);
  const last = replay?.path || state?.turns.at(-1)?.path || [];
  const fragment = document.createDocumentFragment();
  for (let row = 0; row < 8; row += 1) {
    for (let col = 0; col < 8; col += 1) {
      const square = squareAt(flipped ? 7 - row : row, flipped ? 7 - col : col);
      if (!square) {
        const pale = document.createElement('div');
        pale.className = 'square light-square';
        pale.setAttribute('aria-hidden', 'true');
        fragment.append(pale);
        continue;
      }
      const button = document.createElement('button');
      button.type = 'button';
      button.dataset.square = square;
      button.className = 'square';
      button.tabIndex = square === focusSquare ? 0 : -1;
      if (last.includes(square)) button.classList.add('last');
      if (path.at(-1) === square) button.classList.add('selected');
      if (next.has(square)) button.classList.add('target');
      const piece = map.get(square);
      const number = document.createElement('span');
      number.className = 'number';
      number.textContent = square;
      number.setAttribute('aria-hidden', 'true');
      button.append(number);
      const names = ['Square ' + square];
      if (piece) {
        names.push(piece.side + (piece.king ? ' king' : ' man'));
        const face = document.createElement('span');
        face.className = 'piece ' + piece.side.toLowerCase() + (captured.has(square) ? ' taken' : '');
        face.setAttribute('aria-hidden', 'true');
        face.textContent = piece.king ? '♛' : '';
        face.draggable = Boolean(movingAllowed() && !path.length && next.has(square));
        button.append(face);
      }
      if (captured.has(square)) names.push('jumped');
      if (next.has(square)) names.push(path.length ? 'legal landing' : 'can move');
      button.setAttribute('aria-label', names.join(', '));
      fragment.append(button);
    }
  }
  controls.board.replaceChildren(fragment);
  controls.board.classList.toggle('hide-numbers', !numbers);
  if (focus) controls.board.querySelector('[data-square="' + focusSquare + '"]')?.focus({ preventScroll: true });
}

function botSide(side) {
  if (!session || remote()) return false;
  return session.mode === 'bots' || (session.mode === 'bot' && side === 'White')
    || (session.mode === 'bot-black' && side === 'Black');
}

function formatClock(ms) {
  if (ms === undefined) return '∞';
  const seconds = Math.ceil(ms / 1000);
  return Math.floor(seconds / 60) + ':' + String(seconds % 60).padStart(2, '0');
}

function renderClocks() {
  for (const [slot, side] of [['top', flipped ? 'Black' : 'White'], ['bottom', flipped ? 'White' : 'Black']]) {
    const clock = state?.clock;
    const active = !replay && state?.side === side && !state?.outcome && !clock?.paused;
    const elapsed = active && clock ? performance.now() - clockSample : 0;
    const time = clock ? Math.max(0, clock[side.toLowerCase() + '_ms'] - elapsed) : undefined;
    $(slot + '-player').classList.toggle('active', active);
    $(slot + '-player').querySelector('.player-icon').className = 'player-icon ' + side.toLowerCase();
    $(slot + '-name').textContent = side;
    $(slot + '-status').textContent = botSide(side) ? (active ? 'Engine to move' : 'Engine')
      : active ? 'To move' : session?.role === side ? 'You' : 'Player';
    $(slot + '-clock').textContent = formatClock(time);
    $(slot + '-clock').setAttribute('aria-label', side + ' clock');
  }
}

function renderTurns() {
  const fragment = document.createDocumentFragment();
  const turns = state?.turns ?? [];
  if (turns.length === 0) {
    const empty = document.createElement('p');
    empty.className = 'empty';
    empty.textContent = 'No turns played';
    fragment.append(empty);
  }
  const offset = Number(state?.start.startsWith('W:') || false);
  for (let i = 0; i < turns.length + offset; i += 2) {
    const row = document.createElement('div');
    row.className = 'turn-row';
    for (const text of [(i / 2 + 1) + '.', turns[i - offset]?.text || '', turns[i + 1 - offset]?.text || '']) {
      const cell = document.createElement('span');
      cell.textContent = text;
      row.append(cell);
    }
    fragment.append(row);
  }
  controls.turns.replaceChildren(fragment);
  controls.turns.scrollTop = controls.turns.scrollHeight;
}

function renderGame() {
  const title = replay ? 'Variation board' : state?.outcome
    ? state.outcome.winner ? state.outcome.winner + ' wins' : 'Game drawn'
    : !state ? 'Waiting for opponent' : state.clock?.paused ? 'Clock paused' : state.side + ' to move';
  controls.status.textContent = title;
  $('game-mode').textContent = remote() ? 'Invite-only room · ' + session.role
    : { study: 'Local study', local: 'Local two-player', bot: 'Human vs engine', 'bot-black': 'Human vs engine', bots: 'Engine vs engine' }[session?.mode] ?? 'Local game';
  $('revision').textContent = (state?.turns.length ?? 0) + ' ply';
  controls.hint.textContent = replay ? 'Viewing the saved position. Return to the live board in the PDN panel.'
    : state?.outcome ? state.outcome.reason.replaceAll('_', ' ')
    : path.length > 1 ? 'Continue the same capture. The clock and turn still belong to ' + state.side + '.'
    : state?.legal[0]?.capture ? 'Capture is compulsory. Choose any complete legal route.'
    : !state ? 'Share the one-use invite to begin the room.'
    : botSide(state.side) ? 'Native engine turn.'
    : 'Select a piece to move. Arrow keys navigate playable squares.';
  $('capture-label').textContent = state?.legal[0]?.capture ? 'Capture required' : '';
  $('takeback').disabled = remote() || !state?.turns.length || state?.policy !== 'study' || pending;
  $('pause').disabled = remote() || !state?.clock || Boolean(state?.outcome) || pending;
  $('pause').textContent = state?.clock?.paused ? 'Resume clock' : 'Pause clock';
  $('draw').disabled = !state || Boolean(state.outcome) || pending || (remote() && state.offer === session.role);
  $('draw').textContent = state?.offer ? 'Accept draw' : 'Offer draw';
  $('claim').disabled = state?.policy !== 'tournament' || Boolean(state?.outcome) || pending || (remote() && state.side !== session.role);
  $('resign').disabled = !state || Boolean(state.outcome) || pending;
  $('rematch').disabled = !state?.outcome || pending;
  $('invitation').hidden = !session?.invite || Boolean(state);
  $('invite-link').value = session?.invite ? location.origin + '/#join=' + session.id + '.' + session.invite : '';
  for (const button of document.querySelectorAll('.tab[data-panel=analysis], .tab[data-panel=record]')) button.disabled = Boolean(remote());
  renderTurns();
  renderBoard();
  renderClocks();
}

function ingest(next) {
  if (state?.revision !== next?.revision || state?.fen !== next?.fen) path = [];
  state = next;
  clockSample = performance.now();
  if (session?.role === 'Local' && state?.pdn && !state.clock && !state.outcome && state.turns.length) {
    saveStorage(localStorage, 'crownline-game', { pdn: state.pdn, mode: session.mode, policy: state.policy });
  }
  renderGame();
  scheduleBot();
}

async function poll() {
  if (!session || pending || polling || (!remote() && !state?.clock)) return;
  const current = session;
  polling = true;
  try {
    const { state: next } = await request(sessionUrl('state', current), undefined, current);
    if (session !== current || pending) return;
    if (next?.revision !== state?.revision || next?.fen !== state?.fen) ingest(next);
    else { state = next; clockSample = performance.now(); renderClocks(); }
  } catch (problem) {
    if (session === current) error(problem.message);
  } finally { polling = false; }
}

async function localStudy(body, signal, current = session) {
  if (!current || current.role !== 'Local') throw new Error('Private analysis requires a local session');
  return request(sessionUrl('study', current), body, { token: current.token, signal });
}

function scheduleBot() {
  clearTimeout(botTimer);
  if (!state || !botSide(state.side) || state.outcome || state.clock?.paused || pending || replay) return;
  botTimer = setTimeout(safe(async () => {
    const current = session;
    if (!state || !botSide(state.side) || pending || replay) return;
    pending = true;
    renderGame();
    try {
      const { state: next, search } = await localStudy({ op: 'bot', revision: state.revision, command_id: uid(), ...difficulty[$('difficulty').value] });
      if (session !== current) return;
      controls.output.textContent = searchText(search);
      pending = false;
      ingest(next);
    } catch (problem) {
      if (session !== current) return;
      pending = false;
      renderGame();
      await poll();
      throw problem;
    }
  }), session.mode === 'bots' ? 85 : 45);
}

async function act(command, actor) {
  if (pending || !session || !state) return;
  const current = session;
  pending = true;
  clearTimeout(botTimer);
  error();
  try {
    const result = await request(sessionUrl('act'), {
      revision: state.revision, actor, command, command_id: uid(),
    }, current);
    if (session === current) { pending = false; ingest(result.state); }
  } catch (problem) {
    if (session === current) {
      pending = false;
      path = [];
      if (problem.status === 409) await poll();
      renderGame();
    }
    throw problem;
  }
}

async function choose(square) {
  if (!movingAllowed()) return;
  const next = candidates([...path, square]);
  if (next.length) {
    path.push(square);
    const exact = next.find(turn => turn.path.length === path.length);
    if (exact) {
      await act({ kind: 'move', turn: exact.text }, state.side);
      return;
    }
  } else if (path.length < 2) {
    path = candidates([square]).length ? [square] : [];
  }
  renderGame();
}

function setPanel(panel) {
  if (panel !== 'game' && remote()) return;
  for (const button of document.querySelectorAll('.tab')) {
    const active = button.dataset.panel === panel;
    button.classList.toggle('active', active);
    button.setAttribute('aria-current', active ? 'page' : 'false');
    $('panel-' + button.dataset.panel).hidden = !active;
  }
}

function updateMode() {
  const mode = controls.mode.value;
  $('time-control').disabled = mode === 'study';
  $('opening').disabled = mode === 'room';
  $('new-fen').disabled = mode === 'room';
}

async function createGame(mode, options = {}) {
  clearTimeout(botTimer);
  analysisController?.abort();
  const old = session;
  const config = { mode: mode === 'room' ? 'room' : 'local', policy: mode === 'study' ? 'study' : 'house', ...options };
  const result = await request('/api/session', config);
  session = { ...result, mode };
  replay = null;
  record = null;
  state = null;
  pending = false;
  path = [];
  $('variation-tree').replaceChildren();
  $('review-results').replaceChildren();
  if (old?.role === 'Local') request(sessionUrl('close', old), {}, old).catch(() => {});
  saveStorage(sessionStorage, 'crownline-session', { id: session.id, token: session.token, invite: session.invite, role: session.role, mode });
  controls.mode.value = mode;
  updateMode();
  setPanel('game');
  ingest(result.state);
}

function searchText(value) {
  if (!value) return '';
  const depths = value.depths.map(depth => 'd' + depth.depth + '  ' + depth.score + '  ·  ' + depth.nodes.toLocaleString() + ' nodes');
  return [
    value.best ? 'Best turn  ' + value.best.text : 'Terminal position',
    value.score === null ? 'No completed estimate' : 'Mover score  ' + value.score,
    'Line  ' + (value.pv.map(turn => turn.text).join(' ') || "none"),
    'Stop  ' + value.stop.replaceAll('_', ' '),
    'Visited  ' + value.nodes.toLocaleString() + ' nodes', '', ...depths,
  ].join('\n');
}

async function doAnalysis(op) {
  if (!canStudy()) return;
  analysisController?.abort();
  const controller = new AbortController();
  analysisController = controller;
  controls.output.textContent = op === 'witness' ? 'Reading endgame shard…' : 'Searching native engine…';
  const body = op === 'witness' ? { op, fen: currentFen(), max: 20 }
    : { op, fen: replay?.fen, ...difficulty[$('difficulty').value] };
  try {
    const value = await localStudy(body, controller.signal);
    if (analysisController !== controller) return;
    controls.output.textContent = op === 'witness'
      ? value.map((row, i) => String(i + 1).padStart(2, ' ') + '  ' + row.proof.wdl + '  ' + row.position + (row.chosen ? ' → ' + row.chosen : '')).join('\n')
      : searchText(value);
  } catch (problem) {
    if (problem.name !== 'AbortError') controls.output.textContent = problem.message;
  }
}

function showPosition(fen, turnPath = [], annotations = []) {
  replay = { fen, path: turnPath };
  path = [];
  clearTimeout(botTimer);
  $('record-info').textContent = [fen, ...annotations].join(' · ');
  renderGame();
}

function renderTree() {
  const tree = $('variation-tree');
  tree.replaceChildren();
  if (!record) return;
  function button(text, action) {
    const el = document.createElement('button');
    el.type = 'button';
    el.textContent = text;
    el.addEventListener('click', action);
    return el;
  }
  tree.append(button('Start', () => showPosition(record.start, [], record.line.introduction)));
  tree.append(button('Return to live board', () => { replay = null; renderGame(); scheduleBot(); }));
  function branch(line, container) {
    for (const ply of line.plies) {
      const annotations = ply.annotations.flatMap(item => item.comment || item.glyph ? [item.comment || item.glyph] : []);
      container.append(button(ply.turn.text, () => showPosition(ply.after, ply.turn.path, annotations)));
      for (const item of ply.annotations) {
        if (!item.variation) continue;
        const details = document.createElement('details');
        const label = document.createElement('summary');
        label.textContent = 'Alternative to ' + ply.turn.text;
        details.append(label);
        branch(item.variation, details);
        container.append(details);
      }
    }
  }
  branch(record.line, tree);
}

async function importPdn(partial) {
  if (!canStudy()) return;
  record = await localStudy({ op: 'import', text: controls.pdn.value, partial });
  controls.pdn.value = record.pdn;
  showPosition(record.start, [], record.line.introduction);
  $('record-info').textContent = record.diagnostic
    ? 'Prefix only at byte ' + record.diagnostic.offset + ' · ' + record.diagnostic.message
    : record.line.plies.length + ' main-line turns · Source SHA-256 ' + record.sha256.slice(0, 16);
  renderTree();
}

function reviewGraph(rows) {
  const ns = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(ns, 'svg');
  svg.setAttribute('viewBox', '0 0 500 94');
  svg.setAttribute('role', 'img');
  svg.setAttribute('aria-label', 'Black advantage by played turn, clipped to five men of material');
  svg.classList.add('review-graph');
  const baseline = document.createElementNS(ns, 'line');
  for (const [attr, value] of Object.entries({ x1: 12, x2: 488, y1: 47, y2: 47, stroke: '#657668' })) baseline.setAttribute(attr, value);
  svg.append(baseline);
  const dots = rows.filter(row => row.black_score_after !== null).map((row, i) => {
    const x = 12 + i * 476 / Math.max(1, rows.length - 1);
    const y = 47 - Math.max(-500, Math.min(500, row.black_score_after)) * 0.066;
    return x + ',' + y;
  });
  const plot = document.createElementNS(ns, 'polyline');
  for (const [attr, value] of Object.entries({ points: dots.join(' '), fill: 'none', stroke: '#e7bd77', 'stroke-width': 2 })) plot.setAttribute(attr, value);
  svg.append(plot);
  return svg;
}

async function reviewPdn() {
  if (!canStudy()) return;
  setPanel('analysis');
  controls.output.textContent = 'Replaying and scoring every full turn…';
  let result;
  try {
    result = await localStudy({ op: 'review', text: controls.pdn.value || state?.pdn || '', nodes: 1000 });
  } catch (problem) {
    controls.output.textContent = problem.message;
    return;
  }
  const output = $('review-results');
  output.replaceChildren(reviewGraph(result.rows));
  controls.output.textContent = result.rows.length + ' verified turns\n' + result.rules;
  for (const row of result.rows) {
    const detail = document.createElement('details');
    detail.className = 'review-row';
    const summary = document.createElement('summary');
    summary.textContent = row.ply + '. ' + row.played + ' · ' + row.classification.replaceAll('_', ' ');
    detail.append(summary);
    const inspect = document.createElement('button');
    inspect.textContent = 'Show position';
    inspect.addEventListener('click', () => showPosition(row.before));
    detail.append(inspect);
    const list = document.createElement('ul');
    for (const move of row.candidates) {
      const li = document.createElement('li');
      const scores = move.shallow === null ? 'unscored' : 'estimate ' + move.shallow + ' / ' + move.deep;
      li.textContent = move.turn + ' · ' + (move.proof_for_mover ? 'board proof ' + move.proof_for_mover + ' · ' : '')
        + scores + (move.continuation.length ? ' · ' + move.continuation.join(' ') : '');
      list.append(li);
    }
    detail.append(list);
    output.append(detail);
  }
}

controls.board.addEventListener('click', safe(async event => {
  const square = Number(event.target.closest('[data-square]')?.dataset.square);
  if (square) { focusSquare = square; await choose(square); }
}));
controls.board.addEventListener('keydown', event => {
  if (event.key === 'Escape' && path.length) { path = []; renderGame(); return; }
  const square = Number(event.target.dataset.square);
  if (square && (event.key === 'Enter' || event.key === ' ')) {
    event.preventDefault();
    focusSquare = square;
    safe(() => choose(square))();
    return;
  }
  const direction = { ArrowLeft: [0,-2], ArrowRight: [0,2], ArrowUp: [-1,0], ArrowDown: [1,0] }[event.key];
  if (!direction || !square) return;
  event.preventDefault();
  const [row, col] = coords(square);
  const sign = flipped ? -1 : 1;
  const r = row + direction[0] * sign;
  const c = col + direction[1] * sign;
  const landing = squareAt(r, c) || (direction[0] && (squareAt(r, c + sign) || squareAt(r, c - sign)));
  if (!landing) return;
  event.target.tabIndex = -1;
  const target = controls.board.querySelector('[data-square="' + landing + '"]');
  target.tabIndex = 0;
  target.focus();
  focusSquare = landing;
});
controls.board.addEventListener('dragstart', event => {
  dragged = Number(event.target.closest('[data-square]')?.dataset.square);
  if (dragged) { event.dataTransfer.setData('text/plain', String(dragged)); event.dataTransfer.effectAllowed = 'move'; }
});
controls.board.addEventListener('dragover', event => {
  if (dragged) { event.preventDefault(); event.dataTransfer.dropEffect = 'move'; }
});
controls.board.addEventListener('dragend', () => { dragged = undefined; });
controls.board.addEventListener('drop', safe(async event => {
  if (!dragged || !movingAllowed()) return;
  event.preventDefault();
  const landing = Number(event.target.closest('[data-square]')?.dataset.square);
  if (landing && candidates([dragged]).length) { path = [dragged]; await choose(landing); }
  dragged = undefined;
}));

for (const button of document.querySelectorAll('.tab')) button.addEventListener('click', () => setPanel(button.dataset.panel));
$('flip').addEventListener('click', () => { flipped = !flipped; renderBoard(); renderClocks(); });
$('numbers').addEventListener('click', () => { numbers = !numbers; $('numbers').setAttribute('aria-pressed', String(numbers)); renderBoard(); });
$('zoom').addEventListener('change', () => { controls.board.dataset.zoom = $('zoom').value; });
controls.mode.addEventListener('change', updateMode);
$('new-game').addEventListener('submit', safe(async event => {
  event.preventDefault();
  const mode = controls.mode.value;
  const [initial_ms, increment_ms] = $('time-control').value.split(',').map(Number);
  await createGame(mode, {
    clock: mode !== 'study' && initial_ms ? { initial_ms, increment_ms } : null,
    fen: mode !== 'room' ? $('new-fen').value.trim() || null : null,
    opening: mode !== 'room' ? $('opening').value.split(' ').filter(Boolean) : null,
  });
  clearStorage(localStorage, 'crownline-game');
}));
$('copy-invite').addEventListener('click', safe(async () => {
  if (navigator.clipboard) await navigator.clipboard.writeText($('invite-link').value);
  else { $('invite-link').select(); document.execCommand('copy'); }
}));
$('takeback').addEventListener('click', safe(() => act({ kind: 'takeback' }, state.side)));
$('pause').addEventListener('click', safe(() => act({ kind: state.clock?.paused ? 'resume' : 'pause' }, state.side)));
$('draw').addEventListener('click', safe(() => act({ kind: state.offer ? 'accept_draw' : 'offer_draw' }, state.offer ? opposite(state.offer) : state.side)));
$('claim').addEventListener('click', safe(async () => {
  if (state.quiet_plies >= 80) return act({ kind: 'claim_no_progress' }, state.side);
  const turn = window.prompt('Enter the full turn that would create a third occurrence.');
  if (turn) await act({ kind: 'claim_repetition', turn }, state.side);
}));
$('resign').addEventListener('click', safe(() => act({ kind: 'resign' }, remote() ? session.role : session.mode === 'bot' ? 'Black' : session.mode === 'bot-black' ? 'White' : state.side)));
$('rematch').addEventListener('click', safe(async () => {
  const result = await request(sessionUrl('rematch'), {}, session);
  ingest(result.state);
  if (result.waiting) controls.hint.textContent = 'Rematch requested. Waiting for the other player.';
}));
$('analyze').addEventListener('click', () => doAnalysis('analyze'));
$('witness').addEventListener('click', () => doAnalysis('witness'));
$('cancel').addEventListener('click', () => {
  analysisController?.abort();
  analysisController = undefined;
  controls.output.textContent = 'Display cancelled. The native request has a hard deadline.';
});
$('import').addEventListener('click', safe(() => importPdn(false)));
$('import-partial').addEventListener('click', safe(() => importPdn(true)));
$('export').addEventListener('click', () => {
  const text = (replay ? record?.pdn : state?.pdn) || state?.pdn;
  if (text) { controls.pdn.value = text; $('record-info').textContent = 'Canonical PDN ready to copy'; }
});
$('review').addEventListener('click', safe(reviewPdn));

async function initialize() {
  updateMode();
  const invite = location.hash.match(/^#join=([\w-]{16})\.([\w-]{43})$/);
  if (invite) {
    const result = await request('/api/session/' + invite[1] + '/join', { invite: invite[2] });
    history.replaceState(null, '', '/');
    session = { ...result, mode: 'room' };
    flipped = true;
    saveStorage(sessionStorage, 'crownline-session', { id: session.id, token: session.token, role: session.role, mode: 'room' });
    ingest(result.state);
    return;
  }
  const previous = readStorage(sessionStorage, 'crownline-session');
  if (previous) {
    try {
      const result = await request(sessionUrl('state', previous), undefined, previous);
      session = previous;
      flipped = session.role === 'White';
      controls.mode.value = session.mode;
      updateMode();
      ingest(result.state);
      return;
    } catch { clearStorage(sessionStorage, 'crownline-session'); }
  }
  const saved = readStorage(localStorage, 'crownline-game');
  await createGame(saved?.mode || 'study');
  if (saved?.pdn) {
    pending = true;
    clearTimeout(botTimer);
    try { const next = await localStudy({ op: 'restore', text: saved.pdn, policy: saved.policy }); ingest(next); }
    finally { pending = false; renderGame(); scheduleBot(); }
  }
}

setInterval(renderClocks, 125);
setInterval(poll, 850);
safe(initialize)();
