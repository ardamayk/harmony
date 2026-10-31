// Moves the pinned mpv to its newest release. The official Windows build
// clones FFmpeg and libplacebo at build time and lists neither, so every line
// of licenses/mpv-windows/sources.txt is proved from the build or the script
// that made it. Anything unproved stops the update: the GPL and LGPL source
// published beside a release must match the binaries exactly.
//   update        rewrites the pinned files; GITHUB_OUTPUT status= is current,
//                 waiting (released under a week ago), ready or manual
//   check-build   runs the pinned mpv.exe; only a Windows runner can

import { spawnSync } from "node:child_process";
import {
	appendFileSync,
	mkdtempSync,
	readdirSync,
	readFileSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { cacheDirectory, mpvRelease, repoRoot } from "./config.ts";
import { extractZip, sha256File } from "./fetch.ts";
import { stageMpv } from "./mpv.ts";

const cooldownMs = 7 * 24 * 60 * 60 * 1000;
/** Each one's licence is in licenses/mpv-windows; a new one needs a person. */
const knownLibraries = new Set([
	"avcodec",
	"avdevice",
	"avfilter",
	"avformat",
	"avutil",
	"libass",
	"libdav1d",
	"libfreetype",
	"libfribidi",
	"libgcc_s_seh",
	"libharfbuzz",
	"libiconv",
	"liblcms2",
	"libplacebo",
	"libshaderc_shared",
	"libspirv-cross-c-shared",
	"libssp",
	"libstdc++",
	"libwinpthread",
	"swresample",
	"swscale",
	"vulkan",
	"zlib1",
]);

export class Unproved extends Error {}

function match(text: string, pattern: RegExp, what: string): string {
	const found = pattern.exec(text)?.[1];
	if (!found) throw new Unproved(`${what} not found`);
	return found;
}

export const ffmpegCommit = (avcodec: string) =>
	match(
		avcodec,
		/FFmpeg version git-[\d-]+-([0-9a-f]{7,40})\b/,
		"FFmpeg commit",
	);

export const reportedVersion = (mpvExe: string) =>
	match(mpvExe, /mpv v(\d[^\s\0]*)/, "mpv version");

export const scriptVersion = (buildScript: string, component: string) =>
	match(
		buildScript,
		new RegExp(`_${component} \\(\\) \\{\\s+local ver=(\\S+)`),
		`${component} version`,
	);

export function buildDate(innerZip: string): string {
	return match(innerZip, /^mpv-git-(\d{4}-\d{2}-\d{2})-/, "build date");
}

export function unknownLibraries(files: string[]): string[] {
	return files
		.filter((name) => name.toLowerCase().endsWith(".dll"))
		.map((name) => name.replace(/(-[\d-]+)?\.dll$/i, ""))
		.filter((name) => !knownLibraries.has(name));
}

/** The build took libplacebo's tip; a commit that day leaves it unknowable. */
export function commitAtBuild(before: string, endOfDay: string): string {
	if (before !== endOfDay) {
		throw new Unproved("libplacebo changed on the build date");
	}
	return before;
}

async function github<T>(path: string): Promise<T> {
	const response = await fetch(`https://api.github.com/${path}`, {
		headers: { authorization: `Bearer ${process.env.GH_TOKEN}` },
	});
	if (!response.ok) throw new Unproved(`${path}: ${response.status}`);
	return (await response.json()) as T;
}

const placeboTip = async (until: string) =>
	(
		await github<{ sha: string }[]>(
			`repos/haasn/libplacebo/commits?until=${until}&per_page=1`,
		)
	)[0]?.sha ?? "";

function dayBefore(date: string): string {
	const day = new Date(`${date}T00:00:00Z`);
	day.setUTCDate(day.getUTCDate() - 1);
	return day.toISOString().slice(0, 10);
}

async function pinnedSources(tag: string, build: string, innerZip: string) {
	const script = await (
		await fetch(
			`https://raw.githubusercontent.com/mpv-player/mpv/${tag}/ci/build-mingw64.sh`,
		)
	).text();
	const mpv = (
		await github<{ sha: string }>(`repos/mpv-player/mpv/commits/${tag}`)
	).sha;
	const ffmpeg = (
		await github<{ sha: string }>(
			`repos/FFmpeg/FFmpeg/commits/${ffmpegCommit(
				readFileSync(
					join(
						build,
						readdirSync(build).find((name) => name.startsWith("avcodec")) ?? "",
					),
					"latin1",
				),
			)}`,
		)
	).sha;
	const date = buildDate(innerZip);
	const placebo = commitAtBuild(
		await placeboTip(`${dayBefore(date)}T23:59:59Z`),
		await placeboTip(`${date}T23:59:59Z`),
	);
	const fribidi = scriptVersion(script, "fribidi");
	const iconv = scriptVersion(script, "iconv");
	const archive = (repository: string, commit: string, name: string) =>
		`https://github.com/${repository}/archive/${commit}.tar.gz ${name}-${commit.slice(0, 8)}.tar.gz`;
	const lines = [
		archive("mpv-player/mpv", mpv, "mpv"),
		archive("FFmpeg/FFmpeg", ffmpeg, "FFmpeg"),
		archive("haasn/libplacebo", placebo, "libplacebo"),
		`https://github.com/fribidi/fribidi/releases/download/v${fribidi}/fribidi-${fribidi}.tar.xz fribidi-${fribidi}.tar.xz`,
		`https://ftp.gnu.org/pub/gnu/libiconv/libiconv-${iconv}.tar.gz libiconv-${iconv}.tar.gz`,
	];
	for (const line of lines) {
		const url = line.split(" ")[0] as string;
		const response = await fetch(url, { method: "HEAD" });
		if (!response.ok) throw new Unproved(`${url}: ${response.status}`);
	}
	return `${lines.join("\n")}\n`;
}

function replaceIn(path: string, pattern: RegExp, value: string) {
	const file = join(repoRoot, path);
	const text = readFileSync(file, "utf8");
	if (!pattern.test(text)) throw new Unproved(`${path}: ${pattern} not found`);
	writeFileSync(file, text.replace(pattern, value));
}

async function update(): Promise<{ status: string; version?: string }> {
	const latest = await github<{ tag_name: string; published_at: string }>(
		"repos/mpv-player/mpv/releases/latest",
	);
	const tag = latest.tag_name;
	const version = tag.replace(/^v/, "");
	if (mpvRelease.url.includes(`/${tag}/`)) return { status: "current" };
	if (Date.now() - Date.parse(latest.published_at) < cooldownMs) {
		return { status: "waiting", version };
	}
	const url = `https://github.com/mpv-player/mpv/releases/download/${tag}/mpv-${tag}-x86_64-w64-mingw32.zip`;
	const work = mkdtempSync(join(tmpdir(), "earthly-audio-mpv-update-"));
	try {
		const zipPath = join(work, "mpv.zip");
		const response = await fetch(url);
		if (!response.ok) throw new Unproved(`${url}: ${response.status}`);
		writeFileSync(zipPath, Buffer.from(await response.arrayBuffer()));
		extractZip(zipPath, join(work, "outer"));
		const innerZip = readdirSync(join(work, "outer")).find((name) =>
			name.startsWith("mpv-git-"),
		);
		if (!innerZip) throw new Unproved("no mpv-git-*.zip in the release");
		const build = join(work, "build");
		extractZip(join(work, "outer", innerZip), build);
		const unknown = unknownLibraries(readdirSync(build));
		if (unknown.length > 0) {
			throw new Unproved(`no licence for ${unknown.join(", ")}`);
		}
		const sources = await pinnedSources(tag, build, innerZip);
		const tarball = await fetch(
			`https://github.com/mpv-player/mpv/archive/refs/tags/${tag}.tar.gz`,
		);
		if (!tarball.ok) throw new Unproved(`source tarball: ${tarball.status}`);
		const tarballPath = join(work, "mpv.tar.gz");
		writeFileSync(tarballPath, Buffer.from(await tarball.arrayBuffer()));

		writeFileSync(join(repoRoot, "licenses/mpv-windows/sources.txt"), sources);
		writeFileSync(
			join(repoRoot, "desktop/core/mpv-windows-version.txt"),
			`${reportedVersion(readFileSync(join(build, "mpv.exe"), "latin1"))}\n`,
		);
		writeFileSync(
			join(repoRoot, "desktop/core/mpv-version.txt"),
			`${version}\n`,
		);
		replaceIn(
			"scripts/desktop-windows/config.ts",
			/releases\/download\/v[\d.]+\/mpv-v[\d.]+-x86_64-w64-mingw32\.zip/,
			`releases/download/${tag}/mpv-${tag}-x86_64-w64-mingw32.zip`,
		);
		replaceIn(
			"scripts/desktop-windows/config.ts",
			new RegExp(mpvRelease.sha256, "g"),
			sha256File(zipPath),
		);
		replaceIn(
			"scripts/build-pinned-mpv.sh",
			/(?<=MPV_SOURCE_SHA256:-)[0-9a-f]{64}/,
			sha256File(tarballPath),
		);
		return { status: "ready", version };
	} catch (error) {
		if (!(error instanceof Unproved)) throw error;
		console.error(error.message);
		return { status: "manual", version };
	} finally {
		rmSync(work, { recursive: true, force: true });
	}
}

async function checkBuild() {
	const work = mkdtempSync(join(tmpdir(), "earthly-audio-mpv-check-"));
	await stageMpv(work, mpvRelease, cacheDirectory);
	// One second of 8 kHz silence: a short header plus zeroed samples.
	const samples = 8000;
	const wav = Buffer.alloc(44 + samples * 2);
	wav.write("RIFF", 0);
	wav.writeUInt32LE(36 + samples * 2, 4);
	wav.write("WAVEfmt ", 8);
	wav.writeUInt32LE(16, 16);
	wav.writeUInt16LE(1, 20);
	wav.writeUInt16LE(1, 22);
	wav.writeUInt32LE(8000, 24);
	wav.writeUInt32LE(16000, 28);
	wav.writeUInt16LE(2, 32);
	wav.writeUInt16LE(16, 34);
	wav.write("data", 36);
	wav.writeUInt32LE(samples * 2, 40);
	const fixture = join(work, "silence.wav");
	writeFileSync(fixture, wav);
	const played = spawnSync(
		join(work, "mpv.exe"),
		["--no-config", "--no-video", "--ao=null", "--idle=no", fixture],
		{ encoding: "utf8", timeout: 30_000 },
	);
	if (played.status !== 0) {
		throw new Error(`mpv.exe exited ${played.status}: ${played.stderr}`);
	}
}

if (import.meta.main) {
	const command = process.argv[2];
	if (command === "check-build") {
		await checkBuild();
	} else if (command === "update") {
		const { status, version = "" } = await update();
		console.log(`mpv update: ${status} ${version}`);
		if (process.env.GITHUB_OUTPUT) {
			appendFileSync(
				process.env.GITHUB_OUTPUT,
				`status=${status}\nversion=${version}\n`,
			);
		}
	} else {
		console.error(
			"Usage: node scripts/desktop-windows/mpv-update.ts update|check-build",
		);
		process.exit(2);
	}
}
