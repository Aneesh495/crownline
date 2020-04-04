import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { setTimeout as wait } from 'node:timers/promises';
import { launch } from '../server.mjs';

const output = process.argv[2] && resolve(process.argv[2]);
const table = process.argv[3];
const chromium = process.env.CROWNLINE_CHROME;
if (!output || !table || !chromium) throw new Error('Usage: CROWNLINE_CHROME=/path/to/chrome node web/tests/browser.mjs OUTPUT TABLE');
await mkdir(output, { recursive: true });
const profile = await mkdtemp(join(output, '.chrome-'));
const server = await launch({ port: 0, table });
const origin = 'http://127.0.0.1:' + server.address().port;
const chrome = spawn(chromium, [
  '--headless', '--disable-gpu', '--no-first-run', '--disable-extensions',
  '--disable-background-networking', '--no-default-browser-check', '--remote-debugging-port=0',
  '--user-data-dir=' + profile, 'about:blank',
], { stdio: ['ignore', 'ignore', 'pipe'] });
let chromeError = '';
chrome.stderr.on('data', chunk => { chromeError = (chromeError + chunk).slice(-3500); });
const observations = [];
let ws;
let outstanding = new Map();
let id = 0;

async function until(check, label) {
  for (let i = 0; i < 300; i += 1) {
    const result = await check();
    if (result) return result;
    await wait(45);
  }
  throw new Error('Timed out: ' + label);
}

function command(method, params = {}) {
  const next = ++id;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { outstanding.delete(next); reject(new Error('CDP timeout: ' + method)); }, 4000);
    outstanding.set(next, result => {
      clearTimeout(timer);
      if (result.error) reject(new Error(result.error.message));
      else resolve(result.result);
    });
    ws.send(JSON.stringify({ id: next, method, params }));
  });
}

async function evaluate(expression) {
  const result = await command('Runtime.evaluate', { expression, returnByValue: true });
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
  return result.result.value;
}

async function click(selector) {
  const hit = await evaluate('(() => { const node = document.querySelector(' + JSON.stringify(selector)
    + '); if (!node || node.disabled) return false; node.click(); return true; })()');
  assert.ok(hit, selector);
}

async function key(key, code, windowsVirtualKeyCode) {
  const args = { key, code, windowsVirtualKeyCode };
  await command('Input.dispatchKeyEvent', { ...args, type: 'keyDown' });
  await command('Input.dispatchKeyEvent', { ...args, type: 'keyUp' });
}

async function shot(name) {
  const result = await command('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
  await writeFile(join(output, name), Buffer.from(result.data, 'base64'));
}

async function resize(width, height, mobile) {
  await command('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile });
}

try {
  const port = await until(async () => {
    try { return Number((await readFile(join(profile, 'DevToolsActivePort'), 'utf8')).split('\n')[0]); }
    catch { return null; }
  }, 'Chrome debugger');
  const pages = await (await fetch('http://127.0.0.1:' + port + '/json/list')).json();
  ws = new WebSocket(pages.find(page => page.type === 'page').webSocketDebuggerUrl);
  ws.addEventListener('message', message => {
    const data = JSON.parse(message.data);
    if (data.id && outstanding.has(data.id)) {
      const finish = outstanding.get(data.id);
      outstanding.delete(data.id);
      finish(data);
    } else if (data.method === 'Runtime.exceptionThrown') {
      observations.push({ exception: data.params.exceptionDetails.text });
    }
  });
  await new Promise((resolve, reject) => {
    ws.addEventListener('open', resolve, { once: true });
    ws.addEventListener('error', reject, { once: true });
  });
  await command('Runtime.enable');
  await command('Page.enable');
  await resize(1300, 1070, false);
  await command('Page.navigate', { url: origin });
  await until(() => evaluate('document.querySelectorAll("[data-square]").length === 32'), 'initial board');
  assert.equal(await evaluate('document.querySelectorAll(".piece").length'), 24);
  assert.ok(await evaluate('[...document.querySelectorAll(".square")].every(tile => { const box = tile.getBoundingClientRect(); return Math.abs(box.width - box.height) <= 1; })'));
  await evaluate('document.querySelector("[data-square=\\"9\\"]").focus()');
  await key('Enter', 'Enter', 13);
  await key('ArrowDown', 'ArrowDown', 40);
  const target = await evaluate('document.activeElement.dataset.square');
  assert.equal(target, '14');
  await key('Enter', 'Enter', 13);
  await until(() => evaluate('document.querySelector("#revision").textContent === "1 ply"'), 'keyboard move');
  await command('Page.reload');
  await until(() => evaluate('document.querySelector("#revision")?.textContent === "1 ply"'), 'session reload');
  observations.push({ keyboard_move: '9-14', reloaded_ply: 1, viewport: [1300, 1070] });
  await shot('board-desktop.png');

  await click('[data-panel=record]');
  const pdn = await readFile(new URL('../../examples/branch-study.pdn', import.meta.url), 'utf8');
  await evaluate('document.querySelector("#pdn").value = ' + JSON.stringify(pdn));
  await click('#import');
  await until(() => evaluate('document.querySelector("#variation-tree details")'), 'legal variation tree');
  await click('#review');
  await until(() => evaluate('document.querySelector("#review-results").textContent.includes("board only lost draw")'), 'proof-backed review');
  assert.equal(await evaluate('document.querySelector("#review-results").textContent.includes("heuristic drop")'), true);
  await click('.review-row summary');
  await click('.review-row:nth-of-type(2) summary');
  await shot('board-review.png');
  observations.push({ review: ['heuristic_drop', 'board_only_lost_draw'], input: 'examples/branch-study.pdn' });

  await resize(390, 844, true);
  await click('[data-panel=game]');
  assert.ok(await evaluate('document.documentElement.scrollWidth <= innerWidth'));
  await click('#flip');
  assert.equal(await evaluate('document.querySelector("[data-square]").dataset.square'), '32');
  await shot('board-mobile.png');
  observations.push({ viewport: [390, 844], overflow: false, flipped_first_square: 32 });

  await evaluate('document.querySelector("#mode").value = "local"; document.querySelector("#new-fen").value = "B:BK14:W10,11,18,19"');
  await click('#new-game button[type=submit]');
  await until(() => evaluate('document.querySelector("#capture-label").textContent === "Capture required"'), 'forced capture');
  await click('[data-square="14"]');
  await click('[data-square="7"]');
  assert.equal(await evaluate('document.querySelector("#revision").textContent'), '0 ply');
  assert.equal(await evaluate('document.querySelectorAll(".piece.taken").length'), 1);
  for (const square of [16, 23, 14]) await click('[data-square="' + square + '"]');
  await until(() => evaluate('document.querySelector("#status").textContent === "Black wins"'), 'return-to-origin capture');
  observations.push({ full_capture: '14x7x16x23x14', intermediate_turns: 0, outcome: 'Black wins' });

  await click('[data-panel=analysis]');
  await click('#analyze');
  await click('#cancel');
  assert.equal(await evaluate('document.querySelector("#analysis-output").textContent.includes("cancelled")'), true);
  observations.push({ cancellation: 'browser aborted display; native hard limit active' });
  assert.equal(observations.filter(row => row.exception).length, 0);
  await writeFile(join(output, 'browser-observations.json'), JSON.stringify(observations, null, 2) + '\n');
  console.log(JSON.stringify(observations, null, 2));
} catch (error) {
  if (chromeError) console.error(chromeError);
  throw error;
} finally {
  ws?.close();
  if (chrome.exitCode === null && chrome.signalCode === null) {
    const exited = new Promise(resolve => chrome.once('close', resolve));
    chrome.kill();
    await exited;
  }
  server.closeAllConnections();
  await new Promise(resolve => server.close(resolve));
  await rm(profile, { recursive: true, force: true });
}
