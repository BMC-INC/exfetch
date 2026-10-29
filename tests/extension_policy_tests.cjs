const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const vm = require('node:vm');
const { test } = require('node:test');

for (const browser of ['chrome', 'safari/Shared']) {
  test(`${browser}: extension rejects blocked navigation and validates loaded pages`, async () => {
    let queried = false;
    const context = vm.createContext({
      URL, AbortSignal, console,
      fetch: () => new Promise(() => {}),
      chrome: {
        alarms: { create() {}, onAlarm: { addListener() {} } },
        runtime: { onMessage: { addListener() {} }, onInstalled: { addListener() {} }, onStartup: { addListener() {} } },
        tabs: {
          query: async () => { queried = true; return []; },
          create: async () => ({ id: 1, url: 'https://allowed.example' }),
          get: async () => ({ id: 1, url: 'https://blocked.example' }),
        },
      },
    });
    vm.runInContext(readFileSync(`extension/${browser}/background.js`, 'utf8'), context);
    vm.runInContext('waitForTabLoad = async () => {}', context);
    await assert.rejects(
      vm.runInContext('findOrCreateTab("https://blocked.example", { blocked_domains: ["blocked.example"] })', context),
      /blocked by policy/,
    );
    assert.throws(() => vm.runInContext('assertAllowedUrl("https://blocked.example./", { blocked_domains: ["blocked.example"] })', context), /blocked by policy/);
    for (const url of ['file:///tmp/private.html', 'chrome://settings', 'data:text/html,private']) {
      assert.throws(() => vm.runInContext(`assertAllowedUrl(${JSON.stringify(url)})`, context), /only HTTP and HTTPS/);
    }
    assert.equal(queried, false, 'blocked target must be refused before opening/querying a tab');
    await assert.rejects(
      vm.runInContext('findOrCreateTab("https://allowed.example", { blocked_domains: ["blocked.example"] })', context),
      /blocked by policy/,
    );
    assert.throws(() => vm.runInContext('assertAllowedUrl("https://outside.example", { allowed_domains: ["*.example.com"] })', context), /allowlist/);
  });

  test(`${browser}: screenshot captures the validated tab's window and rechecks navigation`, async () => {
    let current = { id: 7, windowId: 42, active: true, url: 'https://allowed.example' };
    const captured = [];
    const context = vm.createContext({
      URL, AbortSignal, console,
      setTimeout: (callback) => { callback(); },
      fetch: () => new Promise(() => {}),
      chrome: {
        alarms: { create() {}, onAlarm: { addListener() {} } },
        runtime: { onMessage: { addListener() {} }, onInstalled: { addListener() {} }, onStartup: { addListener() {} } },
        tabs: {
          update: async (id, properties) => {
            assert.equal(id, 7);
            assert.equal(properties.active, true);
          },
          get: async () => current,
          captureVisibleTab: async (windowId) => { captured.push(windowId); return 'data:image/png;base64,image'; },
        },
      },
    });
    vm.runInContext(readFileSync(`extension/${browser}/background.js`, 'utf8'), context);
    vm.runInContext('findOrCreateTab = async () => ({ id: 7, windowId: 42, url: "https://allowed.example" })', context);
    const command = 'cmdScreenshot({ url: "https://allowed.example", blocked_domains: ["blocked.example"] })';
    const result = await vm.runInContext(command, context);
    assert.equal(result.screenshot, 'image');
    assert.deepEqual(captured, [42], 'capture must target the allowed tab window, not the current window');

    current = { ...current, url: 'https://blocked.example' };
    await assert.rejects(vm.runInContext(command, context), /blocked by policy/);
    current = { ...current, url: 'https://allowed.example', active: false };
    await assert.rejects(vm.runInContext(command, context), /no longer the active tab/);
    assert.deepEqual(captured, [42], 'changed or inactive targets must not be captured');
  });

}
