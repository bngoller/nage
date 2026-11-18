// The set-up of a server (decision 121; SPEC 10.1's setup project, which is two, one per server):
// setup-desktop and setup-phone each run this file against their own server, after that server's
// fresh-server checks and before every test that needs a signed-in user. The set-up goes through
// `/setup` in the browser, as the household's would, and the signed-in state it leaves is saved to
// that server's own file under `playwright/.auth/`, which git ignores.
import { readFileSync } from 'node:fs';
import { openMenu } from '../lib/menu';
import { HOUSEHOLD, SESSION_COOKIE, saveSignedInState, setUp } from '../lib/session';
import { expect, test } from '../lib/test';

/**
 * The set-up page, filled as a household would fill it and sent once with the starter accounts
 * ticked, leads straight to the home page, signed in as the user it created, with the four starter
 * accounts of Assets and Liabilities listed; the state file then holds the session cookie this
 * browser was given, for this server's host alone, so that the tests that follow start signed in.
 * Proves: decision 121 (each set-up project runs `/setup` in the browser and saves that server's
 * signed-in state to a file of its own); W1's acceptance with the starter chart requested, through
 * a real browser (a fresh data directory reaches the home page in one form submission; decisions
 * 105 and 119).
 */
test('the set-up page sets the server up, signs its first user in, and the state is saved', async ({ page, stateFile }) => {
    const response = await page.goto('/setup');
    expect(response?.status()).toBe(200);
    await setUp(page);
    await expect(page.getByRole('heading', { level: 1, name: 'Home' })).toBeVisible();
    expect(new URL(page.url()).pathname).toBe('/');
    await expect(page.locator('section.balances li')).toHaveCount(4);
    const menu = await openMenu(page);
    await expect(menu.getByText(`Signed in as ${HOUSEHOLD.displayName}`)).toBeVisible();

    await saveSignedInState(page, stateFile);
    const session = (await page.context().cookies()).find((cookie) => cookie.name === SESSION_COOKIE);
    expect(session, 'the session cookie of the browser that was set up').toBeDefined();
    const saved = JSON.parse(readFileSync(stateFile, 'utf8')) as { cookies: { name: string; value: string; domain: string }[] };
    const kept = saved.cookies.filter((cookie) => cookie.name === SESSION_COOKIE);
    expect(kept.map((cookie) => cookie.domain), 'the hosts of the saved session cookie').toEqual(['127.0.0.1']);
    expect(kept[0]?.value, 'the saved session cookie').toBe(session?.value);
});
