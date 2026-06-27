"use strict";
// Kingdomino front-end. Renders the public game state from the server and handles the human's
// claim/place interactions. The engine (via the server) is the single source of truth — this
// file never decides legality; it only offers the actions the server says are legal.

const SHEET = "assets/tiles-2025.webp", SHEET_W = 1035, SHEET_H = 1090, TILE = 100;
const STORE = 13, CENTER = 6;
const DIRS = [[-1, 0], [0, 1], [1, 0], [0, -1]]; // rot 0=up,1=right,2=down,3=left

// (terrain,crowns) -> [domino number, side(0=left,1=right)] — single-square crop in the sheet.
const SQ = {
  "0,0": [1, 0], "0,1": [19, 0],
  "1,0": [3, 0], "1,1": [24, 0],
  "2,0": [7, 0], "2,1": [30, 0],
  "3,0": [10, 0], "3,1": [36, 1], "3,2": [41, 1],
  "4,0": [12, 0], "4,1": [38, 1], "4,2": [43, 1],
  "5,0": [23, 1], "5,1": [40, 0], "5,2": [45, 0], "5,3": [48, 1],
};
const tileXY = n => [3 + 207 * Math.floor((n - 1) / 10), 4 + 109 * ((n - 1) % 10)];

function applySquare(el, terrain, crowns, px) {
  const m = SQ[`${terrain},${crowns}`]; if (!m) return;
  const [n, side] = m, [x0, y0] = tileXY(n), x = x0 + 100 * side, f = px / TILE;
  el.style.backgroundImage = `url(${SHEET})`;
  el.style.backgroundSize = `${SHEET_W * f}px ${SHEET_H * f}px`;
  el.style.backgroundPosition = `${-x * f}px ${-y0 * f}px`;
}
function applyDomino(el, n, px) {
  const [x0, y0] = tileXY(n), f = px / TILE;
  el.style.backgroundImage = `url(${SHEET})`;
  el.style.backgroundSize = `${SHEET_W * f}px ${SHEET_H * f}px`;
  el.style.backgroundPosition = `${-x0 * f}px ${-y0 * f}px`;
  el.style.backgroundRepeat = "no-repeat";
}
const cellPx = small =>
  parseInt(getComputedStyle(document.documentElement).getPropertyValue(small ? "--cell-sm" : "--cell"));

// ---- state ----
let S = null, rot = 0;
let legalPlace = {}, placeByRot = {}, claimSlots = {}, discardIndex = null;
let youCells = {}, ghostCells = null;

const $ = id => document.getElementById(id);
const yourTurn = () => S && !S.terminal && S.to_act === S.human_seat;
const oppSeat = () => 1 - S.human_seat;

async function api(path, body) {
  const r = await fetch(path, {
    method: body ? "POST" : "GET",
    headers: { "Content-Type": "application/json" },
    body: body ? JSON.stringify(body) : undefined,
  });
  return r.json();
}

function addLog(text, cls) {
  const d = document.createElement("div");
  d.className = "logline " + (cls || "sys");
  d.textContent = text;
  $("log-entries").appendChild(d);
}

// ---- apply a new server state ----
function applyState(state) {
  if (state.error) { addLog("error: " + state.error, "sys"); return; }
  S = state;
  rot = 0;
  legalPlace = {}; placeByRot = { 0: new Set(), 1: new Set(), 2: new Set(), 3: new Set() };
  claimSlots = {}; discardIndex = null;
  for (const a of (S.legal || [])) {
    if (a.type === "place") {
      legalPlace[`${a.row}_${a.col}_${a.rot}`] = a.index;
      placeByRot[a.rot].add(`${a.row}_${a.col}`);
    } else if (a.type === "claim") claimSlots[a.slot] = a.index;
    else if (a.type === "discard") discardIndex = a.index;
  }
  // start on a rotation that has legal anchors so the board shows options immediately.
  if (S.phase === "place" && placeByRot[rot].size === 0)
    rot = [0, 1, 2, 3].find(r => placeByRot[r].size) ?? 0;

  for (const e of (S.events || [])) addLog(e.text, "opp");
  render();
  if (S.terminal) showOverlay();
}

// ---- render ----
function render() {
  renderStatus();
  renderScores();
  renderDraft();
  renderBoard("board-opp", oppSeat(), true, false);
  renderBoard("board-you", S.human_seat, false, yourTurn() && S.phase === "place");
  renderHand();
}

function renderStatus() {
  let msg;
  if (S.terminal) msg = "Game over";
  else if (yourTurn()) msg = S.phase === "place" ? "Your turn — place your tile"
    : "Your turn — claim a tile";
  else msg = "Opponent is thinking…";
  $("status").textContent = `${msg}  ·  round ${Math.min(S.round + 1, 12)}/12  ·  ${S.deck_remaining} tiles left`;
  $("opp-name").textContent = S.opponent || "Opponent";
}

function fmtBreakdown(sc) {
  let s = `${sc.crown_score} pts`;
  if (sc.harmony) s += " +5 harmony";
  if (sc.middle_kingdom) s += " +10 middle";
  s += `  ·  largest territory ${sc.largest_territory}`;
  return s;
}
function renderScores() {
  const you = S.scores[S.human_seat], opp = S.scores[oppSeat()];
  $("score-you").textContent = you.total;
  $("score-opp").textContent = opp.total;
  $("breakdown-you").textContent = fmtBreakdown(you);
  $("breakdown-opp").textContent = fmtBreakdown(opp);
}

function renderDraft() {
  drawLine("current-line", S.current_line, S.phase === "start_claim");
  drawLine("next-line", S.next_line, S.phase === "claim");
}
function drawLine(elId, line, isClaimLine) {
  const el = $(elId); el.innerHTML = "";
  const H = 52, W = 104;
  for (const slot of line) {
    const d = document.createElement("div");
    d.className = "slot"; d.style.width = W + "px"; d.style.height = H + "px";
    if (slot.domino == null) { d.classList.add("empty"); el.appendChild(d); continue; }
    applyDomino(d, slot.number, H);
    if (slot.owner != null) {
      const k = document.createElement("div"); k.className = "king";
      k.style.background = slot.owner === S.human_seat ? "var(--you)" : "var(--opp)";
      d.appendChild(k);
    }
    if (isClaimLine && yourTurn() && claimSlots[slot.slot] !== undefined) {
      d.classList.add("claimable");
      d.onclick = () => { addLog(`You claimed domino ${slot.number}`, "you"); move(claimSlots[slot.slot]); };
    }
    el.appendChild(d);
  }
}

function renderBoard(elId, seat, small, interactive) {
  const el = $(elId);
  el.innerHTML = "";
  el.className = "kingdom" + (small ? " small" : "") + (interactive ? " interactive" : "");
  const px = cellPx(small);
  const cm = {};
  for (const c of S.seats[seat].cells) cm[`${c.r},${c.c}`] = c;
  // flash the opponent's most recent placement
  const lastPlace = small ? [...(S.events || [])].reverse().find(e => e.type === "place") : null;
  const flash = new Set();
  if (lastPlace) {
    flash.add(`${lastPlace.row},${lastPlace.col}`);
    const [dr, dc] = DIRS[lastPlace.rot];
    flash.add(`${lastPlace.row + dr},${lastPlace.col + dc}`);
  }
  // cells covered by ANY legal placement at the current rotation (both squares, not just anchor)
  const footprint = new Set();
  if (interactive) {
    const [dr, dc] = DIRS[rot];
    for (const key of placeByRot[rot]) {
      const [r, c] = key.split("_").map(Number);
      footprint.add(`${r}_${c}`); footprint.add(`${r + dr}_${c + dc}`);
    }
  }
  const map = {};
  for (let r = 0; r < STORE; r++) for (let c = 0; c < STORE; c++) {
    const cell = document.createElement("div");
    cell.className = "cell";
    if (r === CENTER && c === CENTER) {
      cell.classList.add("castle"); cell.textContent = "🏰";
      cell.style.color = seat === S.human_seat ? "#274c8a" : "#8a2418";
    } else if (cm[`${r},${c}`]) {
      const sq = cm[`${r},${c}`];
      applySquare(cell, sq.terrain, sq.crowns, px);
      if (sq.crowns > 0) {
        const b = document.createElement("span"); b.className = "crownbadge";
        b.textContent = "👑" + (sq.crowns > 1 ? sq.crowns : "");
        cell.appendChild(b);
      }
    } else {
      cell.classList.add("empty");
      if (interactive && footprint.has(`${r}_${c}`)) {
        cell.classList.add("legal");
        cell.onclick = () => clickPlace(r, c);
        cell.onmouseenter = () => hoverPlace(r, c);
        cell.onmouseleave = clearGhost;
      }
    }
    if (flash.has(`${r},${c}`)) cell.classList.add("lastplace");
    el.appendChild(cell);
    map[`${r},${c}`] = cell;
  }
  if (interactive) { youCells = map; ghostCells = null; }
}

function renderHand() {
  const hand = $("hand");
  if (yourTurn() && S.phase === "place" && S.current_domino) {
    hand.classList.remove("hidden");
    const hd = $("hand-domino"); hd.style.width = "104px"; hd.style.height = "52px";
    applyDomino(hd, S.current_domino.number, 52);
    hd.style.transform = `rotate(${((rot + 3) % 4) * 90}deg)`;
    const noSpot = Object.keys(legalPlace).length === 0;
    $("rotate").classList.toggle("hidden", noSpot);
    const disc = $("discard");
    if (noSpot && discardIndex !== null) {
      disc.classList.remove("hidden");
      disc.onclick = () => { addLog("You discarded (no legal spot)", "you"); move(discardIndex); };
    } else disc.classList.add("hidden");
  } else hand.classList.add("hidden");
}

// ---- placement helpers ----
// A footprint cell is either a placement's anchor or its partner; resolve to the placement
// (preferring an anchor interpretation), so hovering/clicking any covered cell works.
function resolvePlacement(r, c) {
  const a = legalPlace[`${r}_${c}_${rot}`];
  if (a !== undefined) return { index: a, ar: r, ac: c };
  const [dr, dc] = DIRS[rot], ar = r - dr, ac = c - dc;
  const p = legalPlace[`${ar}_${ac}_${rot}`];
  if (p !== undefined) return { index: p, ar, ac };
  return null;
}
function hoverPlace(r, c) { const p = resolvePlacement(r, c); if (p) showGhostAt(p.ar, p.ac); }
function clickPlace(r, c) {
  const p = resolvePlacement(r, c);
  if (p) { addLog(`You placed domino ${S.current_domino.number}`, "you"); move(p.index); }
}
function showGhostAt(ar, ac) {
  const [dr, dc] = DIRS[rot];
  ghostCells = [[ar, ac, S.current_domino.a], [ar + dr, ac + dc, S.current_domino.b]];
  for (const [rr, cc, sq] of ghostCells) {
    const el = youCells[`${rr},${cc}`];
    if (el) { applySquare(el, sq.terrain, sq.crowns, cellPx(false)); el.classList.add("ghost"); el.classList.remove("empty"); }
  }
}
function clearGhost() {
  for (const [rr, cc] of (ghostCells || [])) {
    const el = youCells[`${rr},${cc}`];
    if (el) { el.style.backgroundImage = ""; el.classList.remove("ghost"); el.classList.add("empty"); }
  }
  ghostCells = null;
}
function rotate() {
  if (!(yourTurn() && S.phase === "place")) return;
  const opts = [0, 1, 2, 3].filter(r => placeByRot[r].size);
  if (!opts.length) return;
  rot = opts[(opts.indexOf(rot) + 1) % opts.length];
  clearGhost();
  renderBoard("board-you", S.human_seat, false, true);
  renderHand();
}

// ---- actions ----
async function move(index) {
  applyState(await api("/api/move", { index }));
}
async function newGame() {
  $("log-entries").innerHTML = "";
  $("overlay").classList.add("hidden");
  applyState(await api("/api/new", {
    human_seat: parseInt($("seat").value),
    harmony: $("harmony").checked,
    middle_kingdom: $("middle").checked,
  }));
}

// ---- end-of-game overlay ----
function showOverlay() {
  const v = S.terminal_value || [0, 0];
  const winner = v[S.human_seat] > v[oppSeat()] ? "You win!"
    : v[S.human_seat] < v[oppSeat()] ? "Opponent wins" : "Shared victory";
  $("overlay-title").textContent = winner;
  const row = (label, sc, cls) => `<tr>
      <td class="${cls}">${label}</td>
      <td>${sc.crown_score} crowns${sc.harmony ? " +5" : ""}${sc.middle_kingdom ? " +10" : ""}</td>
      <td class="tot">${sc.total}</td></tr>`;
  $("overlay-body").innerHTML = `<table class="result-table">
      ${row("You", S.scores[S.human_seat], v[S.human_seat] >= v[oppSeat()] ? "winner" : "")}
      ${row(S.opponent || "Opponent", S.scores[oppSeat()], v[oppSeat()] > v[S.human_seat] ? "winner" : "")}
    </table>`;
  $("overlay").classList.remove("hidden");
}

// ---- wire up ----
$("newgame").onclick = newGame;
$("overlay-new").onclick = newGame;
$("rotate").onclick = rotate;
document.addEventListener("keydown", e => { if (e.key === "r" || e.key === "R") rotate(); });
api("/api/state").then(applyState);
