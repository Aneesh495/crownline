import { after, test } from 'node:test';
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { open, readFile } from 'node:fs/promises';
import { withServer } from './support.mjs';

const fixture = JSON.parse(await readFile(new URL('./fixtures/completed-game.json', import.meta.url)));
const clocks = [
  null,
  { initial_ms: 180_000, increment_ms: 2_000 },
  { initial_ms: 300_000, increment_ms: 3_000 },
  { initial_ms: 900_000, increment_ms: 10_000 },
];
const levels = {
  quick: { millis: 45, nodes: 1500, depth: 5 },
  club: { millis: 180, nodes: 12_000, depth: 10 },
  deep: { millis: 700, nodes: 95_000, depth: 22 },
};
const other = side => side === 'Black' ? 'White' : 'Black';

const ledger = process.env.CROWNLINE_PLAYTHROUGH_LEDGER
  ? await open(process.env.CROWNLINE_PLAYTHROUGH_LEDGER, 'wx', 0o600)
  : null;
after(async () => { await ledger?.close(); });

async function recordGame(t, game) {
  t.diagnostic(JSON.stringify({ mode: game.mode, turns: game.turns, outcome: game.outcome }));
  if (ledger) await ledger.writeFile(JSON.stringify(game) + '\n');
}


for (const remote of [false, true]) {
  for (const clock of clocks) {
    const label = (remote ? 'invite room' : 'local players') + ' ' + (clock ? clock.initial_ms : 'unclocked');
    test('complete signed 42-turn game through ' + label, async t => {
      const { api } = await withServer(t);
      const created = await api('/api/session', { mode: remote ? 'room' : 'local', policy: 'house', clock });
      assert.equal(created.status, 201);
      const host = created.json;
      const path = '/api/session/' + host.id;
      const guest = remote ? (await api(path + '/join', { invite: host.invite })).json : host;
      const tokens = { Black: host.token, White: guest.token };
      let state = remote ? guest.state : host.state;
      for (const [ply, turn] of fixture.turns.entries()) {
        assert.ok(state.legal.some(move => move.text === turn), label + ', turn ' + (ply + 1));
        const body = { revision: state.revision, actor: state.side, command_id: randomUUID(), command: { kind: 'move', turn } };
        const moved = await api(path + '/act', body, tokens[state.side]);
        assert.equal(moved.status, 200, moved.json.error);
        if (ply === 19) {
          assert.deepEqual(await api(path + '/act', body, tokens[state.side]), moved);
          const rejoined = await api(path + '/state', undefined, guest.token);
          assert.equal(rejoined.json.state.fen, moved.json.state.fen);
          assert.equal(rejoined.json.state.revision, moved.json.state.revision);
        }
        state = moved.json.state;
        assert.equal(state.turns.length, ply + 1);
        if (remote && !state.outcome) assert.equal(state.pdn, null);
      }
      assert.deepEqual(state.outcome, fixture.outcome);
      assert.equal(state.legal.length, 0);
      assert.ok(state.pdn.includes('23x14x5'));
      assert.ok(state.pdn.trimEnd().endsWith('1-0'));
      if (remote) assert.equal((await api(path + '/study', { op: 'analyze' }, guest.token)).status, 403);
      await recordGame(t, { mode: label, turns: state.turns.length, outcome: state.outcome,
        clock, pdn: state.pdn, fixture_game: fixture.game_id, fixture_source: fixture.source_commit });
    });
  }
}

for (const [level, limits] of Object.entries(levels)) {
  for (const engineSide of ['Black', 'White', 'both']) {
    for (const clock of clocks) {
      const label = level + ', engine ' + engineSide + ', ' + (clock ? clock.initial_ms + '+' + clock.increment_ms : 'unclocked');
      test('finish a live game with ' + label, { timeout: 35_000 }, async t => {
        const { api } = await withServer(t);
        const created = await api('/api/session', { mode: 'local', policy: 'house', clock });
        const player = created.json;
        const path = '/api/session/' + player.id;
        let state = player.state;
        for (let ply = 0; ply < 320 && !state.outcome; ply += 1) {
          const revision = state.revision;
          if (engineSide === 'both' || state.side === engineSide) {
            const moved = await api(path + '/study', { op: 'bot', revision, command_id: randomUUID(), ...limits }, player.token);
            assert.equal(moved.status, 200, moved.json.error);
            const chosen = moved.json.state.turns.at(-1);
            assert.ok(state.legal.some(turn => turn.text === chosen.text));
            state = moved.json.state;
          } else {
            const turn = state.legal[0].text;
            const moved = await api(path + '/act', {
              revision, actor: state.side, command_id: randomUUID(), command: { kind: 'move', turn },
            }, player.token);
            assert.equal(moved.status, 200, moved.json.error);
            state = moved.json.state;
          }
          assert.ok(state.revision > revision);
        }
        assert.ok(state.outcome, 'unfinished ' + label);
        assert.ok(['elimination', 'immobility', 'repetition', 'no_progress'].includes(state.outcome.reason));
        assert.ok(state.turns.length > 3);
        assert.equal(state.legal.length, 0);
        assert.ok(state.pdn);
        const imported = await api(path + '/study', { op: 'import', text: state.pdn, partial: false }, player.token);
        assert.equal(imported.status, 200, imported.json.error);
        assert.equal(imported.json.line.plies.length, state.turns.length);
        await recordGame(t, { mode: label, turns: state.turns.length, outcome: state.outcome,
          human_seat: engineSide === 'both' ? null : other(engineSide), clock, limits, pdn: state.pdn });
      });
    }
  }
}
