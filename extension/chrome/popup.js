// Query background service worker for connection status and update the popup UI.

const dot = document.getElementById("dot");
const statusText = document.getElementById("status-text");
const detail = document.getElementById("detail");

chrome.runtime.sendMessage({ type: "get_status" }, (response) => {
  if (chrome.runtime.lastError || !response) {
    dot.className = "dot disconnected";
    statusText.textContent = "Unable to reach background worker";
    return;
  }

  const { state, connectionId, port } = response;

  dot.className = `dot ${state === "authenticated" ? "connected" : state === "connecting" ? "connecting" : "disconnected"}`;

  const labels = {
    authenticated: "Connected",
    connecting: "Connecting...",
    disconnected: "Disconnected",
  };
  statusText.textContent = labels[state] || state;

  const parts = [];
  if (port) parts.push(`Port: ${port}`);
  if (connectionId) parts.push(`ID: ${connectionId.substring(0, 8)}...`);
  detail.textContent = parts.join(" | ");
});
