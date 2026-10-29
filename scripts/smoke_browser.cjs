// Real extension integration in a disposable Chromium profile; never uses a personal browser.
// Requires Playwright. Set EXFETCH_PLAYWRIGHT_MODULE to an installed module path if needed.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const http = require('node:http');
const net = require('node:net');
const readline = require('node:readline');
const { spawn, execFile } = require('node:child_process');
const { promisify } = require('node:util');
const { chromium } = require(process.env.EXFETCH_PLAYWRIGHT_MODULE || 'playwright');

async function unusedPort() {
  const server = net.createServer();
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port;
  await new Promise(resolve => server.close(resolve));
  return port;
}

async function main() {
  const binary = path.resolve(process.argv[2] || 'target/debug/exfetch');
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'exfetch-browser-'));
  let browser, child, fixture;
  try {
    const port = await unusedPort();
    const extension = path.join(root, 'extension');
    fs.cpSync(path.resolve(__dirname, '../extension/chrome'), extension, { recursive: true });
    const background = path.join(extension, 'background.js');
    const original = fs.readFileSync(background, 'utf8');
    assert(original.includes('const PORT_RANGE_START = 9876;'));
    assert(original.includes('const PORT_RANGE_END = 9886;'));
    // Scope discovery in this TEST COPY so it cannot attach to an existing live bridge.
    fs.writeFileSync(background, original
      .replace('const PORT_RANGE_START = 9876;', `const PORT_RANGE_START = ${port};`)
      .replace('const PORT_RANGE_END = 9886;', `const PORT_RANGE_END = ${port};`));
    fs.writeFileSync(path.join(root, 'config.toml'), '[policy]\nblocked_domains = ["localhost"]\n');
    fixture = http.createServer((request, response) => {
      if (request.url === '/redirect') {
        response.writeHead(302, { Location: `http://localhost:${fixture.address().port}/blocked` });
        response.end();
        return;
      }
      response.writeHead(200, { 'Content-Type': 'text/html', 'Set-Cookie': 'canary=synthetic; Path=/' });
      response.end('<!doctype html><title>exfetch integration</title><p id="rendered">initial</p>' +
        '<button id="change" onclick="document.querySelector(\'#rendered\').textContent=\'clicked-canary\'">Change</button>' +
        '<script>document.querySelector("#rendered").textContent="rendered-canary"</script>');
    });
    await new Promise(resolve => fixture.listen(0, '127.0.0.1', resolve));
    const url = `http://127.0.0.1:${fixture.address().port}/`;
    const env = { ...process.env, EXFETCH_CONFIG_DIR: root };
    child = spawn(binary, ['serve', '--mcp-stdio', '--port', String(port)], { env, stdio: ['pipe', 'pipe', 'pipe'] });
    let errors = '';
    child.stderr.on('data', chunk => { errors = (errors + chunk).slice(-12000); });
    let serial = 0;
    const pending = new Map();
    readline.createInterface({ input: child.stdout }).on('line', line => {
      const message = JSON.parse(line);
      if (pending.has(message.id)) pending.get(message.id)(message);
    });
    const request = (method, params) => new Promise((resolve, reject) => {
      const id = ++serial;
      const timeout = setTimeout(() => { pending.delete(id); reject(new Error(`Timeout: ${method}\n${errors}`)); }, 45000);
      pending.set(id, message => { clearTimeout(timeout); pending.delete(id); resolve(message); });
      child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
    });
    const tool = (name, args = {}) => request('tools/call', { name, arguments: args });
    const initialized = await request('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'exfetch-browser-integration', version: '1' } });
    assert(initialized.result);
    child.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n');
    browser = await chromium.launchPersistentContext(path.join(root, 'profile'), {
      channel: 'chromium', headless: true,
      executablePath: process.env.EXFETCH_BROWSER_EXECUTABLE || undefined,
      args: [`--disable-extensions-except=${extension}`, `--load-extension=${extension}`],
    });
    let connected = false;
    for (let attempt = 0; attempt < 60; attempt++) {
      const status = await tool('connection_status');
      const data = JSON.parse(status.result.content[0].text);
      if (data.extension_connected) { connected = true; break; }
      await new Promise(resolve => setTimeout(resolve, 250));
    }
    assert(connected, 'real browser extension must authenticate with isolated bridge');
    const fetched = await tool('fetch_page', { url, use_browser: true, selector: '#rendered', format: 'text' });
    assert(!fetched.error && JSON.stringify(fetched).includes('rendered-canary'), JSON.stringify(fetched));
    const cli = await promisify(execFile)(binary, ['fetch', url, '--browser', '--raw', '--selector', '#rendered'], { env, timeout: 45000 });
    assert(cli.stdout.includes('rendered-canary'));
    const click = await tool('browser_action', { action: 'click', url, selector: '#change' });
    assert(!click.error, JSON.stringify(click));
    const dom = await tool('browser_action', { action: 'read_dom', url, selector: '#rendered' });
    assert(JSON.stringify(dom).includes('clicked-canary'));
    const screenshot = await tool('browser_action', { action: 'screenshot', url });
    assert(!screenshot.error, JSON.stringify(screenshot));
    const image = JSON.parse(screenshot.result.content[0].text).screenshot;
    assert(image && Buffer.from(image, 'base64').subarray(1, 4).toString() === 'PNG');
    const cookies = await tool('browser_action', { action: 'get_cookies', url });
    assert(cookies.error && cookies.error.message.includes('disabled'));
    const javascript = await tool('browser_action', { action: 'execute_js', url, code: 'document.cookie' });
    assert(javascript.error && javascript.error.message.includes('not allowed'));
    const redirected = await tool('fetch_page', { url: url + 'redirect', use_browser: true });
    assert(redirected.error && redirected.error.message.includes('blocked'), JSON.stringify(redirected));
    const page = browser.pages()[0] || await browser.newPage();
    const version = await page.evaluate(() => navigator.userAgent);
    console.log(JSON.stringify({ ok: true, browser: version, protocol: initialized.result.protocolVersion,
      rendered_fetch: true, standalone_cli: true, click_and_read: true, screenshot_png: true,
      cookie_and_javascript_denied: true, redirected_page_denied: true,
      isolated_extension_discovery: true, personal_profile_used: false }, null, 2));
  } finally {
    if (browser) await browser.close();
    if (child) { child.stdin.end(); if (child.exitCode === null) child.kill('SIGTERM'); }
    if (fixture) await new Promise(resolve => fixture.close(resolve));
    fs.rmSync(root, { recursive: true, force: true });
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
