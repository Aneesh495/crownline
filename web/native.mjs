import { spawn } from 'node:child_process';

const MAGIC = Buffer.from('CL01');
const MAX_INPUT = 384 * 1024;
const MAX_OUTPUT = 4 * 1024 * 1024;
const MAX_PENDING = 12;

export class NativeSession {
  #child;
  #waiting = [];
  #current;
  #partial = Buffer.alloc(0);
  #closed;
  #alarm;

  constructor(engine, table) {
    this.#child = spawn(engine, ['wire', ...(table ? [table] : [])], {
      stdio: ['pipe', 'pipe', 'inherit'],
      windowsHide: true,
    });
    this.#child.on('error', error => this.close(error.message));
    this.#child.on('exit', () => this.close('Native session exited'));
    this.#child.stdin.on('error', error => this.close(error.message));
    this.#child.stdout.on('data', chunk => this.#onData(chunk));
  }

  get closed() {
    return Boolean(this.#closed);
  }

  request(message) {
    if (this.#closed) return Promise.reject(new Error(this.#closed));
    if (this.#waiting.length >= MAX_PENDING) return Promise.reject(new Error('Native session is busy'));
    const body = Buffer.from(JSON.stringify(message));
    if (body.length > MAX_INPUT) return Promise.reject(new Error('Request exceeds native frame limit'));
    const frame = Buffer.allocUnsafe(8 + body.length);
    MAGIC.copy(frame);
    frame.writeUInt32LE(body.length, 4);
    body.copy(frame, 8);
    return new Promise((resolve, reject) => {
      this.#waiting.push({ frame, resolve, reject });
      this.#advance();
    });
  }

  #advance() {
    if (this.#current || this.#closed) return;
    this.#current = this.#waiting.shift();
    if (!this.#current) return;
    this.#alarm = setTimeout(() => this.close('Native command exceeded its deadline'), 20_000);
    this.#alarm.unref();
    this.#child.stdin.write(this.#current.frame);
  }

  #onData(chunk) {
    if (this.#closed) return;
    this.#partial = Buffer.concat([this.#partial, chunk]);
    if (this.#partial.length < 8) return;
    const size = this.#partial.readUInt32LE(4);
    if (!this.#current || !this.#partial.subarray(0, 4).equals(MAGIC) || size > MAX_OUTPUT || size === 0) {
      this.close('Invalid response frame');
      return;
    }
    if (this.#partial.length < size + 8) return;
    try {
      if (this.#partial.length !== size + 8) throw new Error('Unexpected extra frame data');
      const response = JSON.parse(this.#partial.subarray(8).toString('utf8'));
      if (typeof response.ok !== 'boolean') throw new Error('Missing response status');
      const current = this.#current;
      this.#current = undefined;
      this.#partial = Buffer.alloc(0);
      clearTimeout(this.#alarm);
      if (response.ok) current.resolve(response.value);
      else current.reject(new Error(response.error || 'Native request failed'));
      this.#advance();
    } catch {
      this.close('Malformed response frame');
    }
  }

  close(reason = 'Session closed') {
    if (this.#closed) return;
    this.#closed = reason;
    clearTimeout(this.#alarm);
    this.#child.kill();
    this.#current?.reject(new Error(reason));
    for (const request of this.#waiting) request.reject(new Error(reason));
    this.#waiting = [];
    this.#current = undefined;
  }
}
