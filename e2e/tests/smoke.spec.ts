// The smoke test (PLAN M1 slice 7; decisions 73, 115 and 121): the server built from this tree,
// started on an empty data directory, leads `/` to its set-up page, which carries the page title,
// and answers `/health` with 200. These checks need a server nobody has set up, so they run in the
// first project of each server's chain (fresh-desktop and fresh-phone), before that server's set-up
// project; no test here sets the server up.
import { expect, test } from '../lib/test';

/**
 * A fresh server sends `/` to `/setup` (SPEC W1 step 1), and the page the browser ends on answers
 * 200 and carries the title every Nage page has (base.html). On it the help line under the
 * starter-accounts tick box starts below the box, so no part of the box covers its text. The page
 * is watched like every page of Nage a test opens: it reports no policy violation and htmx has
 * loaded on it with its configuration.
 * Proves: decision 115's smoke ("`/` ends at `/setup` and keeps its title"); decision 109's redirect
 * through a real browser; decision 73, in both browsers of decision 31; decision 105 (the large
 * tick box sits above its help line, at a desktop's width and at a phone's); decision 121 (the
 * fresh-server checks run before the set-up; the set-up page keeps the policy with htmx loaded).
 */
test('the start page of a fresh server leads to the set-up page, which shows the title', async ({ page }) => {
    const response = await page.goto('/');
    expect(response?.status()).toBe(200);
    expect(new URL(page.url()).pathname).toBe('/setup');
    await expect(page).toHaveTitle('Nage');
    const box = await page.locator('input[type="checkbox"][name="starter_chart"]').boundingBox();
    const help = await page.locator('label.tick + small.help').boundingBox();
    expect(box, 'the starter-accounts tick box').not.toBeNull();
    expect(help, 'the help line under the tick box').not.toBeNull();
    const boxBottom = (box?.y ?? 0) + (box?.height ?? 0);
    expect(help?.y ?? 0, 'top edge of the help line under the tick box')
        .toBeGreaterThanOrEqual(boxBottom);
});

/**
 * `/health` answers 200 on the freshly migrated database the server created at start; only the
 * status is a contract (SPEC 5.2), and it needs no session.
 * Proves: PLAN M1 slice 7's smoke ("`/health` returns 200"); SPEC 5.2, decision 55.
 */
test('the health endpoint answers 200', async ({ request }) => {
    const response = await request.get('/health');
    expect(response.status()).toBe(200);
});
