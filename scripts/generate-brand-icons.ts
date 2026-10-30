// Draws the Harmony mark and writes every icon file from it: the Web Client's
// favicon and logos, and the Desktop Client's window, tray and executable icons.
// The mark is the 3:2 Lissajous curve, the shape of a perfect fifth. Small sizes
// get a heavier line so the curve stays open. Run with `mise run brand:icons`.

import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { deflateSync } from "node:zlib";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const webPublic = join(repoRoot, "web/public");
const desktopResources = join(repoRoot, "desktop/resources");

type Color = [number, number, number];
type Point = [number, number];
type Style = { tile: boolean; scale: number; stroke: number };

const TILE: Color = [0x0b, 0x0b, 0x12];
const GRADIENT: Color[] = [
	[0x6e, 0x6e, 0x6e],
	[0xb6, 0xb6, 0xb6],
	[0xff, 0xff, 0xff],
];
const HEX_GRADIENT = ["#6E6E6E", "#B6B6B6", "#FFFFFF"];

// Units of the mark's 64-unit box.
const AMPLITUDE = 25;
const CENTER = 32;
const TILE_RADIUS = 14;
const CURVE_POINTS = 180;

const LARGE: Style = { tile: true, scale: 0.8, stroke: 5.5 };
const SMALL: Style = { tile: true, scale: 0.94, stroke: 7 };
const MARK: Style = { tile: false, scale: 1.1, stroke: 7.5 };

function curve(): Point[] {
	const points: Point[] = [];
	for (let step = 0; step < CURVE_POINTS; step += 1) {
		const t = (step / CURVE_POINTS) * 2 * Math.PI;
		points.push([
			CENTER + AMPLITUDE * Math.sin(3 * t + Math.PI / 2),
			CENTER + AMPLITUDE * Math.sin(2 * t),
		]);
	}
	return points;
}

function distanceToSegment(point: Point, from: Point, to: Point): number {
	const [dx, dy] = [to[0] - from[0], to[1] - from[1]];
	const along =
		((point[0] - from[0]) * dx + (point[1] - from[1]) * dy) /
		(dx * dx + dy * dy);
	const t = Math.min(1, Math.max(0, along));
	return Math.hypot(point[0] - from[0] - t * dx, point[1] - from[1] - t * dy);
}

function distanceToTile([x, y]: Point): number {
	const [qx, qy] = [
		Math.abs(x - CENTER) - CENTER + TILE_RADIUS,
		Math.abs(y - CENTER) - CENTER + TILE_RADIUS,
	];
	return (
		Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) +
		Math.min(Math.max(qx, qy), 0) -
		TILE_RADIUS
	);
}

function gradientAt([x, y]: Point): Color {
	const t = Math.min(1, Math.max(0, (x + y - 12) / 104)) * 2;
	const from = GRADIENT[Math.min(1, Math.floor(t))];
	const to = GRADIENT[Math.min(2, Math.floor(t) + 1)];
	const mix = t - Math.min(1, Math.floor(t));
	return from.map((value, index) => value + (to[index] - value) * mix) as Color;
}

/** Straight RGBA pixels; edges get their coverage from the signed distance. */
function render(size: number, style: Style): Uint8Array {
	const pixels = new Uint8Array(size * size * 4);
	const unitsPerPixel = 64 / size;
	const coverage = (distance: number) =>
		Math.min(1, Math.max(0, 0.5 - distance / unitsPerPixel));
	const points = curve();
	for (let row = 0; row < size; row += 1) {
		for (let column = 0; column < size; column += 1) {
			const point: Point = [
				(column + 0.5) * unitsPerPixel,
				(row + 0.5) * unitsPerPixel,
			];
			const onCurve: Point = [
				CENTER + (point[0] - CENTER) / style.scale,
				CENTER + (point[1] - CENTER) / style.scale,
			];
			let nearest = Number.POSITIVE_INFINITY;
			for (let index = 0; index < points.length; index += 1) {
				const next = points[(index + 1) % points.length];
				nearest = Math.min(
					nearest,
					distanceToSegment(onCurve, points[index], next),
				);
			}
			const line = coverage((nearest - style.stroke / 2) * style.scale);
			const tile = style.tile ? coverage(distanceToTile(point)) : 0;
			const color = gradientAt(onCurve);
			const alpha = style.tile ? tile : line;
			const offset = (row * size + column) * 4;
			for (let channel = 0; channel < 3; channel += 1) {
				pixels[offset + channel] = Math.round(
					style.tile
						? TILE[channel] + (color[channel] - TILE[channel]) * line
						: color[channel],
				);
			}
			pixels[offset + 3] = Math.round(alpha * 255);
		}
	}
	return pixels;
}

function crc32(bytes: Uint8Array): number {
	let crc = 0xffffffff;
	for (const byte of bytes) {
		crc ^= byte;
		for (let bit = 0; bit < 8; bit += 1) {
			crc = crc & 1 ? (crc >>> 1) ^ 0xedb88320 : crc >>> 1;
		}
	}
	return (crc ^ 0xffffffff) >>> 0;
}

function pngChunk(type: string, data: Uint8Array): Buffer {
	const body = Buffer.concat([Buffer.from(type, "latin1"), data]);
	const chunk = Buffer.alloc(body.length + 8);
	chunk.writeUInt32BE(data.length, 0);
	body.copy(chunk, 4);
	chunk.writeUInt32BE(crc32(body), body.length + 4);
	return chunk;
}

function png(size: number, style: Style): Buffer {
	const pixels = render(size, style);
	const header = Buffer.alloc(13);
	header.writeUInt32BE(size, 0);
	header.writeUInt32BE(size, 4);
	header.set([8, 6, 0, 0, 0], 8); // 8-bit RGBA
	const scanlines = Buffer.alloc((size * 4 + 1) * size);
	for (let row = 0; row < size; row += 1) {
		const start = row * (size * 4 + 1);
		scanlines.set(
			pixels.subarray(row * size * 4, (row + 1) * size * 4),
			start + 1,
		);
	}
	return Buffer.concat([
		Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
		pngChunk("IHDR", header),
		pngChunk("IDAT", deflateSync(scanlines, { level: 9 })),
		pngChunk("IEND", Buffer.alloc(0)),
	]);
}

/** An .ico holding PNG images. */
function ico(sizes: number[], style: (size: number) => Style): Buffer {
	const images = sizes.map((size) => png(size, style(size)));
	const directory = Buffer.alloc(6 + 16 * sizes.length);
	directory.writeUInt16LE(1, 2);
	directory.writeUInt16LE(sizes.length, 4);
	let offset = directory.length;
	sizes.forEach((size, index) => {
		const entry = 6 + 16 * index;
		directory[entry] = size === 256 ? 0 : size;
		directory[entry + 1] = size === 256 ? 0 : size;
		directory.writeUInt16LE(1, entry + 4);
		directory.writeUInt16LE(32, entry + 6);
		directory.writeUInt32LE(images[index].length, entry + 8);
		directory.writeUInt32LE(offset, entry + 12);
		offset += images[index].length;
	});
	return Buffer.concat([directory, ...images]);
}

function svg(style: Style): string {
	const path = curve()
		.filter((_, index) => index % 2 === 0)
		.map(
			([x, y], index) => `${index ? "L" : "M"}${x.toFixed(2)} ${y.toFixed(2)}`,
		)
		.join("");
	const stops = HEX_GRADIENT.map(
		(color, index) => `<stop offset="${index / 2}" stop-color="${color}"/>`,
	).join("");
	const tile = style.tile
		? `<rect width="64" height="64" rx="${TILE_RADIUS}" fill="#0B0B12"/>`
		: "";
	const scale = `translate(32 32) scale(${style.scale}) translate(-32 -32)`;
	return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><defs><linearGradient id="fifth" gradientUnits="userSpaceOnUse" x1="6" y1="6" x2="58" y2="58">${stops}</linearGradient></defs>${tile}<path transform="${scale}" d="${path}Z" fill="none" stroke="url(#fifth)" stroke-width="${style.stroke}" stroke-linejoin="round"/></svg>\n`;
}

const tileStyle = (size: number) => (size <= 32 ? SMALL : LARGE);

writeFileSync(join(webPublic, "logo.svg"), svg({ ...MARK, stroke: 6.5 }));
writeFileSync(join(webPublic, "favicon.svg"), svg(LARGE));
writeFileSync(
	join(webPublic, "favicon.ico"),
	ico([16, 24, 32, 48, 64], tileStyle),
);
writeFileSync(join(webPublic, "logo192.png"), png(192, LARGE));
writeFileSync(join(webPublic, "logo512.png"), png(512, LARGE));
writeFileSync(join(desktopResources, "icon.png"), png(512, LARGE));
writeFileSync(
	join(desktopResources, "icon.ico"),
	ico([16, 24, 32, 48, 64, 128, 256], tileStyle),
);
writeFileSync(join(desktopResources, "tray.png"), png(16, SMALL));
writeFileSync(join(desktopResources, "tray@2x.png"), png(32, SMALL));
