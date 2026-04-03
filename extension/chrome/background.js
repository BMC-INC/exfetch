// exfetch Chrome Extension — Background Service Worker
// Connects to the exfetch CLI WebSocket server and executes browser commands.

const PORT_RANGE_START = 9876;
const PORT_RANGE_END = 9886;
const RECONNECT_INTERVAL_MS = 5000;
const KEEPALIVE_ALARM_NAME = "exfetch-keepalive";
const KEEPALIVE_PERIOD_MINUTES = 25 / 60; // ~25 seconds
const COMMAND_TIMEOUT_MS = 30000;

let ws = null;
let connectionState = "disconnected"; // "disconnected" | "connecting" | "authenticated"
let connectionId = null;
let currentPort = null;

// ---------------------------------------------------------------------------
// WebSocket lifecycle
// ---------------------------------------------------------------------------

async function discoverPort() {
  for (let port = PORT_RANGE_START; port <= PORT_RANGE_END; port++) {
    try {
      const resp = await fetch(`http://127.0.0.1:${port}/health`, {
        signal: AbortSignal.timeout(1000),
      });
      if (resp.ok) return port;
    } catch {
      // port not available, try next
    }
  }
  // Fallback: try connecting WebSocket directly to each port
  return PORT_RANGE_START;
}

async function fetchToken(port) {
  try {
    const resp = await fetch(`http://127.0.0.1:${port}/token`, {
      signal: AbortSignal.timeout(2000),
    });
    if (resp.ok) {
      const data = await resp.json();
      return data.token || null;
    }
  } catch {
    // Token endpoint may not exist; read from storage as fallback
  }
  // Fallback: check extension storage for a manually configured token
  return new Promise((resolve) => {
    chrome.storage.local.get("exfetch_token", (items) => {
      resolve(items.exfetch_token || null);
    });
  });
}

async function connect() {
  if (connectionState === "connecting" || connectionState === "authenticated") return;
  connectionState = "connecting";

  const port = await discoverPort();
  const token = await fetchToken(port);

  if (!token) {
    console.warn("[exfetch] No token available. Will retry.");
    connectionState = "disconnected";
    scheduleReconnect();
    return;
  }

  try {
    ws = new WebSocket(`ws://127.0.0.1:${port}`);
  } catch (e) {
    console.warn("[exfetch] WebSocket constructor error:", e);
    connectionState = "disconnected";
    scheduleReconnect();
    return;
  }

  ws.onopen = () => {
    console.log("[exfetch] WebSocket opened, sending auth...");
    ws.send(
      JSON.stringify({
        token,
        browser: "chrome",
        profile: "default",
      })
    );
  };

  ws.onmessage = (event) => {
    let data;
    try {
      data = JSON.parse(event.data);
    } catch {
      console.warn("[exfetch] Non-JSON message:", event.data);
      return;
    }

    // Handle auth acknowledgement
    if (data.status === "authenticated") {
      connectionState = "authenticated";
      connectionId = data.connection_id;
      currentPort = port;
      console.log("[exfetch] Authenticated. Connection ID:", connectionId);
      return;
    }

    // Handle auth rejection
    if (data.status === "rejected" || data.status === "error") {
      console.error("[exfetch] Auth failed:", data.reason);
      connectionState = "disconnected";
      ws.close();
      return;
    }

    // Handle command requests from CLI
    if (data.msg_type === "request") {
      handleCommand(data);
    }
  };

  ws.onclose = () => {
    console.log("[exfetch] WebSocket closed.");
    connectionState = "disconnected";
    connectionId = null;
    ws = null;
    scheduleReconnect();
  };

  ws.onerror = (err) => {
    console.warn("[exfetch] WebSocket error:", err);
    // onclose will fire after this
  };
}

function scheduleReconnect() {
  setTimeout(() => connect(), RECONNECT_INTERVAL_MS);
}

function sendResponse(id, command, params) {
  if (!ws || ws.readyState !== WebSocket.OPEN) return;
  ws.send(
    JSON.stringify({
      id,
      msg_type: "response",
      command,
      params,
      timestamp: new Date().toISOString(),
    })
  );
}

// ---------------------------------------------------------------------------
// Keepalive alarm (prevents service worker from going idle)
// ---------------------------------------------------------------------------

chrome.alarms.create(KEEPALIVE_ALARM_NAME, {
  periodInMinutes: KEEPALIVE_PERIOD_MINUTES,
});

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === KEEPALIVE_ALARM_NAME) {
    // Just touching the service worker is enough to keep it alive.
    // Also attempt reconnect if disconnected.
    if (connectionState === "disconnected") {
      connect();
    }
  }
});

// ---------------------------------------------------------------------------
// Tab helpers
// ---------------------------------------------------------------------------

function waitForTabLoad(tabId, timeoutMs = 15000) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      chrome.tabs.onUpdated.removeListener(listener);
      reject(new Error("Tab load timeout"));
    }, timeoutMs);

    function listener(updatedTabId, changeInfo) {
      if (updatedTabId === tabId && changeInfo.status === "complete") {
        clearTimeout(timer);
        chrome.tabs.onUpdated.removeListener(listener);
        resolve();
      }
    }
    chrome.tabs.onUpdated.addListener(listener);
  });
}

async function findOrCreateTab(url) {
  if (!url) {
    // Return the active tab
    const [active] = await chrome.tabs.query({ active: true, currentWindow: true });
    return active || null;
  }

  // Exact URL match
  const exactTabs = await chrome.tabs.query({ url });
  if (exactTabs.length > 0) return exactTabs[0];

  // Same-origin match
  try {
    const parsed = new URL(url);
    const originPattern = `${parsed.origin}/*`;
    const originTabs = await chrome.tabs.query({ url: originPattern });
    if (originTabs.length > 0) return originTabs[0];
  } catch {
    // invalid URL, skip origin match
  }

  // Fallback: create new tab
  const tab = await chrome.tabs.create({ url, active: false });
  await waitForTabLoad(tab.id);
  return tab;
}

// ---------------------------------------------------------------------------
// Command dispatch
// ---------------------------------------------------------------------------

async function handleCommand(msg) {
  const { id, command, params } = msg;
  try {
    let result;
    switch (command) {
      case "fetch_page":
        result = await cmdFetchPage(params);
        break;
      case "read_dom":
        result = await cmdReadDom(params);
        break;
      case "click":
        result = await cmdClick(params);
        break;
      case "type_text":
        result = await cmdTypeText(params);
        break;
      case "navigate":
        result = await cmdNavigate(params);
        break;
      case "screenshot":
        result = await cmdScreenshot(params);
        break;
      case "get_cookies":
        result = await cmdGetCookies(params);
        break;
      case "list_tabs":
        result = await cmdListTabs(params);
        break;
      case "switch_tab":
        result = await cmdSwitchTab(params);
        break;
      case "execute_js":
        result = await cmdExecuteJs(params);
        break;
      default:
        result = { error: `Unknown command: ${command}` };
    }
    sendResponse(id, command, { success: true, ...result });
  } catch (err) {
    sendResponse(id, command, { success: false, error: err.message || String(err) });
  }
}

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

async function cmdFetchPage(params) {
  const tab = await findOrCreateTab(params.url);
  if (tab.url !== params.url) {
    await chrome.tabs.update(tab.id, { url: params.url });
    await waitForTabLoad(tab.id);
  }

  const results = await chrome.scripting.executeScript({
    target: { tabId: tab.id },
    func: () => ({
      html: document.documentElement.outerHTML,
      title: document.title,
      url: window.location.href,
    }),
  });

  const data = results[0]?.result || {};
  return { html: data.html, title: data.title, url: data.url, tab_id: tab.id };
}

async function cmdReadDom(params) {
  const tab = await findOrCreateTab(params.url);
  const selector = params.selector || "body";

  const results = await chrome.scripting.executeScript({
    target: { tabId: tab.id },
    func: (sel) => {
      const elements = document.querySelectorAll(sel);
      return Array.from(elements).map((el) => ({
        tag: el.tagName.toLowerCase(),
        text: el.textContent?.trim().substring(0, 5000) || "",
        html: el.outerHTML.substring(0, 10000),
        attributes: Object.fromEntries(
          Array.from(el.attributes).map((a) => [a.name, a.value])
        ),
      }));
    },
    args: [selector],
  });

  return { elements: results[0]?.result || [] };
}

async function cmdClick(params) {
  const tab = await findOrCreateTab(params.url);
  const selector = params.selector;
  if (!selector) throw new Error("selector is required for click");

  const results = await chrome.scripting.executeScript({
    target: { tabId: tab.id },
    func: (sel) => {
      const el = document.querySelector(sel);
      if (!el) return { found: false };
      el.click();
      return { found: true, tag: el.tagName.toLowerCase() };
    },
    args: [selector],
  });

  const data = results[0]?.result || {};
  if (!data.found) throw new Error(`Element not found: ${selector}`);
  return { clicked: true, tag: data.tag };
}

async function cmdTypeText(params) {
  const tab = await findOrCreateTab(params.url);
  const selector = params.selector;
  const text = params.text;
  if (!selector) throw new Error("selector is required for type_text");
  if (text === undefined || text === null) throw new Error("text is required for type_text");

  const results = await chrome.scripting.executeScript({
    target: { tabId: tab.id },
    func: (sel, val) => {
      const el = document.querySelector(sel);
      if (!el) return { found: false };
      el.focus();
      el.value = val;
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
      return { found: true, tag: el.tagName.toLowerCase() };
    },
    args: [selector, text],
  });

  const data = results[0]?.result || {};
  if (!data.found) throw new Error(`Element not found: ${selector}`);
  return { typed: true, tag: data.tag };
}

async function cmdNavigate(params) {
  const url = params.url;
  if (!url) throw new Error("url is required for navigate");

  const tab = await findOrCreateTab(url);
  if (tab.url !== url) {
    await chrome.tabs.update(tab.id, { url });
    await waitForTabLoad(tab.id);
  }

  return { navigated: true, tab_id: tab.id, url };
}

async function cmdScreenshot(params) {
  const tab = await findOrCreateTab(params.url);
  // Bring tab to front for captureVisibleTab
  await chrome.tabs.update(tab.id, { active: true });
  // Small delay to let the tab render
  await new Promise((r) => setTimeout(r, 200));

  const dataUrl = await chrome.tabs.captureVisibleTab(null, { format: "png" });
  // dataUrl is "data:image/png;base64,..."
  const base64 = dataUrl.split(",")[1] || dataUrl;
  return { screenshot: base64, format: "png", tab_id: tab.id };
}

async function cmdGetCookies(params) {
  const url = params.url;
  const domain = params.domain;

  const query = {};
  if (url) query.url = url;
  if (domain) query.domain = domain;

  const cookies = await chrome.cookies.getAll(query);
  return {
    cookies: cookies.map((c) => ({
      name: c.name,
      value: c.value,
      domain: c.domain,
      path: c.path,
      secure: c.secure,
      httpOnly: c.httpOnly,
      expirationDate: c.expirationDate,
    })),
  };
}

async function cmdListTabs(_params) {
  const tabs = await chrome.tabs.query({});
  return {
    tabs: tabs.map((t) => ({
      id: t.id,
      url: t.url,
      title: t.title,
      active: t.active,
      windowId: t.windowId,
    })),
  };
}

async function cmdSwitchTab(params) {
  const tabId = params.tab_id;
  if (tabId === undefined || tabId === null) throw new Error("tab_id is required for switch_tab");

  await chrome.tabs.update(tabId, { active: true });
  const tab = await chrome.tabs.get(tabId);
  return { switched: true, tab_id: tab.id, url: tab.url, title: tab.title };
}

async function cmdExecuteJs(params) {
  // SECURITY NOTE: This command only executes JavaScript that has been sent
  // through the authenticated WebSocket channel from the exfetch CLI binary.
  // The CLI's PolicyEngine gates whether execute_js is allowed at all.
  // The extension trusts authenticated binary commands by design.
  const tab = await findOrCreateTab(params.url);
  const code = params.code;
  if (!code) throw new Error("code is required for execute_js");

  const results = await chrome.scripting.executeScript({
    target: { tabId: tab.id },
    func: (userCode) => {
      // Evaluate the code string in the page context
      const fn = Function('"use strict"; return (async () => {' + userCode + '})()');
      return fn();
    },
    args: [code],
  });

  return { result: results[0]?.result };
}

// ---------------------------------------------------------------------------
// Popup status queries
// ---------------------------------------------------------------------------

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message.type === "get_status") {
    sendResponse({
      state: connectionState,
      connectionId,
      port: currentPort,
    });
    return true; // async response
  }
});

// ---------------------------------------------------------------------------
// Init: start connection on install/startup
// ---------------------------------------------------------------------------

chrome.runtime.onInstalled.addListener(() => {
  console.log("[exfetch] Extension installed. Connecting...");
  connect();
});

chrome.runtime.onStartup.addListener(() => {
  connect();
});

// Also try to connect immediately when the service worker loads
connect();
