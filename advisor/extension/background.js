// Service worker: receives snapshots from the content bridge, POSTs them to the local advisor
// server, caches the latest reply in session storage, and pings the side panel to refresh.
//
// The fetch lives here rather than in the page because BGA's CSP blocks page-context requests
// to localhost; the extension's host_permissions cover it with no CORS gymnastics.
"use strict";

const DEFAULT_SERVER = "http://localhost:8000";

async function serverUrl() {
  const { server } = await chrome.storage.local.get("server");
  return (server || DEFAULT_SERVER).replace(/\/+$/, "");
}

chrome.runtime.onInstalled.addListener(() => {
  chrome.sidePanel.setPanelBehavior({ openPanelOnActionClick: true }).catch(() => {});
});

async function setState(patch) {
  const cur = (await chrome.storage.session.get("state")).state || {};
  const state = { ...cur, ...patch, ts: Date.now() };
  await chrome.storage.session.set({ state });
  chrome.runtime.sendMessage({ from: "bg", kind: "state" }).catch(() => {});
}

function notifyTab(tabId, msg) {
  if (tabId != null) chrome.tabs.sendMessage(tabId, msg).catch(() => {});
}

let gameTabId = null; // last tab that posted a snapshot (the highlight target)

async function findGameTab() {
  if (gameTabId != null) {
    try { await chrome.tabs.get(gameTabId); return gameTabId; } catch (e) { gameTabId = null; }
  }
  const tabs = await chrome.tabs.query({
    url: ["https://boardgamearena.com/*", "https://*.boardgamearena.com/*"],
  });
  return tabs.length ? tabs[0].id : null;
}

// Paint the top recommendation on the board. Anything that makes the advice untrustworthy —
// a table we do not support, an unreadable position — clears the highlight instead, so the
// page never points at a move the panel is disowning.
function highlightTop(rec) {
  const ok = rec && !rec.unsupported && !rec.capture_error;
  const top = ok && rec.recommendations && rec.recommendations[0];
  notifyTab(gameTabId, { kind: "hl", hl: top ? top.hl : null });
}

async function post(path, body) {
  const r = await fetch((await serverUrl()) + path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body || {}),
  });
  if (!r.ok) throw new Error("server " + r.status + ": " + (await r.text()).slice(0, 200));
  return r.json();
}

async function get(path) {
  const r = await fetch((await serverUrl()) + path);
  if (!r.ok) throw new Error("server " + r.status);
  return r.json();
}

chrome.runtime.onMessage.addListener((msg, sender) => {
  // ---- panel -> server ----
  if (msg && msg.from === "panel") {
    if (msg.kind === "set-server") {
      chrome.storage.local.set({ server: msg.server }).then(() =>
        get("/config")
          .then((c) => setState({ config: c, error: null }))
          .catch((e) => setState({ error: String(e) })));
      return;
    }
    if (msg.kind === "set-sims" || msg.kind === "set-checkpoint") {
      const patch = msg.kind === "set-sims" ? { sims: msg.sims } : { checkpoint: msg.checkpoint };
      setState({ posting: true, error: null });
      post("/config", patch)
        .then((c) => get("/latest").then((rec) => {
          const has = rec && (rec.recommendations || rec.unsupported || rec.capture_error);
          setState({ posting: false, error: null, config: c, ...(has ? { rec } : {}) });
          if (has) highlightTop(rec);
        }))
        .catch((e) => setState({ posting: false, error: String(e) }));
      return;
    }
    if (msg.kind === "refresh") {
      Promise.all([get("/config"), get("/models").catch(() => null)])
        .then(([config, models]) => setState({ config, models, error: null }))
        .catch((e) => setState({ error: String(e) }));
      return;
    }
    if (msg.kind === "dump") {
      setState({ dump: { busy: true } });
      findGameTab()
        .then((tid) => {
          if (tid == null) throw new Error("no BGA tab found");
          gameTabId = tid;
          return chrome.tabs.sendMessage(tid, { kind: "dump-req" });
        })
        .catch((e) => setState({ dump: { busy: false, error: String(e) } }));
      return;
    }
    return;
  }

  // ---- page -> server ----
  if (!msg || msg.from !== "page") return;
  const tabId = sender.tab && sender.tab.id;
  if (msg.kind === "snapshot" || msg.kind === "hello") gameTabId = tabId;

  if (msg.kind === "dump") {
    (async () => {
      const { state } = await chrome.storage.session.get("state");
      const out = await post("/debug_dump", {
        client: msg.data,
        extension: { version: chrome.runtime.getManifest().version, state: state || null },
      });
      await setState({ dump: { busy: false, path: out.path, bytes: out.bytes } });
    })().catch((e) => setState({ dump: { busy: false, error: String(e) } }));
    return;
  }
  if (msg.kind === "status") {
    setState({ pageStatus: msg.status });
    if (msg.status === "watching") notifyTab(tabId, { kind: "hl", hl: null }); // decision gone
    return;
  }
  if (msg.kind === "unhandled") {
    setState({ unhandled: msg.state });
    return;
  }
  if (msg.kind === "snapshot") {
    setState({ posting: true, error: null, unhandled: null });
    notifyTab(tabId, { kind: "hl", hl: null }); // drop the stale highlight while we think
    post("/recommend_bga", msg.snapshot)
      .then((rec) => {
        setState({ posting: false, rec, error: null });
        highlightTop(rec);
      })
      .catch((e) => setState({ posting: false, error: String(e) }));
  }
});
