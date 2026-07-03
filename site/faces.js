// Fleet avatar pixel data — direct port of .scratch/gen-faces.ts (the same
// generator that produced the TUI sidebar art), so the web sprites ARE the
// TUI faces. Grids are 24x21, null = transparent.

const SKIN = [252, 205, 165], EYEW = [255, 255, 255], PUP = [35, 35, 50];
const MOUTH = [195, 95, 85], BLUSH = [250, 170, 150];
const W = 24, H = 21;

function grid() { return Array.from({ length: H }, () => Array(W).fill(null)); }

function head(g) {
	for (let y = 3; y <= 19; y++) for (let x = 3; x <= 20; x++) {
		const dy = (y - 11) / 8.5, dx = (x - 11.5) / 9;
		if (dx * dx + dy * dy <= 1) g[y][x] = SKIN;
	}
}
function eyes(g, style = "round") {
	for (const ex of [7, 14]) {
		for (const yy of [10, 11]) for (const xx of [ex, ex + 1, ex + 2]) g[yy][xx] = EYEW;
		const px = ex + (style === "side" ? 0 : 1);
		g[10][px] = PUP; g[11][px] = PUP; g[10][px + 1] = PUP; g[11][px + 1] = PUP;
		if (style === "happy") for (const xx of [ex, ex + 1, ex + 2]) { g[10][xx] = PUP; g[11][xx] = PUP; }
	}
}
function glasses(g, col = [20, 20, 20]) {
	for (const ex of [6, 13]) {
		for (let x = ex; x <= ex + 4; x++) { g[9][x] = col; g[12][x] = col; }
		for (const yy of [10, 11]) { g[yy][ex] = col; g[yy][ex + 4] = col; }
	}
	g[10][11] = col; g[10][12] = col;
}
function smile(g, big = false) {
	for (let x = 9; x <= 14; x++) g[15][x] = MOUTH;
	if (big) { g[14][8] = MOUTH; g[14][15] = MOUTH; for (let x = 10; x <= 13; x++) g[16][x] = MOUTH; }
	g[14][4] = BLUSH; g[14][5] = BLUSH; g[14][18] = BLUSH; g[14][19] = BLUSH;
}
function hair(g, c, style) {
	if (style !== "bald") for (let x = 4; x <= 19; x++) { g[2][x] = c; g[3][x] = c; g[4][x] = c; }
	if (style === "bangs") for (let x = 4; x <= 19; x++) g[5][x] = c;
	if (style === "side") { for (let x = 4; x <= 14; x++) g[5][x] = c; g[5][3] = c; g[6][3] = c; g[7][3] = c; }
	if (style === "long") { for (let y = 5; y <= 12; y++) { g[y][3] = c; g[y][20] = c; } for (let x = 4; x <= 19; x++) g[5][x] = c; }
	if (style === "bun") { g[0][11] = c; g[0][12] = c; g[1][10] = c; g[1][13] = c; for (let x = 4; x <= 19; x++) g[5][x] = c; }
	if (style === "spiky") { g[0][6] = c; g[0][11] = c; g[0][16] = c; for (let x = 4; x <= 19; x++) g[5][x] = c; }
}
function beard(g, c) {
	for (let x = 6; x <= 17; x++) { g[17][x] = c; g[18][x] = c; }
	for (const x of [6, 7, 8, 15, 16, 17]) g[16][x] = c;
}
function mustache(g, c) { for (let x = 8; x <= 15; x++) { g[13][x] = c; g[14][x] = c; } }
function flower(g) {
	const P = [255, 120, 180], Y = [255, 220, 90];
	for (const [y, x] of [[1, 18], [0, 17], [0, 19], [2, 17], [2, 19]]) g[y][x] = P;
	g[1][17] = Y;
}
function bowtie(g) {
	const B = [120, 170, 255];
	for (const x of [9, 10, 13, 14]) { g[20][x] = B; g[19][x] = B; }
	g[19][11] = B; g[19][12] = B; g[20][11] = B; g[20][12] = B;
}

const chars = {
	linus: (g) => { head(g); hair(g, [205, 165, 110], "side"); eyes(g); smile(g); },
	ada: (g) => { head(g); hair(g, [70, 45, 30], "long"); eyes(g, "happy"); smile(g, true); flower(g); },
	grace: (g) => { head(g); hair(g, [40, 40, 45], "bun"); eyes(g); glasses(g); smile(g); },
	ken: (g) => { head(g); hair(g, [150, 150, 155], "bangs"); eyes(g); smile(g); },
	dennis: (g) => { head(g); hair(g, [60, 50, 45], "bangs"); eyes(g); mustache(g, [60, 50, 45]); smile(g); },
	margaret: (g) => { head(g); hair(g, [120, 80, 50], "bun"); eyes(g); glasses(g, [90, 60, 40]); smile(g, true); },
	turing: (g) => { head(g); hair(g, [50, 45, 40], "side"); eyes(g, "side"); smile(g); bowtie(g); },
	woz: (g) => { head(g); hair(g, [110, 75, 50], "bangs"); eyes(g, "happy"); glasses(g); beard(g, [110, 75, 50]); smile(g); },
};

export const FACE_W = W, FACE_H = H;

/** 24x21 RGB grid (null = transparent) for one avatar key. */
export function faceGrid(key) {
	const g = grid();
	(chars[key] ?? chars.linus)(g);
	return g;
}
