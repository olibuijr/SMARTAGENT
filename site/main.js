// SMARTAGENT pixel world — a playable 8-bit night level built on three.js
// WebGPU (WebGL fallback is automatic). All art is generated at runtime:
// the agent faces are the exact TUI sidebar sprites (faces.js port of
// gen-faces.ts), everything else is drawn onto canvas textures. No DOM UI.
import * as THREE from "three";
import { faceGrid, FACE_W, FACE_H } from "./faces.js";

// ── Brand data ──────────────────────────────────────────────────────────────
const AGENTS = [
	{ key: "linus", name: "LINUS TORVALDS", spec: "TEAM LEAD", accent: "#ffaf5f", line: "Coordinates the fleet. Merges nothing without taste." },
	{ key: "ada", name: "ADA LOVELACE", spec: "BACKEND EXPERT", accent: "#ff87d7", line: "Wrote the first program. Now writes your APIs." },
	{ key: "dennis", name: "DENNIS RITCHIE", spec: "SYSTEMS EXPERT", accent: "#5fd7d7", line: "Everything here sits on ideas he started." },
	{ key: "woz", name: "STEVE WOZNIAK", spec: "FRONTEND EXPERT", accent: "#afd787", line: "Builds the parts you can see. And enjoys it." },
	{ key: "margaret", name: "MARGARET HAMILTON", spec: "DATABASE EXPERT", accent: "#af87ff", line: "Her code landed on the moon. Yours is safe." },
	{ key: "grace", name: "GRACE HOPPER", spec: "QA LEAD", accent: "#87d7ff", line: "Found the first actual bug. Still finding them." },
	{ key: "turing", name: "ALAN TURING", spec: "VERIFICATION EXPERT", accent: "#5fd7d7", line: "Decides what halts. Approves what ships." },
	{ key: "ken", name: "KEN THOMPSON", spec: "INFRASTRUCTURE EXPERT", accent: "#ffaf5f", line: "Keeps the pipes named well and running." },
];
const FACTS = [
	"27 PURE-RUST CRATES",
	"ZERO DEPENDENCIES",
	"22 AGENT TOOLS",
	"OWN VECTOR DB: SEMDB",
	"8 AUTONOMOUS AGENTS",
	"KANBAN-DRIVEN FLEET",
	"SELF-CREATED SKILLS",
	"STD-ONLY. FROM SCRATCH.",
];
const REPO = "https://github.com/olibuijr/SMARTAGENT";
const INSTALL_CMD = "git clone https://github.com/olibuijr/SMARTAGENT && SMARTAGENT/install.sh my-agent";
const BG = "#161620", PANEL = "#262626", SKINC = "#fccda5";
const reduceMotion = matchMedia("(prefers-reduced-motion: reduce)").matches;
const isTouch = matchMedia("(pointer: coarse)").matches;

// ── Level metrics (world units = pixels) ────────────────────────────────────
const VIEW_H = 270;
const GROUND = 48;            // ground top y
const STATION_GAP = 340;
const LEVEL_START = 300;
const LEVEL_END = LEVEL_START + AGENTS.length * STATION_GAP + 420;
const BLOCK_Y = GROUND + 88;  // ?-block altitude (reachable by jump)

// ── Canvas-texture helpers ──────────────────────────────────────────────────
function cv(w, h) {
	const c = document.createElement("canvas");
	c.width = w; c.height = h;
	const x = c.getContext("2d");
	x.imageSmoothingEnabled = false;
	return [c, x];
}
function tex(canvas) {
	const t = new THREE.CanvasTexture(canvas);
	t.magFilter = THREE.NearestFilter;
	t.minFilter = THREE.NearestFilter;
	t.colorSpace = THREE.SRGBColorSpace;
	return t;
}
function plane(canvas, scale = 1, transparent = true) {
	const t = tex(canvas);
	const m = new THREE.Mesh(
		new THREE.PlaneGeometry(canvas.width * scale, canvas.height * scale),
		new THREE.MeshBasicMaterial({ map: t, transparent })
	);
	m.userData.canvas = canvas;
	return m;
}
function overlay(mesh, z = 40) {
	// speech/UI planes: pull forward and skip the depth test so 3D world
	// geometry (blocks, lamps, buildings) can never clip them
	mesh.position.z = z;
	mesh.material.depthTest = false;
	mesh.renderOrder = 10;
	return mesh;
}
const FONT = '"Press Start 2P"';
function textCanvas(str, size, color, { bg = null, pad = 0, glow = null } = {}) {
	const [, mx] = cv(8, 8);
	mx.font = `${size}px ${FONT}`;
	const w = Math.ceil(mx.measureText(str).width) + pad * 2;
	let h = size + pad * 2 + Math.ceil(size * 0.3);
	if (h % 2) h++; // even height — odd sizes sit on half-texels and garble
	// WebGPU texture uploads need 256-byte-aligned rows; odd canvas widths
	// skew into garbled glyphs. Pad to 64px (RGBA) and center the box.
	const cw = Math.ceil(w / 64) * 64;
	const [c, x] = cv(cw, h);
	const ox = Math.floor((cw - w) / 2);
	if (bg) { x.fillStyle = bg; x.fillRect(ox, 0, w, h); }
	x.font = `${size}px ${FONT}`;
	x.textBaseline = "top";
	if (glow) { x.shadowColor = glow; x.shadowBlur = size / 2; }
	x.fillStyle = color;
	x.fillText(str, ox + pad, pad + 1);
	return c;
}
function gridCanvas(g, w, h) {
	const [c, x] = cv(w, h);
	for (let y = 0; y < h; y++) for (let xx = 0; xx < w; xx++) {
		const p = g[y][xx];
		if (!p) continue;
		x.fillStyle = `rgb(${p[0]},${p[1]},${p[2]})`;
		x.fillRect(xx, y, 1, 1);
	}
	return c;
}

// ── Sprite art (all procedural) ─────────────────────────────────────────────
function agentSprite(key, accent) {
	// face 24x21 at 2x + pixel body → 48x76 sprite
	const face = gridCanvas(faceGrid(key), FACE_W, FACE_H);
	const [c, x] = cv(48, 76);
	x.drawImage(face, 0, 0, 24, 21, 0, 0, 48, 42);
	// body: hoodie in the agent's accent, dark trousers
	x.fillStyle = accent;
	x.fillRect(10, 42, 28, 20);          // torso
	x.fillRect(4, 44, 6, 12); x.fillRect(38, 44, 6, 12); // arms
	x.fillStyle = SKINC;
	x.fillRect(4, 56, 6, 4); x.fillRect(38, 56, 6, 4);   // hands
	x.fillStyle = "#1c1c24";
	x.fillRect(12, 62, 10, 10); x.fillRect(26, 62, 10, 10); // legs
	x.fillStyle = "#3a3a46";
	x.fillRect(10, 70, 12, 6); x.fillRect(26, 70, 12, 6);   // boots
	return c;
}
function playerFrames() {
	// The visiting agent: a small terminal-green robot, 3 frames (idle/step)
	const frames = [];
	for (let f = 0; f < 3; f++) {
		const [c, x] = cv(26, 32);
		x.fillStyle = "#2a2a34";
		x.fillRect(3, 2, 20, 16);                    // head/monitor
		x.fillStyle = "#0f0f14";
		x.fillRect(5, 4, 16, 12);                    // screen
		x.fillStyle = "#7dff9a";                     // terminal-green face
		x.fillRect(8, 8, 3, 3); x.fillRect(15, 8, 3, 3);
		x.fillRect(9, 13, 8, 2);
		x.fillStyle = "#3a3a46";
		x.fillRect(11, 18, 4, 2);                    // neck
		x.fillStyle = "#454552";
		x.fillRect(6, 20, 14, 8);                    // body
		x.fillStyle = "#7dff9a";
		x.fillRect(12, 22, 2, 4);                    // power light
		x.fillStyle = "#2a2a34";
		const l = f === 1 ? 2 : f === 2 ? -2 : 0;    // walk cycle
		x.fillRect(7 + l, 28, 5, 4); x.fillRect(14 - l, 28, 5, 4);
		x.fillStyle = "#5a5a68";
		x.fillRect(2, 21, 4, 6); x.fillRect(20, 21, 4, 6); // arms
		frames.push(c);
	}
	return frames;
}
function groundStrip(w) {
	const [c, x] = cv(w, GROUND);
	x.fillStyle = "#2e2a33"; x.fillRect(0, 0, w, GROUND);
	x.fillStyle = "#3d3746"; x.fillRect(0, 0, w, 4);           // lit lip
	x.fillStyle = "#262230";
	for (let row = 0; row < 3; row++)
		for (let bx = (row % 2) * 12; bx < w; bx += 24)
			x.strokeStyle = "#211d29", x.strokeRect(bx + .5, 6 + row * 14 + .5, 24, 14);
	return c;
}
function qBlock(lit = true) {
	const [c, x] = cv(24, 24);
	x.fillStyle = lit ? "#ffaf5f" : "#4a4452";
	x.fillRect(0, 0, 24, 24);
	x.fillStyle = lit ? "#c97f2f" : "#332e3a";
	x.fillRect(0, 0, 24, 2); x.fillRect(0, 22, 24, 2);
	x.fillRect(0, 0, 2, 24); x.fillRect(22, 0, 2, 24);
	for (const [px, py] of [[3, 3], [18, 3], [3, 18], [18, 18]]) x.fillRect(px, py, 3, 3);
	if (lit) {
		x.fillStyle = "#262626"; x.font = `10px ${FONT}`; x.textBaseline = "top";
		x.fillText("?", 8, 8);
	}
	return c;
}
function lampPost(accent) {
	const [c, x] = cv(20, 96);
	x.fillStyle = "#3a3a46";
	x.fillRect(8, 14, 4, 82);
	x.fillStyle = accent;
	x.fillRect(4, 2, 12, 12);
	x.fillStyle = "#fff";
	x.fillRect(7, 5, 6, 6);
	return c;
}
function glowDisc(accent, r) {
	const [c, x] = cv(r * 2, r * 2);
	const g = x.createRadialGradient(r, r, 2, r, r, r);
	g.addColorStop(0, accent + "55");
	g.addColorStop(1, accent + "00");
	x.fillStyle = g;
	x.fillRect(0, 0, r * 2, r * 2);
	return c;
}
function castle() {
	const [c, x] = cv(120, 120);
	x.fillStyle = "#3a3440";
	x.fillRect(10, 40, 100, 80);
	x.fillRect(0, 30, 30, 20); x.fillRect(90, 30, 30, 20);
	for (let bx = 0; bx < 120; bx += 20) x.fillRect(bx, 20, 12, 14);   // crenellation
	x.fillStyle = "#262230";
	x.fillRect(48, 72, 24, 48);                                        // gate
	x.beginPath(); x.arc(60, 74, 12, Math.PI, 0); x.fill();
	x.fillStyle = "#7dff9a";                                           // lit windows
	x.fillRect(24, 52, 8, 10); x.fillRect(88, 52, 8, 10);
	return c;
}
function skylineStrip() {
	const [c, x] = cv(480, 100);
	let bx = 0;
	// deterministic pseudo-random so every load is the same city
	let seed = 7;
	const rnd = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
	while (bx < 480) {
		const bw = 24 + Math.floor(rnd() * 40), bh = 30 + Math.floor(rnd() * 66);
		x.fillStyle = "#20202c";
		x.fillRect(bx, 100 - bh, bw, bh);
		x.fillStyle = rnd() > .5 ? "#43536b" : "#3f4c40";
		for (let wy = 100 - bh + 6; wy < 94; wy += 10)
			for (let wx = bx + 4; wx < bx + bw - 4; wx += 8)
				if (rnd() > .6) x.fillRect(wx, wy, 3, 4);
		bx += bw + 4 + Math.floor(rnd() * 12);
	}
	return c;
}
function hillsStrip() {
	const [c, x] = cv(480, 60);
	x.fillStyle = "#1b1b26";
	for (let i = 0; i < 6; i++) {
		const cx = 40 + i * 84, r = 34 + (i % 3) * 12;
		x.beginPath(); x.arc(cx, 60 + 8, r, Math.PI, 0); x.fill();
	}
	return c;
}
function cloud() {
	const [c, x] = cv(56, 20);
	x.fillStyle = "#2c2c3a";
	x.fillRect(8, 8, 40, 8);
	x.fillRect(16, 2, 16, 8); x.fillRect(34, 4, 10, 6);
	x.fillRect(2, 10, 8, 6); x.fillRect(46, 10, 8, 6);
	return c;
}
function coin() {
	const [c, x] = cv(12, 14);
	x.fillStyle = "#ffd75f"; x.fillRect(2, 0, 8, 14);
	x.fillStyle = "#c9972f"; x.fillRect(2, 0, 2, 14);
	x.fillStyle = "#fff3c4"; x.fillRect(8, 0, 2, 14);
	return c;
}
function signBoard(w, h, accent) {
	const [c, x] = cv(w, h);
	x.fillStyle = PANEL; x.fillRect(0, 0, w, h);
	x.strokeStyle = accent; x.lineWidth = 2;
	x.strokeRect(1, 1, w - 2, h - 2);
	return [c, x];
}

// ── Boot ────────────────────────────────────────────────────────────────────
const canvas = document.getElementById("g");
const renderer = new THREE.WebGPURenderer({ canvas, antialias: false });
try {
	await renderer.init();
} catch (e) {
	// No WebGPU and no WebGL2 (ancient browser / headless): reveal the
	// semantic content instead of a black screen.
	document.querySelector(".sr").removeAttribute("class");
	canvas.remove();
	document.body.style.cssText = "background:#161620;color:#fccda5;font:14px/1.7 monospace;padding:2rem;overflow:auto";
	throw e;
}
await document.fonts.load(`8px ${FONT}`).catch(() => {});
renderer.setClearColor(new THREE.Color(BG));

const scene = new THREE.Scene();
// Perspective diorama: the gameplay plane sits at z=0 and spans VIEW_H
// vertically, exactly like the old ortho view — but depth is now real.
const FOV = 38;
const TAN = Math.tan((FOV / 2) * Math.PI / 180);
const camera = new THREE.PerspectiveCamera(FOV, 16 / 9, 10, 2600);
scene.add(camera);
// landscape: 270-tall retro view. portrait: hold world WIDTH at 420 and let
// the view grow tall — more sky and street, nothing stretches.
let hudRef = null; // set once the HUD group exists; resize() repositions it
let ctrlBand = 0;  // reserved controller shelf height (touch portrait)
let topBand = 0;   // console bezel above the screen (touch portrait)
let rebuildShellFn = null;
const pillZones = []; // START/SELECT hit zones in world coords
let viewW = 480, viewH = VIEW_H, DIST = (VIEW_H / 2) / TAN, CAM_Y = VIEW_H / 2 - 24;
function resize() {
	// measure the CANVAS, not the window: on phones 100vh ≠ innerHeight
	// (URL bar), and that mismatch silently offset every touch hit-test
	const rect = canvas.getBoundingClientRect();
	const aspect = (rect.width || innerWidth) / (rect.height || innerHeight);
	camera.aspect = aspect;
	camera.updateProjectionMatrix();
	if (aspect < .9) {
		viewW = 420;
		viewH = Math.round(Math.min(1040, 420 / aspect));
	} else {
		viewH = VIEW_H;
		viewW = Math.round(Math.min(760, Math.max(340, viewH * aspect)));
	}
	// Game-Boy layout on touch portrait: the world sits above a dedicated
	// controller shelf instead of the pads floating over the street
	ctrlBand = isTouch && aspect < .9 ? Math.round(Math.min(260, viewH * .29)) : 0;
	topBand = ctrlBand > 0 ? 46 : 0;
	DIST = (viewH / 2) / TAN;
	CAM_Y = viewH / 2 - 24 - ctrlBand;
	camera.position.set(camera.position.x, CAM_Y, DIST);
	if (hudRef) hudRef.position.set(0, -CAM_Y, -DIST);
	scene.fog = new THREE.Fog(new THREE.Color(BG), DIST + 60, DIST + 900);
	renderer.setSize(viewW, viewH, false);
	if (rebuildShellFn) rebuildShellFn();
}
addEventListener("resize", resize);
window.visualViewport?.addEventListener("resize", resize);
resize();
camera.position.set(0, CAM_Y, DIST);

// ── Build world ─────────────────────────────────────────────────────────────
const world = new THREE.Group();
scene.add(world);

// sky bands (deep background — fog-exempt)
{
	const bands = [["#12121c", 2400, 1200], ["#161624", 420, 210], ["#1a1a2c", 230, -10]];
	for (const [col, h, y] of bands) {
		const m = new THREE.Mesh(
			new THREE.PlaneGeometry(LEVEL_END * 4, h),
			new THREE.MeshBasicMaterial({ color: col, fog: false })
		);
		m.position.set(LEVEL_END / 2, y, -900);
		scene.add(m);
	}
}
// stars
const stars = [];
{
	let seed = 31;
	const rnd = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
	const [sc, sx] = cv(2, 2); sx.fillStyle = "#e8e8ff"; sx.fillRect(0, 0, 2, 2);
	for (let i = 0; i < 90; i++) {
		const s = plane(sc, 2);
		s.material.fog = false;
		s.position.set(rnd() * LEVEL_END * 1.4 - 200, 120 + rnd() * 860, -820 + rnd() * 120);
		s.material.opacity = .3 + rnd() * .7;
		s.userData.tw = rnd() * 6.28;
		scene.add(s); stars.push(s);
	}
}
// city: a detailed 2.5D night skyline — real boxes, textured on front AND
// sides, rooftop props, crate-name neon signs, blinking antennas, a moon.
const blinkers = [], neons = [];
{
	let seed = 13;
	const rnd = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
	const flat = (col) => new THREE.MeshBasicMaterial({ color: col });
	// muted facade palette — night-blue, warm-gray, brown-gray
	const FACADES = ["#20202c", "#242028", "#1e222c", "#262229", "#1c1f28"];
	const WINDOW_COLS = ["#43536b", "#3f4c40", "#6b5a43", "#4a4358"];

	function facade(w, h, { storefront = false } = {}) {
		const [c, x] = cv(Math.ceil(w / 4) * 4, Math.ceil(h / 4) * 4);
		const base = FACADES[Math.floor(rnd() * FACADES.length)];
		x.fillStyle = base; x.fillRect(0, 0, c.width, c.height);
		// cornice + roofline trim
		x.fillStyle = "#2c2c3a"; x.fillRect(0, 0, w, 3);
		// per-building window style
		const ww = rnd() > .5 ? 4 : 3, wh = rnd() > .5 ? 5 : 6;
		const gx = ww + 4 + Math.floor(rnd() * 4), gy = wh + 6 + Math.floor(rnd() * 3);
		const litCol = WINDOW_COLS[Math.floor(rnd() * WINDOW_COLS.length)];
		const litRate = .25 + rnd() * .45;
		for (let wy = 8; wy < h - (storefront ? 22 : 10); wy += gy) {
			const darkFloor = rnd() > .85; // whole floor asleep
			for (let wx = 5; wx < w - ww - 3; wx += gx) {
				if (darkFloor || rnd() > litRate) {
					x.fillStyle = "#15151d"; x.fillRect(wx, wy, ww, wh); // dark pane
				} else {
					x.fillStyle = litCol; x.fillRect(wx, wy, ww, wh);
					x.fillStyle = "#0f0f14"; x.fillRect(wx + ww - 1, wy + wh - 2, 1, 2); // mullion nick
					x.fillStyle = litCol;
				}
			}
		}
		if (storefront) {
			// lit ground floor: awning band + door + shop windows
			x.fillStyle = "#101018"; x.fillRect(0, h - 20, w, 20);
			x.fillStyle = rnd() > .5 ? "#6b5a43" : "#43536b";
			for (let wx = 4; wx < w - 12; wx += 18) x.fillRect(wx, h - 16, 10, 10);
			x.fillStyle = "#2c2c3a"; x.fillRect(0, h - 22, w, 3); // awning
		}
		return c;
	}
	function sideFace(w, h) {
		// sparser, dimmer windows on the flank — reads as depth when tilting
		const [c, x] = cv(Math.ceil(w / 4) * 4, Math.ceil(h / 4) * 4);
		x.fillStyle = "#181822"; x.fillRect(0, 0, c.width, c.height);
		x.fillStyle = "#242c3a";
		for (let wy = 10; wy < h - 10; wy += 14)
			for (let wx = 5; wx < w - 6; wx += 12)
				if (rnd() > .82) x.fillRect(wx, wy, 3, 4);
		return c;
	}
	const NEON_WORDS = [
		["SEMDB", "#87d7ff"], ["MEMORY", "#ff87d7"], ["TASKS", "#ffaf5f"],
		["CODEGRAPH", "#afd787"], ["VAULT", "#af87ff"], ["WORKFLOW", "#5fd7d7"],
		["RUST", "#ffaf5f"],
	];
	let neonIdx = 0;

	for (let i = 0; i < 46; i++) {
		const near = i % 3 === 0; // every third building sits in the near row
		const bw = (near ? 60 : 44) + rnd() * 70;
		const bh = (near ? 90 : 70) + rnd() * 190;
		const bd = 36 + rnd() * 40;
		const z = near ? -(70 + rnd() * 90) : -(180 + rnd() * 240);
		const bx = -350 + rnd() * (LEVEL_END + 900);
		const front = new THREE.MeshBasicMaterial({ map: tex(facade(bw, bh, { storefront: near })) });
		const side = new THREE.MeshBasicMaterial({ map: tex(sideFace(bd, bh)) });
		const top = flat("#14141c");
		const b = new THREE.Mesh(new THREE.BoxGeometry(bw, bh, bd), [side, side, top, flat("#101018"), front, flat("#181822")]);
		b.position.set(bx, GROUND + bh / 2 - 6, z - bd / 2);
		scene.add(b);
		const roofY = GROUND + bh - 6, roofZ = z - bd / 2;

		// rooftop furniture: water tank, AC boxes, antenna with blinker
		if (rnd() > .45) {
			const tank = new THREE.Mesh(new THREE.BoxGeometry(10, 12, 10), flat("#2a2530"));
			tank.position.set(bx - bw / 4, roofY + 6, roofZ);
			const legs = new THREE.Mesh(new THREE.BoxGeometry(8, 4, 8), flat("#1b1820"));
			legs.position.set(bx - bw / 4, roofY + 1, roofZ);
			scene.add(tank, legs);
		}
		if (rnd() > .5) {
			const ac = new THREE.Mesh(new THREE.BoxGeometry(8, 5, 8), flat("#232330"));
			ac.position.set(bx + bw / 5, roofY + 2.5, roofZ + 4);
			scene.add(ac);
		}
		if (rnd() > .55) {
			const mastH = 12 + rnd() * 20;
			const mast = new THREE.Mesh(new THREE.BoxGeometry(1.6, mastH, 1.6), flat("#3a3a46"));
			mast.position.set(bx + bw / 3, roofY + mastH / 2, roofZ);
			scene.add(mast);
			const [lc, lx] = cv(2, 2); lx.fillStyle = "#ff5f5f"; lx.fillRect(0, 0, 2, 2);
			const lamp = plane(lc, 2);
			lamp.position.set(bx + bw / 3, roofY + mastH + 2, roofZ + 1);
			lamp.userData.ph = rnd() * 6.28;
			scene.add(lamp);
			blinkers.push(lamp);
		}
		// crate-name neon on select mid-height near buildings
		if (near && neonIdx < NEON_WORDS.length && bh > 120 && rnd() > .35) {
			const [word, col] = NEON_WORDS[neonIdx++];
			const sign = plane(textCanvas(word, 8, col, { glow: col, pad: 4 }));
			sign.position.set(bx, roofY - 14 - rnd() * 30, z + 1);
			sign.userData.base = .95;
			scene.add(sign);
			const halo = plane(glowDisc(col, 40));
			halo.material.blending = THREE.AdditiveBlending;
			halo.material.depthWrite = false;
			halo.position.set(bx, sign.position.y, z + .5);
			neons.push({ sign, halo, ph: rnd() * 9 });
			scene.add(halo);
		}
	}

	// pixel moon with craters + halo
	{
		const R = 26;
		const [mc, mx] = cv(R * 2, R * 2);
		for (let y = 0; y < R * 2; y++) for (let x2 = 0; x2 < R * 2; x2++) {
			const d = Math.hypot(x2 - R, y - R);
			if (d < R) mx.fillStyle = "#d8d8c8", mx.fillRect(x2, y, 1, 1);
		}
		mx.fillStyle = "#b8b8aa";
		for (const [cx, cy, cr] of [[16, 14, 5], [34, 30, 7], [30, 12, 3], [14, 34, 4], [40, 18, 3]]) {
			for (let y = -cr; y <= cr; y++) for (let x2 = -cr; x2 <= cr; x2++)
				if (x2 * x2 + y * y <= cr * cr && Math.hypot(cx + x2 - R, cy + y - R) < R - 1) mx.fillRect(cx + x2, cy + y, 1, 1);
		}
		const moon = plane(mc, 3);
		moon.material.fog = false;
		moon.position.set(LEVEL_START + 1250, 350, -780);
		scene.add(moon);
		const halo = plane(glowDisc("#d8d8c8", 90), 3);
		halo.material.blending = THREE.AdditiveBlending;
		halo.material.depthWrite = false;
		halo.material.fog = false;
		halo.position.set(moon.position.x, moon.position.y, -781);
		scene.add(halo);
	}

	// soft hills silhouette behind the city
	const hil = hillsStrip();
	for (let px = -480; px < LEVEL_END + 960; px += 480) {
		const h = plane(hil, 2.2);
		h.material.fog = false;
		h.position.set(px + 240, GROUND + 44, -560);
		scene.add(h);
	}
	// drifting clouds between city and sky
	const cl = cloud();
	for (let i = 0; i < 14; i++) {
		const c = plane(cl, 1.6);
		c.material.fog = false;
		c.position.set(rnd() * LEVEL_END * 1.2 - 100, GROUND + 220 + rnd() * 160, -480 + rnd() * 140);
		scene.add(c);
	}
}

// ground: one long slab with real depth — front face bricks, walkable top
{
	const g = groundStrip(480);
	const front = new THREE.MeshBasicMaterial({ map: tex(g) });
	front.map.wrapS = THREE.RepeatWrapping;
	front.map.repeat.x = (LEVEL_END + 1920) / 480;
	const top = new THREE.MeshBasicMaterial({ color: "#232028" });
	const side = new THREE.MeshBasicMaterial({ color: "#1b1820" });
	const slab = new THREE.Mesh(
		new THREE.BoxGeometry(LEVEL_END + 1920, GROUND, 520),
		[side, side, top, side, front, side]
	);
	slab.position.set(LEVEL_END / 2, GROUND / 2, -260);
	world.add(slab);
}

// agents + lamps + signs
const stations = [];
AGENTS.forEach((a, i) => {
	const ax = LEVEL_START + (i + 1) * STATION_GAP;
	const spr = plane(agentSprite(a.key, a.accent));
	spr.position.set(ax, GROUND + 38, 1);
	world.add(spr);
	const post = new THREE.Mesh(
		new THREE.BoxGeometry(4, 84, 4),
		new THREE.MeshBasicMaterial({ color: "#3a3a46" })
	);
	post.position.set(ax - 60, GROUND + 42, -8);
	world.add(post);
	const head = new THREE.Mesh(
		new THREE.BoxGeometry(12, 12, 12),
		new THREE.MeshBasicMaterial({ color: a.accent })
	);
	head.position.set(ax - 60, GROUND + 88, -8);
	world.add(head);
	const glow = plane(glowDisc(a.accent, 48));
	glow.material.blending = THREE.AdditiveBlending;
	glow.material.depthWrite = false;
	glow.position.set(ax - 60, GROUND + 88, -1);
	world.add(glow);
	// nameplate (always visible, small)
	const np = overlay(plane(textCanvas(a.name, 8, a.accent, { bg: PANEL, pad: 5 })));
	np.position.set(ax, GROUND + 90, 40);
	world.add(np);
	// dialog card (hidden until near)
	const [dc, dx] = signBoard(256, 64, a.accent);
	dx.font = `8px ${FONT}`; dx.textBaseline = "top";
	dx.fillStyle = a.accent; dx.fillText(a.spec, 8, 9);
	dx.fillStyle = "#e8e2d8";
	// word-wrap the flavor line
	const words = a.line.split(" ");
	let line = "", ly = 26;
	for (const w of words) {
		if (dx.measureText(line + w).width > 238) { dx.fillText(line, 8, ly); ly += 13; line = ""; }
		line += w + " ";
	}
	dx.fillText(line.trim(), 8, ly);
	const card = overlay(plane(dc));
	card.position.set(ax, GROUND + 138, 40);
	card.material.opacity = 0;
	world.add(card);
	stations.push({ x: ax, spr, card, bob: Math.random() * 6.28 });
});

// ?-blocks with facts
const blocks = [];
FACTS.forEach((fact, i) => {
	const bx = LEVEL_START + (i + 1) * STATION_GAP - STATION_GAP / 2;
	const qt = new THREE.MeshBasicMaterial({ map: tex(qBlock(true)) });
	const qs = new THREE.MeshBasicMaterial({ map: tex(qBlock(true)), color: "#9a9a9a" });
	const b = new THREE.Mesh(new THREE.BoxGeometry(24, 24, 24), [qs, qs, qs, qs, qt, qs]);
	b.position.set(bx, BLOCK_Y, 0);
	world.add(b);
	const toast = overlay(plane(textCanvas(fact, 8, "#ffd75f", { bg: PANEL, pad: 5 })));
	toast.position.set(bx, BLOCK_Y + 34, 40);
	toast.material.opacity = 0;
	world.add(toast);
	const cn = plane(coin());
	overlay(cn, 30);
	cn.position.set(bx, BLOCK_Y + 20, 30);
	cn.material.opacity = 0;
	world.add(cn);
	blocks.push({ x: bx, mesh: b, toast, coin: cn, hit: false, anim: 0 });
});

// install terminal: one keypress copies the install command
const INSTALL_X = LEVEL_END - 330;
let copied = 0;
{
	const [tc, tx] = cv(64, 44);
	tx.fillStyle = "#2a2a34"; tx.fillRect(4, 0, 56, 34);      // terminal shell
	tx.fillStyle = "#0f0f14"; tx.fillRect(8, 4, 48, 26);      // screen
	tx.fillStyle = "#7dff9a"; tx.font = `8px ${FONT}`; tx.textBaseline = "top";
	tx.fillText(">_", 12, 8);
	tx.fillStyle = "#3a3a46"; tx.fillRect(26, 34, 12, 6); tx.fillRect(16, 40, 32, 4);
	const term = plane(tc);
	term.position.set(INSTALL_X, GROUND + 22, 1);
	world.add(term);
	const label = plane(textCanvas("INSTALL", 8, "#7dff9a", { bg: PANEL, pad: 5 }));
	overlay(label);
	label.position.set(INSTALL_X, GROUND + 58, 40);
	world.add(label);
	const cmd = plane(textCanvas(INSTALL_CMD, 8, "#e8e2d8", { bg: PANEL, pad: 6 }));
	cmd.scale.setScalar(.62); // long line — shrink to fit the view
	overlay(cmd);
	cmd.position.set(INSTALL_X, GROUND + 110, 40);
	cmd.material.opacity = 0;
	world.add(cmd);
	const copyHint = plane(textCanvas(matchMedia("(pointer: coarse)").matches ? "PRESS B TO COPY" : "PRESS C OR CLICK TO COPY", 8, "#7dff9a", { bg: PANEL, pad: 5 }));
	overlay(copyHint);
	copyHint.position.set(INSTALL_X, GROUND + 84, 40);
	copyHint.material.opacity = 0;
	world.add(copyHint);
	const copiedToast = plane(textCanvas("COPIED! PASTE IT IN YOUR TERMINAL", 8, "#161620", { bg: "#7dff9a", pad: 5 }));
	overlay(copiedToast);
	copiedToast.position.set(INSTALL_X, GROUND + 136, 40);
	copiedToast.material.opacity = 0;
	world.add(copiedToast);
	window.__install = { cmd, copyHint, copiedToast };
}
function nearInstall() { return Math.abs(P.x - INSTALL_X) < 90; }
async function copyInstall() {
	try { await navigator.clipboard.writeText(INSTALL_CMD); } catch {
		const t = document.createElement("textarea");
		t.value = INSTALL_CMD; document.body.appendChild(t);
		t.select(); document.execCommand("copy"); t.remove();
	}
	copied = performance.now();
	coinSound();
}

// castle + flag at the end
{
	const cfront = new THREE.MeshBasicMaterial({ map: tex(castle()), transparent: true });
	const cside = new THREE.MeshBasicMaterial({ color: "#2c2733" });
	const cs = new THREE.Mesh(new THREE.BoxGeometry(120, 120, 90), [cside, cside, cside, cside, cfront, cside]);
	cs.position.set(LEVEL_END - 80, GROUND + 60, -45);
	world.add(cs);
	const pole = plane((() => { const [c, x] = cv(4, 140); x.fillStyle = "#5a5a68"; x.fillRect(0, 0, 4, 140); return c; })());
	pole.position.set(LEVEL_END - 190, GROUND + 70, 0);
	world.add(pole);
	const flag = plane(textCanvas("GITHUB", 8, "#161620", { bg: "#7dff9a", pad: 4 }));
	flag.position.set(LEVEL_END - 160, GROUND + 126, 1);
	world.add(flag);
	const hint = plane(textCanvas(matchMedia("(pointer: coarse)").matches ? "PRESS B TO ENTER THE REPO" : "PRESS UP TO ENTER THE REPO", 8, "#7dff9a", { bg: PANEL, pad: 5 }));
	overlay(hint);
	hint.position.set(LEVEL_END - 120, GROUND + 150, 40);
	hint.material.opacity = 0;
	world.add(hint);
	window.__castleHint = hint;
}

// ── Player ──────────────────────────────────────────────────────────────────
const frames = playerFrames().map((c) => tex(c));
const player = new THREE.Mesh(
	new THREE.PlaneGeometry(26, 32),
	// DoubleSide: scale.x = -1 (facing left) inverts winding and would
	// otherwise get back-face culled — the character vanished walking left.
	new THREE.MeshBasicMaterial({ map: frames[0], transparent: true, side: THREE.DoubleSide })
);
player.position.set(60, GROUND + 16, 3);
world.add(player);
const spawnX = 60;
const P = { x: spawnX, y: 0, vx: 0, vy: 0, onGround: true, face: 1, coins: 0 };

// ── HUD (camera-space) ──────────────────────────────────────────────────────
const hud = new THREE.Group();
camera.add(hud);
hudRef = hud;
resize(); // positions the HUD for the current aspect
// Title: the generated pixel logo (billboard style); text fallback if it fails
const logoTex = await new Promise((res) => {
	const img = new Image();
	img.onload = () => {
		const [c, x] = cv(img.width, img.height);
		x.drawImage(img, 0, 0);
		res(tex(c));
	};
	img.onerror = () => res(null);
	img.src = "logo.png";
});
const title = logoTex
	? new THREE.Mesh(new THREE.PlaneGeometry(384, 128), new THREE.MeshBasicMaterial({ map: logoTex, transparent: true }))
	: plane(textCanvas("SMARTAGENT", 26, SKINC, { glow: "#ffaf5f", pad: 8 }));
const subtitle = plane(textCanvas("THE LEGENDARY DEVELOPER TEAM", 9, "#87d7ff", { pad: 4 }));
const hintCtl = plane(textCanvas(
	matchMedia("(pointer: coarse)").matches
		? "D-PAD WALKS - A JUMPS - B USES"
		: "ARROWS / AD TO WALK - SPACE TO JUMP",
	8, "#8a8a9a", { pad: 4 }));
title.position.set(0, 70, 5);
subtitle.position.set(0, 44, 5);
hintCtl.position.set(0, 22, 5);
hud.add(title, subtitle, hintCtl);
// ── Game Boy controller overlay ──
function pxCircle(x, cx2, cy, r, col) {
	x.fillStyle = col;
	for (let y = -r; y <= r; y++) for (let x2 = -r; x2 <= r; x2++)
		if (x2 * x2 + y * y <= r * r) x.fillRect(cx2 + x2, cy + y, 1, 1);
}
function dpadCanvas(pressed = null) {
	// classic cross: dark body, beveled arms, engraved arrows, center dimple
	const S = 96, A = 32;                    // size, arm thickness
	const [c, x] = cv(S + 32, S);            // row-aligned width
	const ox = 16, m = (S - A) / 2;
	const arm = (px, py, w, h, dir) => {
		const down = pressed === dir;
		x.fillStyle = "#0a0a10"; x.fillRect(ox + px - 2, py - 2, w + 4, h + 4);
		x.fillStyle = down ? "#101018" : "#22222c"; x.fillRect(ox + px, py, w, h);
		if (!down) {
			x.fillStyle = "#3c3c4a";
			x.fillRect(ox + px, py, w, 3); x.fillRect(ox + px, py, 3, h);
		}
	};
	arm(0, m, S, A, null);                    // horizontal bar (base coat)
	arm(m, 0, A, S, null);                    // vertical bar
	arm(0, m, Math.floor(S / 3), A, "left");
	arm(S - Math.floor(S / 3), m, Math.floor(S / 3), A, "right");
	arm(m, 0, A, Math.floor(S / 3), "jump");
	arm(m, S - Math.floor(S / 3), A, Math.floor(S / 3), "down");
	// engraved arrows
	x.fillStyle = "#0e0e14";
	const t = (cx2, cy, dx, dy) => { for (let i = 0; i < 6; i++) x.fillRect(ox + cx2 + dx * i - (dy ? 6 - i : 0), cy + dy * i - (dx ? 6 - i : 0), dy ? 2 * (6 - i) : 2, dx ? 2 * (6 - i) : 2); };
	t(12, S / 2 - 1, -1, 0); t(S - 14, S / 2 - 1, 1, 0); t(S / 2 - 1, 12, 0, -1); t(S / 2 - 1, S - 14, 0, 1);
	pxCircle(x, ox + S / 2, S / 2, 11, "#1a1a24");
	pxCircle(x, ox + S / 2, S / 2, 8, "#14141c");
	return c;
}
function roundBtn(label, pressed = false) {
	// Game Boy magenta face button, engraved letter
	const R = 25;
	const [c, x] = cv(64, 56);
	pxCircle(x, 32, 28, R + 2, "#0a0a10");                       // outline
	pxCircle(x, 32, 28 + (pressed ? 1 : 2), R, "#5e1c36");       // lower body
	pxCircle(x, 32, 28 + (pressed ? 1 : 0) - 2, R, pressed ? "#7c2848" : "#a13a5e");
	if (!pressed) pxCircle(x, 32 - 7, 28 - 9, 7, "#c25579");     // sheen
	x.fillStyle = "#2a0e1c"; x.font = `14px ${FONT}`; x.textBaseline = "top";
	x.fillText(label, 32 - Math.floor(x.measureText(label).width / 2), 21 + (pressed ? 1 : 0));
	return c;
}
const pads = [];
let dpadMesh = null, dpadMaps = null, shellMesh = null;
if (isTouch) {
	shellMesh = new THREE.Mesh(
		new THREE.PlaneGeometry(1, 1),
		new THREE.MeshBasicMaterial({ transparent: true, depthTest: false })
	);
	shellMesh.renderOrder = 11;
	shellMesh.visible = false;
	hud.add(shellMesh);
	// Draws the whole console around the screen hole: pinstriped top bezel
	// with brand + power LED, inset screen bezel, tagline, START/SELECT
	// pills, speaker grille. Rebuilt on resize/rotation.
	rebuildShellFn = function rebuildShell() {
		pillZones.length = 0;
		if (ctrlBand <= 0) { shellMesh.visible = false; return; }
		const w = viewW, h = viewH;
		const cw = Math.ceil(w / 64) * 64;
		const [c, x] = cv(cw, h);
		const ox = (cw - w) >> 1;
		const holeX = 12, holeY = topBand, holeW = w - 24, holeH = h - ctrlBand - topBand;
		x.fillStyle = "#23232e"; x.fillRect(ox, 0, w, h);                       // shell body
		x.fillStyle = "#2e2e3a"; x.fillRect(ox, 0, w, 2);                       // top edge light
		x.fillStyle = "#5e2a44"; x.fillRect(ox, 7, w, 2);                       // GB pinstripes
		x.fillStyle = "#2a3a5e"; x.fillRect(ox, 11, w, 2);
		x.font = `8px ${FONT}`; x.textBaseline = "top";
		x.fillStyle = "#8a8a9a";
		x.fillText("SMARTAGENT", ox + Math.floor((w - x.measureText("SMARTAGENT").width) / 2), 22);
		pxCircle(x, ox + 20, 27, 4, "#3a1218");                                 // power LED
		pxCircle(x, ox + 20, 26, 2, "#ff4f4f");
		// screen bezel inset
		x.fillStyle = "#0d0d12"; x.fillRect(ox + holeX - 8, holeY - 8, holeW + 16, holeH + 16);
		x.fillStyle = "#050508"; x.fillRect(ox + holeX - 8, holeY - 8, holeW + 16, 2);
		x.fillStyle = "#33333f"; x.fillRect(ox + holeX - 8, holeY + holeH + 4, holeW + 16, 2);
		x.clearRect(ox + holeX, holeY, holeW, holeH);                           // the screen
		const tag = "LEGENDARY DEV SYSTEM";
		x.fillStyle = "#6a6a7a";
		x.fillText(tag, ox + Math.floor((w - x.measureText(tag).width) / 2), holeY + holeH + 12);
		// START / SELECT pills + engraved labels
		const pillY = h - 34;
		for (const [cx2, label, act] of [[w / 2 - 44, "SELECT", "select"], [w / 2 + 44, "START", "start"]]) {
			x.fillStyle = "#0a0a10";
			x.fillRect(ox + cx2 - 16, pillY - 6, 32, 12);
			pxCircle(x, ox + cx2 - 16, pillY, 6, "#0a0a10"); pxCircle(x, ox + cx2 + 16, pillY, 6, "#0a0a10");
			x.fillStyle = "#191922";
			x.fillRect(ox + cx2 - 15, pillY - 4, 30, 8);
			pxCircle(x, ox + cx2 - 15, pillY, 4, "#191922"); pxCircle(x, ox + cx2 + 15, pillY, 4, "#191922");
			x.fillStyle = "#6a6a7a";
			x.fillText(label, ox + cx2 - Math.floor(x.measureText(label).width / 2), pillY + 10);
			pillZones.push({ x: cx2 - w / 2, y: CAM_Y + viewH / 2 - pillY, act });
		}
		// speaker grille: stepped diagonal slots, bottom-right
		for (let i = 0; i < 5; i++)
			for (let j = 0; j < 9; j++) {
				x.fillStyle = "#14141c";
				x.fillRect(ox + w - 76 + i * 11 + j, h - 26 - j * 2, 3, 3);
			}
		const old = shellMesh.material.map;
		shellMesh.material.map = tex(c);
		if (old) old.dispose();
		shellMesh.geometry.dispose();
		shellMesh.geometry = new THREE.PlaneGeometry(cw, h);
		shellMesh.position.set(0, CAM_Y, 4);
		shellMesh.visible = true;
	};
	rebuildShellFn();
	dpadMaps = {
		idle: tex(dpadCanvas()), left: tex(dpadCanvas("left")),
		right: tex(dpadCanvas("right")), jump: tex(dpadCanvas("jump")),
		down: tex(dpadCanvas("down")),
	};
	dpadMesh = overlay(plane(dpadCanvas()), 6);
	dpadMesh.material.opacity = .92;
	dpadMesh.renderOrder = 12;
	hud.add(dpadMesh);
	for (const d of [
		{ label: "B", act: "use", off: 96, lift: 40 },
		{ label: "A", act: "jump", off: 40, lift: 62 },   // A sits higher — GB diagonal
	]) {
		const m = overlay(plane(roundBtn(d.label)), 6);
		const up = m.material.map, down = tex(roundBtn(d.label, true));
		m.material.opacity = .92;
		m.renderOrder = 12;
		hud.add(m);
		pads.push({ ...d, mesh: m, up, down, held: false });
	}
}
// Always-visible install CTA: copy the command from anywhere, any time.
function installBtnCanvas(copiedState) {
	const w = 92, h = 24;
	const [c, x] = cv(128, h);
	const ox = (128 - w) >> 1;
	x.fillStyle = "#0a0a10"; x.fillRect(ox, 0, w, h);                    // outline
	x.fillStyle = copiedState ? "#7dff9a" : "#14201a";                    // face
	x.fillRect(ox + 2, 2, w - 4, h - 4);
	x.strokeStyle = copiedState ? "#161620" : "#7dff9a";
	x.lineWidth = 2; x.strokeRect(ox + 3, 3, w - 6, h - 6);
	x.font = `8px ${FONT}`; x.textBaseline = "top";
	x.fillStyle = copiedState ? "#161620" : "#7dff9a";
	const label = copiedState ? "COPIED!" : "INSTALL";
	x.fillText(label, ox + Math.floor((w - x.measureText(label).width) / 2), 9);
	return c;
}
const installBtn = overlay(plane(installBtnCanvas(false)), 6);
installBtn.renderOrder = 12;
const installBtnMaps = { idle: installBtn.material.map, copied: tex(installBtnCanvas(true)) };
hud.add(installBtn);
let coinHud = plane(textCanvas("FACTS 0/8", 8, "#ffd75f", { pad: 3 }));
hud.add(coinHud);
function refreshCoinHud() {
	hud.remove(coinHud);
	coinHud = plane(textCanvas(`FACTS ${P.coins}/8`, 8, "#ffd75f", { pad: 3 }));
	hud.add(coinHud);
}

// ── Audio (created on first gesture) ───────────────────────────────────────
let AC = null;
function beep(freq, dur = .08, type = "square", vol = .04) {
	if (!AC) return;
	const o = AC.createOscillator(), g = AC.createGain();
	o.type = type; o.frequency.value = freq;
	g.gain.setValueAtTime(vol, AC.currentTime);
	g.gain.exponentialRampToValueAtTime(.0001, AC.currentTime + dur);
	o.connect(g).connect(AC.destination);
	o.start(); o.stop(AC.currentTime + dur);
}
const coinSound = () => { beep(988, .07); setTimeout(() => beep(1319, .18), 70); };

// ── Input ───────────────────────────────────────────────────────────────────
const keys = {};
let started = false, lastInput = 0;
let lastStart = 0;
function start() {
	if (!started) {
		started = true;
		lastStart = performance.now();
		if (!AC) AC = new (window.AudioContext || window.webkitAudioContext)();
	}
	lastInput = performance.now();
}
addEventListener("keydown", (e) => {
	start();
	keys[e.code] = true;
	if (["ArrowUp", "KeyW"].includes(e.code) && nearCastle()) location.href = REPO;
	if (e.code === "KeyC") copyInstall(); // copy works from anywhere
	if (["Space", "ArrowUp", "KeyW"].includes(e.code)) e.preventDefault();
});
addEventListener("keyup", (e) => (keys[e.code] = false));
// touch: on-screen controller (multi-touch — walk while jumping)
let touchDir = 0, touchJump = false;
const activePointers = new Map(); // pointerId -> pad
// map through the canvas rect — NOT innerWidth/innerHeight, which drift
// from the painted canvas when mobile URL bars show/hide
function toView(clientX, clientY) {
	const r = canvas.getBoundingClientRect();
	return [
		((clientX - r.left) / r.width - .5) * viewW,
		(.5 - (clientY - r.top) / r.height) * viewH + CAM_Y,
	];
}
function overInstallBtn(vx, vy) {
	return Math.abs(vx - installBtn.position.x) < 48 && Math.abs(vy - installBtn.position.y) < 14;
}
function padAt(clientX, clientY) {
	const [vx, vy] = toView(clientX, clientY);
	for (const z of pillZones)
		if (Math.abs(vx - z.x) < 26 && Math.abs(vy - z.y) < 16) return { pill: true, act: z.act };
	if (dpadMesh) {
		const dx = vx - dpadMesh.position.x, dy = vy - dpadMesh.position.y;
		if (Math.abs(dx) < 54 && Math.abs(dy) < 54) {
			const act = Math.abs(dx) > Math.abs(dy) ? (dx < 0 ? "left" : "right") : (dy > 0 ? "jump" : "down");
			return { dpad: true, act };
		}
	}
	return pads.find((p) => {
		const dx = vx - p.mesh.position.x, dy = vy - p.mesh.position.y;
		return dx * dx + dy * dy < 30 * 30;
	});
}
function recomputeTouch() {
	touchDir = 0; touchJump = false;
	let dpadAct = null;
	for (const p of activePointers.values()) {
		if (!p) continue;
		if (p.act === "left") touchDir = -1;
		if (p.act === "right") touchDir = 1;
		if (p.act === "jump") touchJump = true;
		if (p.dpad) dpadAct = p.act;
	}
	for (const p of pads) p.held = [...activePointers.values()].includes(p);
	if (dpadMesh) dpadMesh.material.map = dpadMaps[dpadAct ?? "idle"];
}
function doUse() {
	if (nearCastle()) { location.href = REPO; return; }
	if (nearInstall()) copyInstall();
}
canvas.addEventListener("pointerdown", (e) => {
	e.preventDefault(); // no synthetic mouse events / long-press selection
	start();
	{
		const [vx, vy] = toView(e.clientX, e.clientY);
		if (overInstallBtn(vx, vy)) { copyInstall(); return; }
	}
	const p = padAt(e.clientX, e.clientY);
	if (p) {
		if (p.act === "use" || p.act === "start") doUse();
		if (p.act === "select") { P.x = 60; P.y = 0; camX = 60; beep(392, .08); return; }
		if (p.pill) return;
		activePointers.set(e.pointerId, p);
		recomputeTouch();
		return;
	}
	if (!isTouch) doUse(); // desktop click fallback
});
canvas.addEventListener("pointermove", (e) => {
	if (!activePointers.has(e.pointerId)) return;
	activePointers.set(e.pointerId, padAt(e.clientX, e.clientY));
	recomputeTouch();
});
for (const ev of ["pointerup", "pointercancel"])
	addEventListener(ev, (e) => { activePointers.delete(e.pointerId); recomputeTouch(); });

function nearCastle() { return P.x > LEVEL_END - 260; }

// ── Game loop ───────────────────────────────────────────────────────────────
const GRAV = 900, SPEED = 150, JUMP = 350;
let last = performance.now(), walkT = 0, demoDir = 1, camX = spawnX;
let pointerNX = 0, pointerNY = 0, tiltX = 0, tiltY = 0;
addEventListener("pointermove", (e) => {
	if (isTouch) return; // touch pointers drive the pads, not the parallax
	pointerNX = (e.clientX / innerWidth) * 2 - 1;
	pointerNY = (e.clientY / innerHeight) * 2 - 1;
	const [vx, vy] = toView(e.clientX, e.clientY);
	canvas.style.cursor = overInstallBtn(vx, vy) || nearCastle() || nearInstall() ? "pointer" : "default";
});
if (isTouch && "DeviceOrientationEvent" in window)
	addEventListener("deviceorientation", (e) => {
		if (e.gamma == null) return;
		pointerNX = Math.max(-1, Math.min(1, e.gamma / 28));
		pointerNY = Math.max(-1, Math.min(1, (e.beta - 42) / 32));
	});

function step(dt, now) {
	// input → velocity
	let dir = (keys.ArrowRight || keys.KeyD ? 1 : 0) - (keys.ArrowLeft || keys.KeyA ? 1 : 0) + touchDir;
	const jump = keys.Space || keys.ArrowUp || keys.KeyW || touchJump;
	// auto-demo: after 6s idle, the robot wanders on its own (not with reduced motion)
	if (!reduceMotion && now - lastInput > 6000 && started) {
		if (P.x > LEVEL_END - 300) demoDir = -1;
		if (P.x < 200) demoDir = 1;
		dir = demoDir * .6;
	}
	if (dir) lastInput = keys.ArrowRight || keys.ArrowLeft || keys.KeyA || keys.KeyD || touchDir ? now : lastInput;

	P.vx = dir * SPEED;
	if (jump && P.onGround) { P.vy = JUMP; P.onGround = false; beep(523, .1, "triangle"); }
	P.vy -= GRAV * dt;
	P.x = Math.max(20, Math.min(LEVEL_END - 30, P.x + P.vx * dt));
	P.y += P.vy * dt;

	// block head-bump: player top crossing block bottom while rising
	if (P.vy > 0) {
		for (const b of blocks) {
			if (b.hit) continue;
			const playerTop = GROUND + P.y + 32;
			if (Math.abs(P.x - b.x) < 22 && playerTop > BLOCK_Y - 12 && playerTop < BLOCK_Y + 10) {
				b.hit = true; b.anim = 1; P.coins++; refreshCoinHud(); coinSound();
				const dead = new THREE.MeshBasicMaterial({ map: tex(qBlock(false)) });
				b.mesh.material = [dead, dead, dead, dead, dead, dead];
				P.vy = -60;
			}
		}
	}
	// ground
	if (P.y <= 0) { P.y = 0; P.vy = 0; P.onGround = true; }
	if (dir) P.face = dir > 0 ? 1 : -1;

	// animate player
	walkT += Math.abs(P.vx) * dt * .08;
	const frame = !P.onGround ? 1 : Math.abs(P.vx) > 1 ? 1 + (Math.floor(walkT) % 2) : 0;
	player.material.map = frames[frame];
	player.scale.x = P.face;
	player.position.set(Math.round(P.x), Math.round(GROUND + 16 + P.y), 3);

	// camera follow
	const target = Math.max(viewW / 2 - 40, Math.min(LEVEL_END - viewW / 2 + 40, P.x + P.face * 40));
	camX += (target - camX) * Math.min(1, dt * 3);
	// perspective drift: pointer (or walking) tilts the diorama in 3D
	tiltX += ((reduceMotion ? 0 : pointerNX) - tiltX) * Math.min(1, dt * 2.5);
	tiltY += ((reduceMotion ? 0 : pointerNY) - tiltY) * Math.min(1, dt * 2.5);
	const lean = Math.max(-1, Math.min(1, P.vx / SPEED));
	camera.position.x = camX + tiltX * 26 + lean * 10;
	camera.position.y = CAM_Y + 6 - tiltY * 16;
	camera.lookAt(camX - tiltX * 22, CAM_Y - 8 + tiltY * 10, 0);
	// title: lower-third at rest, parks top-left after the first input —
	// positions derive from viewH so portrait and landscape both compose
	{
		const topY = CAM_Y + viewH / 2 - topBand;
		const startY = viewH * .26, parkY = topY - 66;
		const fit = Math.min(1, (viewW - 24) / 384); // logo never overflows narrow views
		const k = !started ? 0 : reduceMotion ? 1 : Math.min(1, (now - lastStart) / 800);
		const e = 1 - Math.pow(1 - k, 4); // ease-out-quart
		title.scale.setScalar((1 - .6 * e) * fit);
		title.position.x = (-viewW / 2 + 86) * e;
		title.position.y = startY + (parkY - startY) * e;
		subtitle.position.y = startY - (logoTex ? 80 : 26) * fit;
		hintCtl.position.y = startY - (logoTex ? 102 : 48) * fit;
		subtitle.material.opacity = 1 - e;
		hintCtl.material.opacity = Math.max(0, 1 - e * 1.2);
	}
	coinHud.position.set(viewW / 2 - 60, CAM_Y + viewH / 2 - topBand - 40, 5); // inside the screen hole
	installBtn.position.set(viewW / 2 - 60, CAM_Y + viewH / 2 - topBand - 68, 5);
	installBtn.material.map = copied && now - copied < 2000 ? installBtnMaps.copied : installBtnMaps.idle;
	{
		const bottom = CAM_Y - viewH / 2;
		// controls row sits in the upper half of the shell shelf, above the
		// START/SELECT pills and speaker grille the shell texture draws
		const row = bottom + ctrlBand - 88;
		if (dpadMesh) {
			dpadMesh.position.x = -viewW / 2 + 72;
			dpadMesh.position.y = ctrlBand > 0 ? row : bottom + 62;
		}
		for (const p of pads) {
			p.mesh.position.x = viewW / 2 - p.off;
			// A rides higher than B — the Game Boy diagonal
			p.mesh.position.y = ctrlBand > 0
				? row + (p.act === "jump" ? 14 : -10)
				: bottom + p.lift;
			p.mesh.material.map = p.held ? p.down : p.up;
			p.mesh.material.opacity = p.held ? 1 : .92;
		}
	}

	// stations: bob + dialog proximity
	for (const s of stations) {
		if (!reduceMotion) s.spr.position.y = GROUND + 38 + Math.round(Math.sin(now / 400 + s.bob) * 2);
		const near = Math.abs(P.x - s.x) < 70;
		s.card.material.opacity += ((near ? 1 : 0) - s.card.material.opacity) * Math.min(1, dt * (reduceMotion ? 60 : 8));
	}
	// blocks: bump anim, toast + coin
	for (const b of blocks) {
		if (b.anim > 0) {
			b.anim = Math.max(0, b.anim - dt * 2);
			const k = 1 - b.anim;
			b.mesh.position.y = BLOCK_Y + Math.sin(k * Math.PI) * 8;
			b.coin.material.opacity = b.anim;
			b.coin.position.y = BLOCK_Y + 20 + k * 30; b.coin.position.z = 30;
		}
		if (b.hit) b.toast.material.opacity += (1 - b.toast.material.opacity) * Math.min(1, dt * 6);
	}
	// castle hint
	window.__castleHint.material.opacity += ((nearCastle() ? 1 : 0) - window.__castleHint.material.opacity) * Math.min(1, dt * 6);
	// install terminal: reveal command + copy hint when near, toast after copy
	{
		const inst = window.__install, near = nearInstall() ? 1 : 0;
		// keep the long command line inside the visible screen width
		inst.cmd.scale.setScalar(Math.min(.62, (viewW - 36) / inst.cmd.userData.canvas.width));
		inst.cmd.material.opacity += (near - inst.cmd.material.opacity) * Math.min(1, dt * 6);
		inst.copyHint.material.opacity += (near - inst.copyHint.material.opacity) * Math.min(1, dt * 6);
		const showToast = copied && now - copied < 2500 ? 1 : 0;
		inst.copiedToast.material.opacity += (showToast - inst.copiedToast.material.opacity) * Math.min(1, dt * 8);
	}
	// stars twinkle
	if (!reduceMotion) for (const s of stars) s.material.opacity = .4 + .4 * Math.sin(now / 900 + s.userData.tw);
	// antenna blinkers: slow red pulse; neon signs: occasional flicker dip
	if (!reduceMotion) {
		for (const l of blinkers) l.material.opacity = Math.sin(now / 700 + l.userData.ph) > .1 ? 1 : .12;
		for (const n of neons) {
			const flick = Math.sin(now / 90 + n.ph * 7) > .97 ? .35 : 1;
			n.sign.material.opacity = n.sign.userData.base * flick;
			n.halo.material.opacity = .8 * flick;
		}
	}
}

function loop(now) {
	const dt = Math.min(.05, (now - last) / 1000);
	last = now;
	step(dt, now);
	renderer.render(scene, camera);
	requestAnimationFrame(loop);
}
requestAnimationFrame(loop);
