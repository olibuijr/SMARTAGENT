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
await renderer.init();
await document.fonts.load(`8px ${FONT}`).catch(() => {});
renderer.setClearColor(new THREE.Color(BG));

const scene = new THREE.Scene();
// Perspective diorama: the gameplay plane sits at z=0 and spans VIEW_H
// vertically, exactly like the old ortho view — but depth is now real.
const FOV = 38;
const DIST = (VIEW_H / 2) / Math.tan((FOV / 2) * Math.PI / 180); // ≈392
const camera = new THREE.PerspectiveCamera(FOV, 16 / 9, 10, 2600);
scene.add(camera);
let viewW = 480;
function resize() {
	const aspect = innerWidth / innerHeight;
	camera.aspect = aspect;
	camera.updateProjectionMatrix();
	viewW = Math.round(Math.min(760, Math.max(340, VIEW_H * aspect)));
	renderer.setSize(viewW, VIEW_H, false);
}
addEventListener("resize", resize);
resize();

// world y: screen bottom at y=0 → camera rides above the ground line
const CAM_Y = VIEW_H / 2 - 24;
camera.position.set(0, CAM_Y, DIST);
scene.fog = new THREE.Fog(new THREE.Color(BG), DIST + 60, DIST + 900);

// ── Build world ─────────────────────────────────────────────────────────────
const world = new THREE.Group();
scene.add(world);

// sky bands (deep background — fog-exempt)
{
	const bands = [["#12121c", 1100, 620], ["#161624", 420, 210], ["#1a1a2c", 230, -10]];
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
		s.position.set(rnd() * LEVEL_END * 1.4 - 200, 120 + rnd() * 420, -820 + rnd() * 120);
		s.material.opacity = .3 + rnd() * .7;
		s.userData.tw = rnd() * 6.28;
		scene.add(s); stars.push(s);
	}
}
// city: actual 3D boxes at depth — perspective gives the parallax for free
{
	let seed = 13;
	const rnd = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
	const mat = (canvas) => new THREE.MeshBasicMaterial({ map: tex(canvas) });
	const flat = (col) => new THREE.MeshBasicMaterial({ color: col });
	function buildingFace(w, h) {
		const [c, x] = cv(Math.ceil(w / 4) * 4, Math.ceil(h / 4) * 4);
		x.fillStyle = "#20202c"; x.fillRect(0, 0, c.width, c.height);
		x.fillStyle = rnd() > .5 ? "#43536b" : "#3f4c40";
		for (let wy = 8; wy < h - 10; wy += 12)
			for (let wx = 5; wx < w - 8; wx += 10)
				if (rnd() > .62) x.fillRect(wx, wy, 4, 5);
		return c;
	}
	for (let i = 0; i < 46; i++) {
		const bw = 44 + rnd() * 70, bh = 70 + rnd() * 190, bd = 36 + rnd() * 40;
		const z = -(90 + rnd() * 320);
		const front = mat(buildingFace(bw, bh));
		const side = flat("#181822"), top = flat("#14141c");
		const b = new THREE.Mesh(
			new THREE.BoxGeometry(bw, bh, bd),
			[side, side, top, side, front, side]
		);
		b.position.set(-350 + rnd() * (LEVEL_END + 900), GROUND + bh / 2 - 6, z - bd / 2);
		scene.add(b);
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
		new THREE.BoxGeometry(LEVEL_END + 1920, GROUND, 240),
		[side, side, top, side, front, side]
	);
	slab.position.set(LEVEL_END / 2, GROUND / 2, -120);
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
	const np = plane(textCanvas(a.name, 8, a.accent, { bg: PANEL, pad: 5 }));
	np.position.set(ax, GROUND + 90, 1);
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
	const card = plane(dc);
	card.position.set(ax, GROUND + 138, 2);
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
	const toast = plane(textCanvas(fact, 8, "#ffd75f", { bg: PANEL, pad: 5 }));
	toast.position.set(bx, BLOCK_Y + 34, 2);
	toast.material.opacity = 0;
	world.add(toast);
	const cn = plane(coin());
	cn.position.set(bx, BLOCK_Y + 20, 14);
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
	label.position.set(INSTALL_X, GROUND + 58, 1);
	world.add(label);
	const cmd = plane(textCanvas(INSTALL_CMD, 8, "#e8e2d8", { bg: PANEL, pad: 6 }));
	cmd.scale.setScalar(.62); // long line — shrink to fit the view
	cmd.position.set(INSTALL_X, GROUND + 110, 2);
	cmd.material.opacity = 0;
	world.add(cmd);
	const copyHint = plane(textCanvas(matchMedia("(pointer: coarse)").matches ? "PRESS B TO COPY" : "PRESS C OR CLICK TO COPY", 8, "#7dff9a", { bg: PANEL, pad: 5 }));
	copyHint.position.set(INSTALL_X, GROUND + 84, 2);
	copyHint.material.opacity = 0;
	world.add(copyHint);
	const copiedToast = plane(textCanvas("COPIED! PASTE IT IN YOUR TERMINAL", 8, "#161620", { bg: "#7dff9a", pad: 5 }));
	copiedToast.position.set(INSTALL_X, GROUND + 136, 3);
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
	hint.position.set(LEVEL_END - 120, GROUND + 150, 2);
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
hud.position.set(0, -CAM_Y, -DIST); // children keep their world-y semantics
const title = plane(textCanvas("SMARTAGENT", 26, SKINC, { glow: "#ffaf5f", pad: 8 }));
const subtitle = plane(textCanvas("THE LEGENDARY DEVELOPER TEAM", 9, "#87d7ff", { pad: 4 }));
const hintCtl = plane(textCanvas(
	matchMedia("(pointer: coarse)").matches
		? "WALK WITH THE PAD - A JUMPS - B USES"
		: "ARROWS / AD TO WALK - SPACE TO JUMP",
	8, "#8a8a9a", { pad: 4 }));
title.position.set(0, 70, 5);
subtitle.position.set(0, 44, 5);
hintCtl.position.set(0, 22, 5);
hud.add(title, subtitle, hintCtl);
const isTouch = matchMedia("(pointer: coarse)").matches;
function padButton(label, w = 36) {
	const [c, x] = cv(w + (64 - w % 64) % 64, 36); // row-aligned width
	const ox = Math.floor((c.width - w) / 2);
	x.fillStyle = "#262626"; x.fillRect(ox, 0, w, 36);
	x.strokeStyle = "#8a8a9a"; x.lineWidth = 2; x.strokeRect(ox + 1, 1, w - 2, 34);
	x.fillStyle = "#e8e2d8"; x.font = `12px ${FONT}`; x.textBaseline = "top";
	x.fillText(label, ox + Math.floor((w - x.measureText(label).width) / 2), 11);
	return { canvas: c, w };
}
const pads = [];
if (isTouch) {
	// [id, label, dx-from-edge(sign = side), action]
	const defs = [
		{ label: "<", side: -1, off: 26, act: "left" },
		{ label: ">", side: -1, off: 70, act: "right" },
		{ label: "B", side: 1, off: 70, act: "use" },
		{ label: "A", side: 1, off: 26, act: "jump" },
	];
	for (const d of defs) {
		const { canvas } = padButton(d.label);
		const m = plane(canvas);
		m.material.opacity = .72;
		m.position.set(0, 0, 6);
		hud.add(m);
		pads.push({ ...d, mesh: m, held: false });
	}
}
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
	if (e.code === "KeyC" && nearInstall()) copyInstall();
	if (["Space", "ArrowUp", "KeyW"].includes(e.code)) e.preventDefault();
});
addEventListener("keyup", (e) => (keys[e.code] = false));
// touch: on-screen controller (multi-touch — walk while jumping)
let touchDir = 0, touchJump = false;
const activePointers = new Map(); // pointerId -> pad
function padAt(clientX, clientY) {
	const vx = (clientX / innerWidth - .5) * viewW;
	const vy = (.5 - clientY / innerHeight) * VIEW_H;
	return pads.find((p) =>
		Math.abs(vx - p.mesh.position.x) < 24 && Math.abs(vy - p.mesh.position.y) < 26);
}
function recomputeTouch() {
	touchDir = 0; touchJump = false;
	for (const p of activePointers.values()) {
		if (!p) continue;
		if (p.act === "left") touchDir = -1;
		if (p.act === "right") touchDir = 1;
		if (p.act === "jump") touchJump = true;
	}
	for (const p of pads) p.held = [...activePointers.values()].includes(p);
}
canvas.addEventListener("pointerdown", (e) => {
	start();
	const p = padAt(e.clientX, e.clientY);
	if (p) {
		if (p.act === "use") {
			if (nearCastle()) { location.href = REPO; return; }
			if (nearInstall()) copyInstall();
		}
		activePointers.set(e.pointerId, p);
		recomputeTouch();
		return;
	}
	if (!isTouch) { // desktop click fallbacks
		if (nearCastle()) { location.href = REPO; return; }
		if (nearInstall()) { copyInstall(); return; }
	}
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
	// title parks top-left after the first input
	if (started) {
		const k = reduceMotion ? 1 : Math.min(1, (now - lastStart) / 800);
		const e = 1 - Math.pow(1 - k, 4); // ease-out-quart
		title.scale.setScalar(1 - .6 * e);
		title.position.x = (-viewW / 2 + 86) * e;
		title.position.y = 70 + 42 * e;
		subtitle.material.opacity = 1 - e;
		hintCtl.material.opacity = Math.max(0, 1 - e * 1.2);
	}
	coinHud.position.set(viewW / 2 - 54, VIEW_H / 2 - 34, 5);
	for (const p of pads) {
		p.mesh.position.x = p.side * (viewW / 2 - p.off);
		p.mesh.position.y = -VIEW_H / 2 + 30;
		p.mesh.material.opacity = p.held ? 1 : .72;
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
			b.coin.position.y = BLOCK_Y + 20 + k * 30; b.coin.position.z = 14;
		}
		if (b.hit) b.toast.material.opacity += (1 - b.toast.material.opacity) * Math.min(1, dt * 6);
	}
	// castle hint
	window.__castleHint.material.opacity += ((nearCastle() ? 1 : 0) - window.__castleHint.material.opacity) * Math.min(1, dt * 6);
	// install terminal: reveal command + copy hint when near, toast after copy
	{
		const inst = window.__install, near = nearInstall() ? 1 : 0;
		inst.cmd.material.opacity += (near - inst.cmd.material.opacity) * Math.min(1, dt * 6);
		inst.copyHint.material.opacity += (near - inst.copyHint.material.opacity) * Math.min(1, dt * 6);
		const showToast = copied && now - copied < 2500 ? 1 : 0;
		inst.copiedToast.material.opacity += (showToast - inst.copiedToast.material.opacity) * Math.min(1, dt * 8);
	}
	// stars twinkle
	if (!reduceMotion) for (const s of stars) s.material.opacity = .4 + .4 * Math.sin(now / 900 + s.userData.tw);
}

function loop(now) {
	const dt = Math.min(.05, (now - last) / 1000);
	last = now;
	step(dt, now);
	renderer.render(scene, camera);
	requestAnimationFrame(loop);
}
requestAnimationFrame(loop);
