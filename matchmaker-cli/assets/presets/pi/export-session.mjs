#!/usr/bin/env node
/**
 * Convert a pi session .jsonl file to HTML using pi's internal exporter.
 * Produces the same output as pi's /export slash command.
 *
 *   node bin/export-session.mjs <session.jsonl> [-o output.html]
 *
 * pi-coding-agent is auto-detected from the running node's install prefix
 * (works with mise, nvm, fnm, volta, Homebrew, system installs). Override
 * with the PI_INSTALL_DIR env var.
 */
import { existsSync, statSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const HELP = `Usage: node bin/export-session.mjs <session.jsonl> [-o output.html]

Convert a pi session .jsonl file to HTML. Same format as pi's /export.

Arguments:
  <session.jsonl>    Session file (e.g. ~/.pi/agent/sessions/.../*.jsonl)

Options:
  -o <path>          Output HTML path (default: <input>.html)
  -h, --help         Show this help

Environment:
  PI_INSTALL_DIR     Override pi-coding-agent install location.
`;

function parseArgs(argv) {
	const out = { help: false, input: undefined, outPath: undefined };
	for (let i = 2; i < argv.length; i++) {
		const a = argv[i];
		if (a === "-h" || a === "--help") out.help = true;
		else if (a === "-o") out.outPath = argv[++i];
		else if (a.startsWith("-")) throw new Error(`unknown option: ${a}`);
		else if (out.input) throw new Error("only one input file is supported");
		else out.input = a;
	}
	return out;
}

/**
 * Locate the pi-coding-agent install. Strategy:
 *   1. PI_INSTALL_DIR env var (explicit override).
 *   2. Derive the versioned node prefix from process.execPath and look
 *      for the package in <prefix>/lib/node_modules/@earendil-works/.
 *      This works for mise, nvm, fnm, volta, Homebrew, system installs
 *      — anywhere `node` resolves to a versioned directory.
 */
function findPiInstall() {
	if (process.env.PI_INSTALL_DIR) {
		const p = process.env.PI_INSTALL_DIR;
		if (existsSync(resolve(p, "dist/core/export-html/index.js"))) return p;
		return {
			error: `PI_INSTALL_DIR=${p} does not contain a pi-coding-agent install`,
		};
	}
	const prefix = dirname(dirname(process.execPath));
	const candidate = resolve(
		prefix,
		"lib",
		"node_modules",
		"@earendil-works",
		"pi-coding-agent",
	);
	if (existsSync(resolve(candidate, "dist/core/export-html/index.js"))) {
		return candidate;
	}
	return undefined;
}

async function main() {
	let args;
	try {
		args = parseArgs(process.argv);
	} catch (err) {
		console.error(`export-session: ${err.message}\n`);
		console.error(HELP);
		process.exit(1);
	}
	if (args.help) {
		console.log(HELP);
		process.exit(0);
	}
	if (!args.input) {
		console.error(`export-session: missing input file\n`);
		console.error(HELP);
		process.exit(1);
	}

	const inputPath = resolve(args.input);
	if (!existsSync(inputPath)) {
		console.error(`export-session: file not found: ${inputPath}`);
		process.exit(3);
	}

	const piResult = findPiInstall();
	if (!piResult) {
		const tried = resolve(
			dirname(dirname(process.execPath)),
			"lib",
			"node_modules",
			"@earendil-works",
		);
		console.error(
			`export-session: could not find pi-coding-agent.\n` +
				`Looked in: ${tried}\n` +
				`Set PI_INSTALL_DIR to its location, or install pi-coding-agent globally.`,
		);
		process.exit(2);
	}
	if (typeof piResult === "object" && "error" in piResult) {
		console.error(`export-session: ${piResult.error}`);
		process.exit(2);
	}
	const piDir = piResult;

	const outputPath = args.outPath
		? resolve(args.outPath)
		: inputPath.replace(/\.jsonl$/, ".html");

	const exporterUrl = pathToFileURL(
		resolve(piDir, "dist/core/export-html/index.js"),
	).href;
	let exportFromFile;
	try {
		({ exportFromFile } = await import(exporterUrl));
	} catch (err) {
		console.error(
			`export-session: failed to load ${exporterUrl}: ${err.message}`,
		);
		process.exit(2);
	}

	let result;
	try {
		result = await exportFromFile(inputPath, { outputPath });
	} catch (err) {
		console.error(`export-session: export failed: ${err.message}`);
		process.exit(4);
	}

	const inSize = statSync(inputPath).size;
	const outSize = statSync(result).size;
	console.log(`wrote ${result} (${inSize} → ${outSize} bytes)`);
}

main();
