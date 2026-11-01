#!/usr/bin/env node
// Node.js >= 22; no npm dependencies. Uses an existing Trunk dist directory.
import { createServer } from 'node:http';
import { readFile, writeFile, mkdtemp, rm, stat, access, rename, readdir } from 'node:fs/promises';
import { join, resolve, dirname, extname, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';
import { tmpdir, homedir } from 'node:os';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
if (args.includes('--help')) {
  console.log('Usage: node scripts/capture_readme.mjs [--dist PATH] [--browser PATH]\nUses assets/examples/Example.ketchup with an existing Web build; creates assets/screenshots/ketchup.jpg and updates README.md.\nRequires Node.js >= 22 and Chromium/Chrome (or CHROME_BIN). No build is started.');
  process.exit(0);
}
function option(name) {
  const i = args.indexOf(name);
  if (i < 0) return undefined;
  if (!args[i + 1] || args[i + 1].startsWith('--')) throw Error(`Missing value for ${name}`);
  return args[i + 1];
}
for (let i = 0; i < args.length; i += 2) {
  if (!['--dist', '--browser'].includes(args[i])) throw Error(`Unknown option: ${args[i]}`);
}
const dist = resolve(root, option('--dist') || 'dist');
const example = await readFile(join(root, 'assets/examples/Example.ketchup'), 'utf8');
// Fail before launching the browser if the bundled project is invalid JSON.
JSON.parse(example);
await access(join(dist, 'index.html'));
const cache = join(homedir(), '.cache/ms-playwright');
const cachedBrowsers = (await readdir(cache).catch(() => []))
  .filter(name => /^chromium-\d+$/.test(name)).sort().reverse()
  .flatMap(name => ['chrome-linux64/chrome', 'chrome-linux/chrome'].map(path => join(cache, name, path)));
const explicitBrowser = option('--browser') || process.env.CHROME_BIN;
const candidates = explicitBrowser ? [explicitBrowser] : [
  '/usr/bin/chromium', '/usr/bin/chromium-browser', '/usr/bin/google-chrome',
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  ...cachedBrowsers];
let executable;
for (const candidate of candidates) { try { await access(candidate); executable = candidate; break; } catch {} }
if (!executable) throw Error('Chromium not found. Set CHROME_BIN or pass --browser /path/to/chrome.');
const profile = await mkdtemp(join(tmpdir(), 'ketchup-readme-'));
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.svg': 'image/svg+xml', '.png': 'image/png', '.json': 'application/json' };
const server = createServer(async (req, res) => {
  try {
    const path = resolve(dist, '.' + decodeURIComponent(new URL(req.url, 'http://localhost').pathname));
    if (!path.startsWith(dist + sep) && path !== dist) throw Error('Invalid path');
    const file = (await stat(path)).isDirectory() ? join(path, 'index.html') : path;
    res.setHeader('Content-Type', mime[extname(file)] || 'application/octet-stream');
    res.end(await readFile(file));
  } catch { res.writeHead(404); res.end('Not found'); }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const browser = spawn(executable, ['--headless=new', '--no-sandbox', '--disable-dev-shm-usage',
  '--enable-unsafe-swiftshader', '--force-device-scale-factor=1', '--window-size=1920,1080',
  '--remote-debugging-port=0', `--user-data-dir=${profile}`, 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] });
let diagnostics = '';
browser.stderr.on('data', chunk => { diagnostics = (diagnostics + chunk).slice(-12000); });
browser.on('error', error => { diagnostics += error.message; });
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(fn, label, timeout = 60000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) { const value = await fn(); if (value) return value; await pause(200); }
  throw Error(`Timed out waiting for ${label}\n${diagnostics}`);
}
let socket;
try {
  const port = await until(async () => {
    try { return (await readFile(join(profile, 'DevToolsActivePort'), 'utf8')).split('\n')[0]; } catch { return null; }
  }, 'Chromium startup');
  const tabs = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  socket = new WebSocket(tabs.find(tab => tab.type === 'page').webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  let sequence = 0;
  const pending = new Map();
  socket.onmessage = event => {
    const message = JSON.parse(event.data);
    if (message.id) {
      const entry = pending.get(message.id); pending.delete(message.id);
      if (message.error) entry.reject(Error(JSON.stringify(message.error))); else entry.resolve(message.result);
    }
  };
  const call = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++sequence;
    const timer = setTimeout(() => { pending.delete(id); reject(Error(`CDP timeout: ${method}`)); }, 60000);
    pending.set(id, { resolve: value => { clearTimeout(timer); resolve(value); }, reject: error => { clearTimeout(timer); reject(error); } });
    socket.send(JSON.stringify({ id, method, params }));
  });
  await call('Page.enable');
  await call('Emulation.setDeviceMetricsOverride', { width: 1920, height: 1080, deviceScaleFactor: 1, mobile: false });
  await call('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: 'light' }] });
  // Seed only the temporary browser profile, before the app restores its document.
  // Keep the bundled project's saved camera and display settings intact.
  await call('Page.addScriptToEvaluateOnNewDocument', {
    source: `localStorage.setItem('ketchup.document.v1', ${JSON.stringify(example)});`,
  });
  await call('Page.navigate', { url: `http://127.0.0.1:${server.address().port}/` });
  await until(async () => {
    const result = await call('Runtime.evaluate', { expression: `!document.getElementById('loading') && document.getElementById('ketchup-canvas')?.width === 1920` });
    return result.result.value;
  }, 'the loaded KetchUp canvas', 120000);
  // Dismiss the first-launch release notes through the application's keyboard handling.
  await call('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 });
  await call('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 });
  await call('Input.dispatchMouseEvent', { type: 'mouseMoved', x: 0, y: 1079 });
  // Require a stable rendered frame, rather than a fixed startup delay.
  let previous;
  const shot = await until(async () => {
    const { data } = await call('Page.captureScreenshot', { format: 'jpeg', quality: 92, captureBeyondViewport: false });
    const stable = previous === data && Buffer.from(data, 'base64').length > 20000;
    previous = data;
    return stable && data;
  }, 'a stable rendered frame');
  const image = join(root, 'assets/screenshots/ketchup.jpg');
  const readme = join(root, 'README.md');
  const text = await readFile(readme, 'utf8');
  const link = '![KetchUp editor screenshot](assets/screenshots/ketchup.jpg)';
  const updated = /!\[KetchUp editor screenshot\]\([^)]*\)/.test(text)
    ? text.replace(/!\[KetchUp editor screenshot\]\([^)]*\)/, link)
    : text.replace(/^(# KetchUp[^\n]*\n)/m, `$1\n${link}\n`);
  if (!updated.includes(link)) throw Error('README title not found; nothing changed.');
  await writeFile(image + '.tmp', Buffer.from(shot, 'base64'));
  await rename(image + '.tmp', image);
  if (updated !== text) await writeFile(readme, updated);
  console.log(`Captured loaded KetchUp at 1920 × 1080: ${image}\nREADME image link is up to date.`);
} finally {
  socket?.close();
  browser.kill();
  await new Promise(resolve => { if (browser.exitCode !== null) resolve(); else { browser.once('exit', resolve); setTimeout(() => { browser.kill('SIGKILL'); resolve(); }, 3000).unref(); } });
  await new Promise(resolve => server.close(resolve));
  await rm(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
}
