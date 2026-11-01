#!/usr/bin/env node
// Starts one Nage server for the e2e suite (decision 73): a fresh temporary data directory,
// the binary of `cargo build -p nage`, 127.0.0.1 and the given port. Playwright
// runs this through `webServer.command`, waits for /health and stops it with SIGTERM sent to
// the whole process group, so nage receives the signal itself; this script waits for nage to
// exit, removes the directory and exits with nage's status.
//
// Usage: node serve.mjs <port>
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const port = Number(process.argv[2]);
if (!Number.isInteger(port) || port < 1 || port > 65535) {
    console.error('usage: node serve.mjs <port>');
    process.exit(2);
}

const here = dirname(fileURLToPath(import.meta.url));
const binary = resolve(here, '..', 'target', 'debug', 'nage');
if (!existsSync(binary)) {
    console.error(`serve.mjs: ${binary} does not exist; run \`cargo build -p nage\` first`);
    process.exit(2);
}

const dataDir = mkdtempSync(join(tmpdir(), 'nage-e2e-'));
console.error(`serve.mjs: port ${port}, data directory ${dataDir}`);

// The zone and the address are explicit: Playwright hands the developer's environment to the
// server, and a TZ or NAGE_TZ in the shell must not change it.
const child = spawn(
    binary,
    ['serve', '--bind', `127.0.0.1:${port}`, '--data-dir', dataDir, '--tz', 'UTC'],
    { stdio: 'inherit' },
);

// A SIGTERM or SIGINT this script receives is passed to nage while nage runs. Playwright
// signals the whole group, so nage then gets it twice and ignores the repeat during its drain;
// a signal sent to this script alone (a manual kill) reaches nage through the forward.
for (const signal of ['SIGTERM', 'SIGINT']) {
    process.on(signal, () => {
        if (child.exitCode === null && child.signalCode === null) {
            child.kill(signal);
        }
    });
}

child.on('error', (error) => {
    console.error(`serve.mjs: cannot start ${binary}: ${error.message}`);
    rmSync(dataDir, { recursive: true, force: true });
    process.exit(1);
});

child.on('exit', (code, signal) => {
    rmSync(dataDir, { recursive: true, force: true });
    if (signal !== null) {
        console.error(`serve.mjs: nage ended by ${signal}`);
        process.exit(1);
    }
    process.exit(code ?? 1);
});
