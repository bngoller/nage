// The phone check on the busy page (decisions 74, 115 and 121; PLAN 1.1), in webkit-phone: rule 1.
// The page is raised on a server of this test's own, so that the held lock disturbs no other test:
// the test starts the server on a data directory and a port of its own, takes that database's write
// lock from outside through the SQLite module built into Node, and sends the set-up form, the one
// write a server without a user accepts. The server waits about five seconds for the lock and then
// answers the busy page (SPEC 5.4).
import { holdWriteLock, startOwnServer } from '../../lib/busy';
import { expectNoHorizontalScroll } from '../../lib/phone';
import { setUp } from '../../lib/session';
import { expect, test } from '../../lib/test';

/**
 * While another process holds the database's write lock, a set-up form sent from a phone is
 * answered with 503 and the busy page, which does not scroll sideways; once the lock is given back
 * the set-up page is still open, since the refused set-up left nothing behind.
 * Proves: SPEC 10.3 "Phone layout (G6)", rule 1, for the busy page, which discharges decision 115's
 * assignment and with it decision 49's phone gate for the busy page (decision 121); SPEC 5.4 (503
 * and the busy page when the database stays locked) through a real server and browser; INV-28 (a
 * busy set-up leaves no trace).
 */
test('the busy page does not scroll sideways', async ({ page }) => {
    const server = await startOwnServer();
    try {
        const release = holdWriteLock(server.dataDir);
        try {
            await page.goto(`${server.baseURL}/setup`);
            const [response] = await Promise.all([
                page.waitForResponse((answer) => answer.request().method() === 'POST', { timeout: 20_000 }),
                setUp(page),
            ]);
            expect(response.status()).toBe(503);
            await expect(page.getByRole('heading', { level: 1, name: 'Nage is busy' })).toBeVisible();
            await expectNoHorizontalScroll(page);
        } finally {
            release();
        }
        const after = await page.goto(`${server.baseURL}/setup`);
        expect(after?.status(), 'the set-up page after the lock was given back').toBe(200);
    } finally {
        await server.stop();
    }
});
