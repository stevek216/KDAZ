// Side panel: renders whatever background.js last put in session storage.
//
// Two things it must never do: show stale advice as if it were current, and go quiet when
// something is wrong. Every refusal from the server gets its own banner, because a blank
// panel looks exactly like "nothing to advise on".
"use strict";

const $ = (id) => document.getElementById(id);
const send = (msg) => chrome.runtime.sendMessage({ from: "panel", ...msg });

const TERRAIN = ["wheat", "forest", "lake", "grassland", "swamp", "mine"];
const STORE = 13; // engine backing store side; the 7x7 kingdom floats inside it
const CENTER = 6;

// ---- controls ----
function setSims(n) {
  if (!Number.isFinite(n) || n < 0) return;
  send({ kind: "set-sims", sims: Math.floor(n) });
}
document.querySelectorAll("#controls button[data-sims]").forEach((b) =>
  b.addEventListener("click", () => setSims(+b.dataset.sims)));
$("sims-set").addEventListener("click", () => setSims(+$("sims-custom").value));
$("sims-custom").addEventListener("keydown", (e) => {
  if (e.key === "Enter") setSims(+$("sims-custom").value);
});
$("checkpoint").addEventListener("change", () =>
  send({ kind: "set-checkpoint", checkpoint: $("checkpoint").value }));
$("server-set").addEventListener("click", () => send({ kind: "set-server", server: $("server").value.trim() }));
$("server").addEventListener("keydown", (e) => {
  if (e.key === "Enter") send({ kind: "set-server", server: $("server").value.trim() });
});
$("dump-btn").addEventListener("click", () => send({ kind: "dump" }));

// ---- small builders ----
function bar(frac, label, pct) {
  const row = document.createElement("div");
  row.className = "row";
  const name = document.createElement("div");
  name.className = "name";
  name.textContent = label;
  const outer = document.createElement("div");
  outer.className = "bar";
  const inner = document.createElement("div");
  inner.style.width = Math.round(100 * Math.max(0, Math.min(1, frac))) + "%";
  outer.appendChild(inner);
  const val = document.createElement("div");
  val.textContent = pct;
  row.append(name, outer, val);
  return row;
}

function banner(el, text) {
  el.hidden = !text;
  if (text) el.textContent = text;
}

// The 7-wide slice of the 13-wide backing store to draw: centred on what has to be shown, but
// always covering it. A kingdom plus one placement can never span more than 7, so the clamps
// below never have to drop anything.
function window7(lo, hi) {
  let start = Math.round((lo + hi) / 2) - 3;
  start = Math.min(start, lo); // never clip the near edge
  start = Math.max(start, hi - 6); // nor the far one
  return Math.max(0, Math.min(start, STORE - 7));
}

// ---- the reconstructed kingdoms ----
// Rendering what the advisor believes the board is turns any capture bug into something you
// can SEE at the table, instead of advice that is confidently about the wrong position.
function renderBoards(rec) {
  const host = $("boards");
  host.replaceChildren();
  const top = (rec.recommendations || [])[0];
  // Keyed by cell so the preview can be drawn with the FACE that lands there — the panel has
  // to answer "which way round" just as much as the board does.
  const proposed = {};
  if (top && top.hl && top.hl.kind === "cells") {
    top.hl.cells.forEach((c) => {
      if (!Array.isArray(c)) proposed[c.x + "," + c.y] = c;
    });
  }
  (rec.board || []).forEach((b, seat) => {
    const wrap = document.createElement("div");
    wrap.className = "kingdom" + (seat === rec.to_act ? " acting" : "");
    const label = document.createElement("div");
    label.className = "label";
    const bonus = [b.harmony ? "+5 harmony" : null, b.middle_kingdom ? "+10 centred" : null]
      .filter(Boolean).join(", ");
    label.textContent = `${(rec.names || [])[seat] || "seat " + (seat + 1)} — `
      + `${b.crown_score} pts, ${b.filled}/48 squares`
      + (bonus ? ` (${bonus} if it ends here)` : "");
    const grid = document.createElement("div");
    grid.className = "grid";
    // The kingdom is a 7x7 window inside a 13x13 store; anchor it on the occupied bbox so the
    // whole thing is visible however the castle sits within it. The recommended placement is
    // included in that extent — it usually EXTENDS the bbox, and a highlight rendered outside
    // the window would silently vanish, which is worse than no highlight at all.
    const mine = seat === rec.to_act ? Object.values(proposed) : [];
    let [rLo, rHi, cLo, cHi] = b.bbox;
    mine.forEach(({ x, y }) => {
      const r = CENTER - y, c = CENTER + x;
      rLo = Math.min(rLo, r); rHi = Math.max(rHi, r);
      cLo = Math.min(cLo, c); cHi = Math.max(cHi, c);
    });
    const [r0, c0] = [window7(rLo, rHi), window7(cLo, cHi)];
    for (let r = r0; r < r0 + 7; r++) {
      for (let c = c0; c < c0 + 7; c++) {
        const cell = document.createElement("div");
        const here = b.cells[r + "," + c];
        const isCastle = r === b.castle[0] && c === b.castle[1];
        // The recommended placement, drawn on the acting seat's own kingdom as the FACE that
        // would land on each cell — so the panel shows the tile's orientation, not just its
        // footprint. A tile and its flip cover the same two squares.
        const put = seat === rec.to_act ? proposed[(c - CENTER) + "," + (CENTER - r)] : null;
        const shown = here || (put && put.terrain != null
          ? { t: put.terrain, k: put.crowns } : null);
        cell.className = "sq" + (isCastle ? " castle" : shown ? " t" + shown.t : "")
          + (put ? " hl" : "");
        if (isCastle) {
          cell.textContent = "⚑"; // flag: the castle tile
          cell.title = "castle (wild)";
        } else if (shown) {
          cell.textContent = shown.k ? "•".repeat(shown.k) : "";
          cell.title = `${TERRAIN[shown.t]}${shown.k ? " +" + shown.k + " crown" : ""}`
            + (put && !here ? " — recommended" : "");
        }
        grid.appendChild(cell);
      }
    }
    wrap.append(label, grid);
    host.appendChild(wrap);
  });
}

function renderLines(rec) {
  const host = $("lines");
  host.replaceChildren();
  const names = rec.names || [];
  const one = (title, slots) => {
    const row = document.createElement("div");
    row.className = "lrow";
    const parts = (slots || []).map((s) => {
      if (!s) return "·";
      const owner = s.owner == null ? "" : ` [${(names[s.owner] || "seat " + (s.owner + 1)).slice(0, 6)}]`;
      return s.desc + owner;
    });
    row.textContent = title + ": " + parts.join("   ");
    host.appendChild(row);
  };
  one("placing", (rec.lines || {}).current);
  one("claiming", (rec.lines || {}).next);
}

// ---- main render ----
function render(state) {
  const rec = state.rec;
  const cfg = state.config || {};
  const age = state.ts ? Math.round((Date.now() - state.ts) / 1000) : null;

  // controls
  const sims = cfg.sims != null ? cfg.sims : (rec && rec.sims);
  document.querySelectorAll("#controls button[data-sims]").forEach((b) => {
    b.disabled = !!state.posting;
    b.classList.toggle("active", sims != null && +b.dataset.sims === sims);
  });
  const sel = $("checkpoint");
  const models = (state.models && state.models.checkpoints) || [];
  if (sel.dataset.n !== String(models.length)) {
    sel.dataset.n = String(models.length);
    sel.replaceChildren();
    if (!models.length) sel.appendChild(new Option("no checkpoints", ""));
    models.forEach((m) => sel.appendChild(new Option(m.name, m.name)));
  }
  if (cfg.checkpoint) {
    const name = cfg.checkpoint.replace(/^.*[\\/]/, "").replace(/\.pt$/, "");
    if (sel.value !== name) sel.value = name;
  }

  // status line
  const st = $("status");
  if (state.error) {
    st.className = "err";
    st.textContent = "server error: " + state.error
      + " — is `python -m kdagent.server` running?";
  } else if (state.posting) {
    st.className = "";
    st.textContent = "thinking…";
  } else {
    st.className = "";
    st.textContent = "page: " + (state.pageStatus || "no game page seen")
      + (cfg.model ? " · " + cfg.model : "")
      + (age != null ? " · updated " + age + "s ago" : "");
  }

  // banners — each one names a specific reason not to trust what follows
  banner($("unhandled"), state.unhandled
    ? "⚠ unrecognised BGA state: " + state.unhandled + " — no advice for it"
    : "");
  banner($("unsupported"), rec && rec.unsupported
    ? "⚠ " + rec.unsupported + ". The advisor stays quiet rather than guess." : "");
  banner($("capture"), rec && rec.capture_error
    ? "⚠ cannot read this position: " + rec.capture_error
      + " — a tile discarded before the extension started watching does this; "
      + "it is unrecoverable for the rest of the game." : "");

  const lc = rec && rec.legality_check;
  banner($("legality"), lc
    ? (lc.error
        ? "⚠ legality check failed: " + lc.error
        : "⚠ engine/BGA disagree on this placement"
          + (lc.engine_missing && lc.engine_missing.length
              ? " — BGA allows " + lc.engine_missing.join(", ") + " and we do not" : "")
          + (lc.engine_extra && lc.engine_extra.length
              ? " — we allow " + lc.engine_extra.join(", ") + " and BGA does not" : "")
          + (lc.score_mismatch && lc.score_mismatch.length
              ? " — score differs at " + lc.score_mismatch.join("; ") : ""))
    : "");
  const sc = rec && rec.score_check;
  banner($("scorecheck"), sc
    ? "⚠ scores disagree with BGA: engine " + sc.engine.join("/")
      + " vs BGA " + sc.bga.join("/") : "");

  const showAdvice = !!(rec && rec.recommendations && rec.recommendations.length);
  $("content").hidden = !showAdvice;
  $("empty").hidden = !!(showAdvice || (rec && (rec.unsupported || rec.capture_error)));
  if (!showAdvice) return;

  const names = rec.names || [];
  const acting = rec.to_act;
  const top = rec.recommendations[0];
  $("who").textContent =
    (rec.your_turn ? "YOUR TURN — " : (names[acting] || "seat " + (acting + 1)) + " to act — ")
    + (top.desc || "");

  const val = $("value");
  val.replaceChildren();
  (rec.value || []).forEach((v, i) => {
    const row = bar(v, names[i] || "seat " + (i + 1), Math.round(100 * v) + "%");
    if (i === acting) row.classList.add("acting");
    val.appendChild(row);
  });
  if (rec.close) {
    const note = document.createElement("div");
    note.style.cssText = "font-size:11px;opacity:.7;margin-top:2px";
    note.textContent = "close position — the top move is a slight preference, not a plan";
    val.appendChild(note);
  }

  $("recs-h").textContent = "Recommended "
    + (rec.sims ? `(${rec.sims} sims, ${rec.n_legal} legal, ${rec.elapsed_ms}ms)`
                : `(raw policy, ${rec.n_legal} legal)`);
  const recs = $("recs");
  recs.replaceChildren();
  rec.recommendations.forEach((r, i) => {
    const div = document.createElement("div");
    div.className = "rec" + (i === 0 ? " top" : "");
    const desc = document.createElement("div");
    desc.className = "desc";
    desc.textContent = r.desc;
    const meta = document.createElement("div");
    meta.className = "meta";
    meta.textContent = [
      r.visits ? Math.round(100 * r.prob) + "% of visits" : Math.round(100 * r.prob) + "% policy",
      r.visits ? r.visits + " visits" : null,
      r.q != null ? "win " + Math.round(100 * r.q) + "%" : null,
      r.score_total != null ? "board would score " + r.score_total : null,
    ].filter(Boolean).join(" · ");
    const outer = document.createElement("div");
    outer.className = "bar";
    const inner = document.createElement("div");
    inner.style.width = Math.round(100 * (r.prob || 0)) + "%";
    outer.appendChild(inner);
    div.append(desc, meta, outer);
    recs.appendChild(div);
  });

  const cd = rec.current_domino;
  const sp = rec.staged_placement;
  $("ctx").textContent = `round ${rec.round + 1}/12 · ${rec.deck_remaining} left in the pile`
    + (cd ? ` · placing #${cd.number}` : "")
    // The claim commits the staged placement, so the advice below assumes it.
    + (sp ? ` · assuming #${sp.number} goes at (${sp.x},${sp.y})` : "");
  renderBoards(rec);
  renderLines(rec);

  const pv = $("pv");
  pv.replaceChildren();
  (rec.pv || []).forEach((step) => {
    const li = document.createElement("li");
    li.textContent = step;
    pv.appendChild(li);
  });
}

function renderDump(state) {
  const d = state.dump;
  $("dump-btn").disabled = !!(d && d.busy);
  const st = $("dump-status");
  if (!d) { st.className = ""; st.textContent = ""; }
  else if (d.busy) { st.className = ""; st.textContent = "dumping…"; }
  else if (d.error) { st.className = "err"; st.textContent = "dump failed: " + d.error; }
  else {
    st.className = "ok";
    st.textContent = "saved ✓ " + (d.path || "")
      + (d.bytes ? " (" + Math.round(d.bytes / 1024) + " KB)" : "");
  }
}

async function refresh() {
  const { state } = await chrome.storage.session.get("state");
  render(state || {});
  renderDump(state || {});
}

chrome.runtime.onMessage.addListener((msg) => {
  if (msg && msg.from === "bg" && msg.kind === "state") refresh();
});
chrome.storage.local.get("server").then(({ server }) => {
  if (server) $("server").value = server;
});
setInterval(refresh, 5000); // freshness counter + a safety net for missed messages
send({ kind: "refresh" }); // pull config + checkpoint list on open
refresh();
