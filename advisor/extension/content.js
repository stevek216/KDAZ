// Isolated-world bridge + on-board highlight painter. Runs in EVERY frame: BGA nests the live
// client in an iframe, and a MAIN-world agent can only postMessage to its OWN window, so each
// frame needs its own relay or that frame's agent is mute. Duplicate hellos and repeated
// highlight painting across frames are harmless (both are idempotent).
//
//  - Relays the MAIN-world page agent's messages to the service worker.
//  - Paints the recommended move on the board: a pulsing outline on the domino to claim, or on
//    the two cells a placement would occupy. Visual only — the extension never wires handlers
//    to game controls and never clicks anything.
"use strict";

window.addEventListener("message", (ev) => {
  if (ev.source !== window) return; // only our own window's page agent
  const msg = ev.data && ev.data.__kdAdvisor;
  if (!msg || typeof msg !== "object") return;
  try {
    chrome.runtime.sendMessage({ from: "page", ...msg });
  } catch (e) {
    // extension reloaded/updated underneath the page -- a page refresh reconnects
  }
});

// ---- highlight painting ----
const HL_CLASS = "kd-advisor-hl";
const HL_GHOST = "kd-advisor-ghost";
const HL_TILE = "kd-advisor-tile";
const STYLE_ID = "kd-advisor-style";
const CSS = "." + HL_CLASS + ` {
  outline: 3px solid #ffd54a !important;
  outline-offset: 2px;
  border-radius: 6px;
  box-shadow: 0 0 14px 4px rgba(255, 213, 74, .7) !important;
  animation: kd-advisor-pulse 1.2s ease-in-out infinite;
  z-index: 5;
}
@keyframes kd-advisor-pulse {
  50% { box-shadow: 0 0 24px 9px rgba(255, 213, 74, .35) !important; }
}
.` + HL_GHOST + ` {
  position: absolute;
  width: 100px;
  height: 100px;
  box-sizing: border-box;
  border: 3px solid #ffd54a;
  border-radius: 6px;
  background: rgba(255, 213, 74, .18);
  box-shadow: 0 0 14px 4px rgba(255, 213, 74, .45);
  pointer-events: none;
  z-index: 400;
  animation: kd-advisor-pulse 1.2s ease-in-out infinite;
}
/* The recommended tile, ghosted onto the board in the orientation it should be played.
   Same box as BGA's own .domino (200x100, origin 100px 50px) so the rotation lands
   identically; the transparency and glow say "suggestion, not placed".

   The art is faded on its OWN layer rather than the whole element, so the outline stays
   crisp while the real domino reads clearly through the preview once it is dragged onto
   the same cells. Fading the wrapper would take the outline down with it and leave the
   marker as washed out as the thing it is marking. */
.` + HL_TILE + ` {
  position: absolute;
  width: 200px;
  height: 100px;
  pointer-events: none;
  z-index: 401;
}
.` + HL_TILE + ` > div {
  position: relative;
  width: 200px;
  height: 100px;
  box-sizing: border-box;
  transform-origin: 100px 50px;
  outline: 3px solid #ffd54a;
  box-shadow: 0 0 12px 3px rgba(255, 213, 74, .45);
}
.` + HL_TILE + ` > div::before {
  content: "";
  position: absolute;
  inset: 0;
  background-image: var(--kd-tile-img);
  background-position: var(--kd-tile-pos);
  /* Low enough that the real domino reads clearly once dragged onto the same cells, high
     enough that the terrains are still legible on an empty one — which is the common case,
     and the whole point of showing the tile rather than two blank outlines. The pulse is
     shallow for the same reason: it should catch the eye, not blink the advice away. */
  opacity: .45;
  animation: kd-advisor-tile-pulse 1.6s ease-in-out infinite;
}
@keyframes kd-advisor-tile-pulse {
  50% { opacity: .3; }
}`;

// Documents that can hold game elements: this page, the #gameIframe BGA renders the client
// into, and any same-origin hotseat iframes (alongside it or nested inside it).
function gameDocs() {
  const docs = [document];
  let giDoc = null;
  try {
    const gi = document.getElementById("gameIframe");
    giDoc = gi && gi.contentDocument;
    if (giDoc) docs.push(giDoc);
  } catch (e) {}
  [document, giDoc].forEach((d) => {
    if (!d) return;
    try {
      d.querySelectorAll('iframe[id^="hotseat_iframe_"]').forEach((f) => {
        try { if (f.contentDocument) docs.push(f.contentDocument); } catch (e) {}
      });
    } catch (e) {}
  });
  return docs;
}

function ensureStyle(doc) {
  if (doc.getElementById(STYLE_ID)) return;
  const st = doc.createElement("style");
  st.id = STYLE_ID;
  st.textContent = CSS;
  (doc.head || doc.documentElement).appendChild(st);
}

function clearHighlights() {
  gameDocs().forEach((doc) => {
    try {
      doc.querySelectorAll("." + HL_CLASS).forEach((el) => el.classList.remove(HL_CLASS));
      doc.querySelectorAll("." + HL_GHOST + ",." + HL_TILE).forEach((el) => el.remove());
    } catch (e) {}
  });
}

// BGA's tile sheet, read off a rendered domino rather than hardcoded: the URL is versioned and
// the 2016/2025 edition preference swaps the file, so whatever the page is already using is
// the right answer.
function spriteImage(doc) {
  const el = doc.querySelector(".domino-background");
  if (!el) return null;
  const img = (doc.defaultView || window).getComputedStyle(el).backgroundImage;
  return img && img !== "none" ? img : null;
}

// Sprite offset for a tile, straight out of kingdomino.js `createDomino`. Verified against a
// live board: #10 sits at -3px -985px.
function spriteOffset(number) {
  return {
    x: -(Math.floor((number - 1) / 10) * 207 + 3),
    y: -(((number - 1) % 10) * 109 + 4),
  };
}

// Where BGA parks a 200x100 tile so it covers the right two cells, per rotation — the offsets
// in `placeDomino`. The tile rotates about its centre, so the 90 degree cases shift by half a
// cell in each axis.
const TILE_OFFSET = [[0, 0], [-50, 50], [-100, 0], [-50, -50]];

// Draw the recommended tile ON the board, in the orientation it should be played. Cells alone
// cannot express this: a tile and its 180-degree flip cover exactly the same two squares, and
// which half lands where is often the whole decision.
function paintTile(doc, host, hl) {
  const img = spriteImage(doc);
  if (!img || !hl.number || !hl.anchor) return false;
  ensureStyle(doc);
  const off = spriteOffset(hl.number);
  const [x, y] = hl.anchor;
  const [dx, dy] = TILE_OFFSET[hl.rotation] || [0, 0];
  const wrap = doc.createElement("div");
  wrap.className = HL_TILE;
  wrap.style.left = x * 100 + dx + "px";
  wrap.style.top = -y * 100 + dy + "px";
  const bg = doc.createElement("div");
  // The sprite rides on custom properties so the stylesheet can paint it into a separate,
  // faded layer (see the ::before rule) while this element keeps the crisp outline.
  bg.style.setProperty("--kd-tile-img", img);
  bg.style.setProperty("--kd-tile-pos", off.x + "px " + off.y + "px");
  bg.style.transform = "rotate(" + (hl.rotation || 0) * 90 + "deg)";
  wrap.appendChild(bg);
  host.appendChild(wrap);
  return true;
}

// Cells of THIS CLIENT'S OWN kingdom — `map_scrollable_oversurface` is the big board BGA
// renders for you, while other seats get separate, scaled-down `kingdom_<id>` containers. A
// placement for another seat is therefore not painted on the page at all (it would land on
// your board and mean the opposite of what it says); the side panel draws that seat's own
// kingdom with the move on it instead.
//
// BGA lays the board out at 100px per cell with the castle at the origin and +y going UP the
// screen (kingdomino.js: `left: x*100, top: -y*100`), and renders a clickable
// `square_<x>_<y>` for every cell with a legal placement — so on your own turn the square
// usually already exists and can simply be outlined.
function paintCells(hl) {
  for (const doc of gameDocs()) {
    let host = null;
    try { host = doc.getElementById("map_scrollable_oversurface"); } catch (e) {}
    if (!host) continue;
    ensureStyle(doc);
    let painted = 0;
    for (const c of hl.cells) {
      const [x, y] = Array.isArray(c) ? c : [c.x, c.y];
      const square = doc.getElementById("square_" + x + "_" + y);
      if (square) {
        square.classList.add(HL_CLASS);
        painted++;
        continue;
      }
      // No pre-rendered square (a cell BGA did not offer, or another seat's turn): draw a
      // ghost at the same coordinates rather than silently pointing at nothing.
      const ghost = doc.createElement("div");
      ghost.className = HL_GHOST;
      ghost.style.left = x * 100 + "px";
      ghost.style.top = -y * 100 + "px";
      host.appendChild(ghost);
      painted++;
    }
    // The outlined cells say WHERE; the ghosted tile on top says WHICH WAY ROUND.
    paintTile(doc, host, hl);
    if (painted) return;
  }
}

function paintDomino(number) {
  for (const doc of gameDocs()) {
    const el = doc.getElementById("domino_" + number);
    if (el) {
      ensureStyle(doc);
      el.classList.add(HL_CLASS);
      return;
    }
  }
}

function applyHighlight(hl) {
  clearHighlights();
  if (!hl) return;
  try {
    // The draft line is shared, so a claim is always safe to point at; a placement is only
    // painted when it is this client's own (see paintCells).
    if (hl.kind === "domino") paintDomino(hl.number);
    else if (hl.kind === "cells" && hl.own) paintCells(hl);
  } catch (e) {}
}

chrome.runtime.onMessage.addListener((msg) => {
  if (msg && msg.kind === "hl") applyHighlight(msg.hl);
  // Debug-dump request from the side panel (via the service worker): relay it into the MAIN
  // world, where the page agent can see gameui; the reply rides the normal bridge above.
  if (msg && msg.kind === "dump-req") window.postMessage({ __kdAdvisorDumpReq: 1 }, "*");
});

// Announce this tab on load, so a debug dump can find the game tab even when capture is
// broken and no snapshot was ever posted — exactly the case worth dumping.
try {
  chrome.runtime.sendMessage({ from: "page", kind: "hello" }).catch(() => {});
} catch (e) {}
