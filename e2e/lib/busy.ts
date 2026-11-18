// What the busy page's check needs (decision 121): a Nage server of its own, on a data directory and
// a port of its own, and a hold on that database's write lock from outside, through the SQLite
// module built into Node (no package is added). The servers that serve.mjs starts are shared by all
// the tests of a chain, where a held lock would turn other tests' writes into busy pages; this
// server serves one test.
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { setTimeout as pause } from 'node:timers/promises';

/** The port of the busy check's own server, beside decision 73's 8791 and 8792. */
export const OWN_SERVER_PORT = 8793;

/** A Nage server that one test started for itself. */
export interface OwnServer {
    /** `http://127.0.0.1:<port>`. */
    baseURL: string;
    /** The server's data directory, which holds `nage.sqlite`. */
    dataDir: string;
    /** Stops the server with SIGTERM, waits for it to end and removes the data directory. */
    stop(): Promise<void>;
}

/**
 * Starts the binary of `cargo build -p nage` as `serve.mjs` does, with `--tz UTC`, on
 * `OWN_SERVER_PORT` and a fresh temporary data directory, and waits until `/health` answers 200.
 * Fails when the server ends before that, as it does when the port is in use, and when the data
 * directory then holds no database.
 */
export async function startOwnServer(): Promise<OwnServer> {
    const binary = resolve(__dirname, '..', '..', 'target', 'debug', 'nage');
    if (!existsSync(binary)) {
        throw new Error(`${binary} does not exist; run \`cargo build -p nage\` first`);
    }
    const baseURL = `http://127.0.0.1:${OWN_SERVER_PORT}`;
    // A server that answers already is one an earlier run left behind: fail at once instead of testing it.
    if (await answersHealth(baseURL)) {
        throw new Error(`a server already answers on port ${OWN_SERVER_PORT}, left by an earlier run; stop it first`);
    }
    const dataDir = mkdtempSync(join(tmpdir(), 'nage-e2e-busy-'));
    // As serve.mjs starts the suite's servers: the address and the zone are explicit, so nothing in the
    // developer's environment changes them.
    const child = spawn(
        binary,
        ['serve', '--bind', `127.0.0.1:${OWN_SERVER_PORT}`, '--data-dir', dataDir, '--tz', 'UTC'],
        { stdio: 'ignore' },
    );
    let ending = '';
    const ended = new Promise<void>((done) => {
        child.once('exit', (code, signal) => {
            ending = signal === null ? `exit status ${code}` : `signal ${signal}`;
            done();
        });
        child.once('error', (error) => {
            ending = error.message;
            done();
        });
    });
    const stop = async (): Promise<void> => {
        if (ending === '') {
            child.kill('SIGTERM');
            // nage stops cleanly within a moment; a server that does not within the 10 s the suite's own
            // servers are given (playwright.config.ts) is killed, so that it does not hold the port.
            const late = setTimeout(() => child.kill('SIGKILL'), 10_000);
            await ended;
            clearTimeout(late);
        }
        rmSync(dataDir, { recursive: true, force: true });
    };
    const fail = async (why: string): Promise<never> => {
        await stop();
        throw new Error(`the busy check's own server on port ${OWN_SERVER_PORT}: ${why}`);
    };
    const deadline = Date.now() + 30_000;
    while (!(await answersHealth(baseURL))) {
        if (ending !== '') {
            return fail(`it ended (${ending}) before /health answered; is the port in use?`);
        }
        if (Date.now() > deadline) {
            return fail('/health did not answer within 30 s');
        }
        await pause(100);
    }
    if (ending !== '') {
        return fail(`it ended (${ending}), and /health was answered by another server`);
    }
    if (!existsSync(join(dataDir, 'nage.sqlite'))) {
        return fail(`/health answered, but ${dataDir} holds no database`);
    }
    return { baseURL, dataDir, stop };
}

/**
 * Takes the write lock of the database in `dataDir`, which must exist, in a connection of this
 * process, and returns the function that gives it back. While it is held the server's reads still
 * work and each of its writes waits about five seconds and then answers the busy page (SPEC 5.4).
 */
export function holdWriteLock(dataDir: string): () => void {
    const path = join(dataDir, 'nage.sqlite');
    // Opening a path that does not exist would create an empty database there.
    if (!existsSync(path)) {
        throw new Error(`${path} does not exist; the lock is taken on the database of a running server`);
    }
    // A write the server has under way is waited for, up to the five seconds the server itself waits.
    const database = new DatabaseSync(path, { timeout: 5_000 });
    try {
        database.exec('BEGIN IMMEDIATE');
    } catch (error) {
        database.close();
        throw error;
    }
    let held = true;
    return () => {
        if (held) {
            held = false;
            database.exec('ROLLBACK');
            database.close();
        }
    };
}

/** Whether `/health` at `baseURL` answers 200 within a second. */
async function answersHealth(baseURL: string): Promise<boolean> {
    try {
        const answer = await fetch(`${baseURL}/health`, { signal: AbortSignal.timeout(1_000) });
        await answer.body?.cancel();
        return answer.status === 200;
    } catch {
        return false;
    }
}
