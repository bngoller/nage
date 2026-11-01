#!/usr/bin/env node
// The licence allowlist for the Node dependencies of e2e/ (decision 72), the counterpart of
// deny.toml for the Rust ones. No dependencies: it reads package-lock.json, where npm records
// every package's licence and whether it declares an install script, so it runs before `npm ci`.
//
// Usage: node scripts/check-licenses.mjs [path/to/package-lock.json]
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// One line per licence, naming the dependencies that need it; a new licence is the maintainer's
// decision, like a new line in deny.toml.
const ALLOWED = new Set([
    'Apache-2.0', // @playwright/test, playwright, playwright-core, typescript and its platform packages
    'MIT', // @types/node, undici-types
]);

const here = dirname(fileURLToPath(import.meta.url));
const lockPath = process.argv[2] ?? resolve(here, '..', 'package-lock.json');
const lock = JSON.parse(readFileSync(lockPath, 'utf8'));
if (lock.lockfileVersion !== 3) {
    console.error(`${lockPath}: expected lockfileVersion 3, found ${lock.lockfileVersion}`);
    process.exit(2);
}
// The project itself plus at least one dependency; an empty lock file must not pass.
if (typeof lock.packages !== 'object' || lock.packages === null || Object.keys(lock.packages).length < 2) {
    console.error(`${lockPath}: no packages recorded`);
    process.exit(2);
}

const counts = new Map();
const problems = [];
for (const [path, entry] of Object.entries(lock.packages)) {
    if (path === '' || entry.link === true) {
        continue; // the project itself, or a workspace link
    }
    const name = path.slice(path.lastIndexOf('node_modules/') + 'node_modules/'.length);
    const licence = typeof entry.license === 'string' && entry.license !== '' ? entry.license : undefined;
    const key = licence ?? '(none)';
    counts.set(key, (counts.get(key) ?? 0) + 1);
    if (entry.license !== undefined && licence === undefined) {
        problems.push(`${name}@${entry.version}: licence is not an SPDX string: ${JSON.stringify(entry.license)}`);
    } else if (licence === undefined) {
        problems.push(`${name}@${entry.version}: no licence declared`);
    } else if (!ALLOWED.has(licence)) {
        problems.push(`${name}@${entry.version}: ${licence} is not in the allowlist`);
    }
    if (entry.hasInstallScript === true) {
        problems.push(`${name}@${entry.version}: declares an install script (none is allowed; .npmrc ignores scripts)`);
    }
}

for (const [licence, n] of [...counts].sort()) {
    console.log(`${String(n).padStart(4)}  ${licence}`);
}
if (problems.length > 0) {
    console.error(`\n${problems.length} problem(s):\n  ${problems.join('\n  ')}`);
    process.exit(1);
}
console.log(`licenses ok: ${[...counts.values()].reduce((a, b) => a + b, 0)} packages, all allowed`);
