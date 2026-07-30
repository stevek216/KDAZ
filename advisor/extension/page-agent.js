// MAIN-world capture agent: reads the live BGA Kingdomino client and hands snapshots to the
// extension via window.postMessage (the MAIN world has no chrome.* APIs; content.js bridges).
// READ-ONLY: it never clicks, never plays, and never mutates the page or the game's state.
//
// Kingdomino's data layer is unusually kind. `gamedatas.dominoes` is the ENTIRE game state —
// every tile's location (CURRENT / FUTURE / KINGDOM / DISCARD), which king owns it, and where
// it was placed — and the client keeps locations and owners live as notifications arrive. So
// there is no DOM scraping here at all, with one exception: the client does not write a
// placement's coordinates back into `gamedatas` (see `notif_dominoPlaced` in kingdomino.js),
// so this agent subscribes to the notification bus and records them itself. Coordinates that
// predate the extension come from the load-time `getAllDatas`, which does include them.
//
// The one thing nobody can recover is a tile DISCARDED before this agent started watching:
// `getAllDatas` omits the discard pile entirely. That is deliberately left to fail loudly —
// the server refuses to advise from a deck it knows is wrong.

(function () {
  "use strict";
  const W = window;
  if (W.__kdAdvisorRunning) return; // BGA's SPA can re-run scripts -> one loop only
  W.__kdAdvisorRunning = true;

  const POLL_MS = 900;

  // Ring buffer of what the capture loop saw, shipped with a debug dump so a live game can be
  // investigated afterwards rather than paused.
  const DBG_MAX = 300;
  const dbgLog = [];
  function dlog(level, msg) {
    dbgLog.push({ t: new Date().toISOString(), level, msg: String(msg) });
    if (dbgLog.length > DBG_MAX) dbgLog.shift();
    (level === "warn" ? console.warn : console.log)(msg);
  }

  function post(msg) {
    try { W.postMessage({ __kdAdvisor: msg }, "*"); } catch (e) {}
  }

  // BGA wraps the live client in <iframe id="gameIframe"> even for a plain table, so the top
  // window usually has no window.gameui of its own.
  function gameIframeWin(doc) {
    try { const f = doc.getElementById("gameIframe"); return (f && f.contentWindow) || null; }
    catch (e) { return null; }
  }
  function isKingdomino() {
    try { if (W.gameui && W.gameui.game_name) return W.gameui.game_name === "kingdomino"; } catch (e) {}
    try {
      const gi = W.document.getElementById("gameIframe");
      const gw = gi && gi.contentWindow;
      if (gw && gw.gameui && gw.gameui.game_name) return gw.gameui.game_name === "kingdomino";
      if (gi && /kingdomino/.test(gi.src || "")) return true;
    } catch (e) {}
    return /kingdomino/.test(W.location.pathname + W.location.search);
  }

  // Every game context on the page: this window, the client iframe, and any hotseat frames.
  function gatherContexts() {
    const ctxs = [];
    const consider = (w) => {
      try { if (w && w.gameui && w.gameui.gamedatas) ctxs.push(w); } catch (e) {}
    };
    consider(W);
    const giw = gameIframeWin(W.document);
    consider(giw);
    const docs = [W.document];
    try { if (giw && giw.document) docs.push(giw.document); } catch (e) {}
    docs.forEach((d) => {
      try {
        d.querySelectorAll('iframe[id^="hotseat_iframe_"]').forEach((f) => consider(f.contentWindow));
      } catch (e) {}
    });
    return ctxs;
  }

  // The context the user is LOOKING at (hotseat parks other seats' iframes off-screen).
  function visibleContext() {
    try {
      for (const f of W.document.querySelectorAll('iframe[id^="hotseat_iframe_"]')) {
        const r = f.getBoundingClientRect();
        if (r.width > 0 && r.height > 0 && r.left < W.innerWidth && r.left + r.width > 0
            && r.top < W.innerHeight && r.top + r.height > 0) {
          const w = f.contentWindow;
          if (w && w.gameui && w.gameui.gamedatas) return w;
        }
      }
    } catch (e) {}
    return null;
  }

  // Don't snapshot while a context is still animating queued notifications: the mirror is
  // mid-update and the position would be a torn read.
  function queuesBusy(ctxs) {
    return ctxs.some((w) => {
      try { const q = w.gameui.notifqueue; return !!(q && q.queue && q.queue.length > 0); }
      catch (e) { return false; }
    });
  }

  // ---- placement coordinates, recorded from the notification bus -------------------------
  // number -> {x, y, rotation}, per table. BGA dispatches every server notification through
  // dojo.publish, and dojo.subscribe is additive: the game's own handler still runs, the
  // sync-ack mechanism is per type and satisfied by it, and the page cannot tell we listened.
  const placements = Object.create(null);
  const NOTIF_TYPES = ["dominoPlaced", "dominoChosen", "dominoesDrawn", "dominoDiscarded"];

  function hookNotifications(w) {
    try {
      if (!w || w.__kdAdvisorNotifHooked) return;
      const g = w.gameui, dj = w.dojo;
      if (!g || !g.gamedatas || !dj || typeof dj.subscribe !== "function") return;
      if (g.game_name && g.game_name !== "kingdomino") return;
      w.__kdAdvisorNotifHooked = true;
      dj.subscribe("dominoPlaced", (notif) => {
        try {
          const a = (notif && notif.args) || {};
          const p = a.position || {};
          if (a.number != null && p.x != null) {
            placements[+a.number] = { x: +p.x, y: +p.y, rotation: +p.rotation };
          }
        } catch (e) {}
      });
      // The rest ride along purely as capture diagnostics; `gamedatas` already tracks them.
      NOTIF_TYPES.slice(1).forEach((type) => {
        try {
          dj.subscribe(type, (notif) => {
            try { dlog("log", "[advisor] " + type + " " + JSON.stringify((notif && notif.args) || {}).slice(0, 200)); }
            catch (e) {}
          });
        } catch (e) {}
      });
      dlog("log", "[advisor] notification hook installed for seat " + g.player_id);
    } catch (e) {}
  }

  setInterval(() => {
    try {
      hookNotifications(W);
      if (W.self === W.top && isKingdomino()) gatherContexts().forEach(hookNotifications);
    } catch (e) {}
  }, 2000);

  // ---- snapshot ---------------------------------------------------------------------------
  // The variant flags are server-rendered into the "additional rules" buttons
  // (kingdomino.view.php sets each one's class to visible/hidden), which is the only place a
  // client can read them: `getAllDatas` does not carry the table's options.
  function variants(w, gd) {
    const on = (id) => {
      try {
        const el = w.document.getElementById(id);
        return !!(el && el.classList.contains("visible"));
      } catch (e) { return false; }
    };
    const grid = +gd.gridSize || 0;
    return {
      harmony: on("harmonyButton"),
      middle_kingdom: on("middleKingdomButton"),
      // The grid size is the reliable Mighty Duel signal; the button is the corroboration.
      mighty_duel: grid === 7 || on("mightyDuelButton"),
      lost_treasures: on("lostTreasuresButton") || !!gd.treasures,
      dynasty: on("dynastyButton"),
    };
  }

  function livePlayers(w, gd) {
    const g = w.gameui;
    return Object.values(gd.players).map((p) => {
      const pid = String(p.id);
      let score = null;
      try { score = g.scoreCtrl && g.scoreCtrl[pid] ? +g.scoreCtrl[pid].getValue() : null; } catch (e) {}
      return {
        bga_id: pid,
        name: String(p.player_name || p.name || ("seat " + pid)),
        color: String(p.player_color || p.color || ""),
        score: score == null ? +p.score || 0 : score,
      };
    });
  }

  // `gamedatas.dominoes` is authoritative for location and owner (the client's notification
  // handlers keep both live). Coordinates come from there when the placement predates us, and
  // from our own notification record otherwise — the one field the client never writes back.
  function dominoes(gd) {
    const out = {};
    const src = gd.dominoes || {};
    for (const key in src) {
      if (!Object.prototype.hasOwnProperty.call(src, key)) continue;
      const d = src[key];
      const number = +key;
      const rec = { location: String(d.location), owner: d.owner_player ? String(d.owner_player) : null };
      if (rec.location === "KINGDOM") {
        const own = placements[number];
        if (d.x != null && d.rotation != null) {
          rec.x = +d.x; rec.y = +d.y; rec.rotation = +d.rotation;
        } else if (own) {
          rec.x = own.x; rec.y = own.y; rec.rotation = own.rotation;
        }
        // No coordinates either way: leave them out. The server refuses the position and says
        // why, which is the right outcome — a placed tile in an unknown cell is not a position.
      }
      out[String(number)] = rec;
    }
    return out;
  }

  // BGA merges "place" and "claim" into ONE server action when the placement is staged first.
  // `selectDomino` in kingdomino.js hangs the chosen tile off the staged position and submits
  // `placeDomino(position, nextDomino)`; the backend routes it through `chooseDomino` itself.
  // So the server state stays `placeDomino` while the status bar already reads "You must
  // choose a domino", and the ONLY evidence the decision has moved on is `selectedPosition`.
  //
  // Returns the staged placement when the pending decision is really the claim, else null.
  function stagedPlacement(g, gd, gs) {
    try {
      if (String(gs.name) !== "placeDomino") return null;
      // Staging lives in one client's memory, so it only speaks for the seat that did it.
      // Without this, a stale `selectedPosition` (BGA only clears it when THIS client is the
      // active one entering placeDomino) would be read as the opponent's staged move.
      if (String(gs.active_player) !== String(g.player_id)) return null;
      const p = g.selectedPosition;
      if (!p || p.x == null || p.y == null || p.rotation == null) return null;
      const number = +(gs.args || {}).domino;
      if (!number) return null;
      // Still pending: once BGA actually applies the placement, `gamedatas` says KINGDOM and
      // the ordinary path takes over. A second guard against a stale staged position.
      const d = gd.dominoes[String(number)];
      if (!d || d.location !== "CURRENT") return null;
      // A claim only follows if there is something to claim. On the last domino (or a
      // treasure discovery) BGA shows a standalone Confirm button instead, and the pending
      // decision really is still the placement.
      let choosable = false;
      for (const k in gd.dominoes) {
        const t = gd.dominoes[k];
        if (t.location === "FUTURE" && !t.owner_player) { choosable = true; break; }
      }
      if (!choosable) return null;
      return { number, x: +p.x, y: +p.y, rotation: +p.rotation };
    } catch (e) { return null; }
  }

  // Declared above their first use: `let` in a strict-mode IIFE is in the temporal dead zone
  // until its declaration, and a ReferenceError inside the poll loop's catch would be
  // invisible — the diagnostic that never fires.
  let lastPostedSig = null;
  let pendingSig = null;
  let lastStatus = null;
  let lastUnhandled = null;

  function snapshot() {
    if (W.self !== W.top) return null; // only the top-window instance drives
    const ctxs = gatherContexts();
    if (!ctxs.length) return null;
    if (queuesBusy(ctxs)) return null;

    // Prefer the context the user is looking at (hotseat), then this client's own.
    const w = visibleContext() || ctxs[ctxs.length - 1];
    const g = w.gameui, gd = g.gamedatas, gs = gd && gd.gamestate;
    if (!gs || !gd.dominoes) return null;

    const state = String(gs.name || "");
    if (state !== "chooseDomino" && state !== "placeDomino") {
      // Report only states that are real decisions we cannot advise on (today that means
      // `placeTreasure`, the Lost Treasures pick). BGA's setup, router and end states pass
      // through constantly and flagging them would cry wolf.
      const benign = state === "gameSetup" || state === "nextPlayer" || state === "gameEnd";
      if (state && !benign && state !== lastUnhandled) {
        lastUnhandled = state;
        post({ kind: "unhandled", state });
      }
      return null;
    }

    const args = gs.args || {};
    const snap = {
      table: String(g.table_id || gd.tablename || ""),
      seat_bga: String(g.player_id || ""),
      active_bga: String(gs.active_player || ""),
      state,
      grid_size: +gd.gridSize || 0,
      turns_left: +gd.turnsLeft || 0,
      variants: variants(w, gd),
      players: livePlayers(w, gd),
      dominoes: dominoes(gd),
    };
    // The player has staged a placement and is now picking the tile that will commit it:
    // report the position as it will be, and the claim as the decision. Advising the claim
    // from the pre-placement board would evaluate a position that is about to stop existing.
    const staged = stagedPlacement(g, gd, gs);
    if (staged) {
      snap.state = "chooseDomino";
      snap.dominoes[String(staged.number)] = {
        location: "KINGDOM",
        owner: String(gs.active_player),
        x: staged.x, y: staged.y, rotation: staged.rotation,
      };
      // Let the panel say the advice is conditional on that placement.
      snap.staged_placement = staged;
      // No placement oracle here: `placementPreviews` describes a decision already settled,
      // and comparing it against the claim's legal actions would report a false divergence.
      return snap;
    }

    // Unlike Space Base, EVERY seat's position is public here, so a passive client can advise
    // the player on the clock too. BGA even ships that player's own legal-placement list to
    // every client (argsPlaceDomino is computed for the active player and broadcast).
    if (state === "placeDomino") {
      if (args.domino != null) snap.bga_current = +args.domino;
      if (Array.isArray(args.placementPreviews)) {
        snap.bga_previews = args.placementPreviews.map((p) => ({
          x: +p.x, y: +p.y, rotation: +p.rotation, score: +p.score,
        }));
      }
    }
    return snap;
  }

  // ---- poll loop --------------------------------------------------------------------------
  setInterval(() => {
    try {
      if (W.self !== W.top || !isKingdomino()) return;
      const s = snapshot();
      const status = s ? s.state : "watching";
      if (status !== lastStatus) { lastStatus = status; post({ kind: "status", status }); }
      if (!s) { pendingSig = null; return; }
      const sig = JSON.stringify(s);
      // Settle-and-re-post: send only once the snapshot has been STABLE for one extra poll,
      // and re-send whenever it later changes, which self-corrects a stale or partial read.
      if (sig === lastPostedSig) { pendingSig = sig; return; }
      if (sig !== pendingSig) { pendingSig = sig; return; }
      lastPostedSig = sig;
      dlog("log", "[advisor] posting " + s.state + " snapshot");
      post({ kind: "snapshot", snapshot: s });
    } catch (e) { /* never disrupt the game */ }
  }, POLL_MS);

  // ---- debug dump -------------------------------------------------------------------------
  function ctxInfo(w) {
    const o = {};
    try { o.href = w.location.href; } catch (e) { o.href_err = String(e); }
    try {
      const g = w.gameui;
      o.player_id = g.player_id;
      o.table_id = g.table_id;
      o.game_name = g.game_name;
      try { o.is_active = typeof g.isCurrentPlayerActive === "function" ? g.isCurrentPlayerActive() : null; } catch (e) {}
      try { o.notifq_len = g.notifqueue && g.notifqueue.queue ? g.notifqueue.queue.length : null; } catch (e) {}
      const gs = g.gamedatas && g.gamedatas.gamestate;
      o.gamestate = gs ? { name: gs.name, active_player: gs.active_player, args: gs.args } : null;
    } catch (e) { o.err = String(e); }
    try { o.gamedatas = w.gameui.gamedatas; } catch (e) { o.gamedatas_err = String(e); }
    try { o.html = w.document.documentElement.outerHTML.slice(0, 4000000); } catch (e) { o.html_err = String(e); }
    return o;
  }

  function buildDump() {
    const d = {
      when: new Date().toISOString(),
      top_href: W.location.href,
      is_kingdomino: isKingdomino(),
      poll_ms: POLL_MS,
      last_status: lastStatus,
      last_unhandled: lastUnhandled,
      recorded_placements: placements,
      log: dbgLog.slice(),
    };
    try {
      const ctxs = gatherContexts();
      d.context_count = ctxs.length;
      d.queues_busy = queuesBusy(ctxs);
      d.contexts = ctxs.map(ctxInfo);
      try { d.snapshot = snapshot(); } catch (e) { d.snapshot_err = String(e) + "\n" + ((e && e.stack) || ""); }
    } catch (e) { d.err = String(e) + "\n" + ((e && e.stack) || ""); }
    return d;
  }

  W.addEventListener("message", (ev) => {
    if (ev.source !== W || !ev.data || !ev.data.__kdAdvisorDumpReq) return;
    // The top instance always answers (its diagnostics matter most when capture is broken),
    // and so does any frame that holds a game client of its own.
    const isTop = W.self === W.top;
    let hasGame = false;
    try { hasGame = !!(W.gameui && W.gameui.gamedatas); } catch (e) {}
    if (!isTop && !hasGame) return;
    let data;
    try { data = buildDump(); } catch (e) { data = { fatal: String(e) + "\n" + ((e && e.stack) || "") }; }
    data.frame = { top: isTop, has_gameui: hasGame };
    post({ kind: "dump", data });
  });

  dlog("log", "[Kingdomino advisor] page agent active (MAIN world), watching for decisions");
})();
