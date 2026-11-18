// The home page and the frame around it, in both chains, signed in with the state the set-up
// project saved (decisions 116, 118, 119 and 121): the balances of the starter accounts, the menu as
// each screen shows it, the way to the change-password page and back, and the page for an unknown
// address. No form adds or funds an account before PLAN M2 slice 7, so every figure here is zero.
import { openMenu } from '../lib/menu';
import { HOUSEHOLD } from '../lib/session';
import { expect, test } from '../lib/test';

const NOT_FOUND =
    'Nage could not show this page because it has no page at this address: the address may be mistyped, or the link that led here may be out of date. Go to the home page and find what you were looking for from there.';

/**
 * Right after a set-up with the starter accounts the home page names the day its balances are for
 * and lists Assets with Cash, Checking and Savings and Liabilities with Credit Card, in
 * alphabetical order, each at 0.00, with a total beside each heading and the Net worth line; no
 * group heading shows, since nobody owns an account yet. Which day it names is proven by the HTTP
 * tests, with a clock they set (decision 122).
 * Proves: decision 119 as a browser shows it (the day named; Assets and Liabilities only; accounts
 * at zero shown; alphabetical order; no group heading without an owner; the Net worth line); W1's
 * acceptance with the starter chart requested (the home page lists it); decision 121 (both chains
 * open the home page).
 */
test('the home page shows the starter accounts at zero', async ({ page }) => {
    const response = await page.goto('/');
    expect(response?.status()).toBe(200);
    await expect(page).toHaveTitle('Nage');
    await expect(page.getByRole('heading', { level: 1, name: 'Home' })).toBeVisible();
    await expect(page.getByText(/^Balances at the end of \d{4}-\d{2}-\d{2}\.$/)).toBeVisible();

    const sections = page.locator('section.balances');
    await expect(sections).toHaveCount(2);
    await expect(sections.nth(0).locator('h2 .name')).toHaveText('Assets');
    await expect(sections.nth(0).locator('h2 .amount')).toHaveText('0.00');
    await expect(sections.nth(0).locator('li .name')).toHaveText(['Cash', 'Checking', 'Savings']);
    await expect(sections.nth(0).locator('li .amount')).toHaveText(['0.00', '0.00', '0.00']);
    await expect(sections.nth(1).locator('h2 .name')).toHaveText('Liabilities');
    await expect(sections.nth(1).locator('h2 .amount')).toHaveText('0.00');
    await expect(sections.nth(1).locator('li .name')).toHaveText(['Credit Card']);
    await expect(sections.nth(1).locator('li .amount')).toHaveText(['0.00']);
    await expect(page.locator('section.balances h3')).toHaveCount(0);
    await expect(page.locator('p.net-worth .name')).toHaveText('Net worth');
    await expect(page.locator('p.net-worth .amount')).toHaveText('0.00');
});

/**
 * The menu lists Home, Change password, who is signed in and Sign out, and marks Home as the page
 * in view. On a wide screen the entries show at once, with no Menu button; on a narrow one only
 * the Menu button shows until it is tapped, and the wide list does not show at all. How the wide
 * list is laid out, as one row, is the stylesheet's and is measured by no test.
 * Proves: decision 116 (the bar and its menu; the same entries shown at once on a computer's
 * screen and behind the Menu button on a phone's; the page in view marked).
 */
test('the menu shows its entries as the width of the screen asks', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('header.bar > strong')).toHaveText('Nage');
    const button = page.getByRole('button', { name: 'Menu' });
    const row = page.getByRole('navigation', { name: 'Menu' });
    const wide = (page.viewportSize()?.width ?? 0) >= 768;
    if (wide) {
        await expect(row).toBeVisible();
        await expect(button).toBeHidden();
    } else {
        await expect(button).toBeVisible();
        await expect(row).toBeHidden();
        await expect(page.getByRole('link', { name: 'Change password' })).toBeHidden();
        await expect(page.getByRole('button', { name: 'Sign out' })).toBeHidden();
    }
    const menu = await openMenu(page);
    await expect(menu.getByRole('link')).toHaveText(['Home', 'Change password']);
    await expect(menu.getByRole('link', { name: 'Home' })).toHaveAttribute('aria-current', 'page');
    await expect(menu.getByRole('link', { name: 'Change password' })).not.toHaveAttribute('aria-current');
    await expect(menu.getByText(`Signed in as ${HOUSEHOLD.displayName}`)).toBeVisible();
    await expect(menu.getByRole('button', { name: 'Sign out' })).toBeVisible();
});

/**
 * The menu's Change password entry leads to the change-password page, which carries the menu too
 * and marks its own entry; the menu's Home entry leads back.
 * Proves: decision 116 (the menu links the change-password page, which decisions 106 and 112 left
 * reachable only by its address; the home page and the change-password page carry the menu, each
 * marking its own entry).
 */
test('the menu leads to the change-password page and back to the home page', async ({ page }) => {
    await page.goto('/');
    await (await openMenu(page)).getByRole('link', { name: 'Change password' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Change your password' })).toBeVisible();
    expect(new URL(page.url()).pathname).toBe('/account/password');
    const menu = await openMenu(page);
    await expect(menu.getByRole('link', { name: 'Change password' })).toHaveAttribute('aria-current', 'page');
    await expect(menu.getByRole('link', { name: 'Home' })).not.toHaveAttribute('aria-current');
    await menu.getByRole('link', { name: 'Home' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Home' })).toBeVisible();
    expect(new URL(page.url()).pathname).toBe('/');
});

/**
 * An address no page answers shows, with 404, the page for an unknown address under the menu: it
 * says in three parts what happened, never repeats the address, marks no entry of the menu, and
 * its button leads to the home page.
 * Proves: decision 118 as a browser shows it (the page for an unknown address, behind the sign-in,
 * with the menu; the address is never printed; the home page offered).
 */
test('an unknown address shows the page for it, which leads to the home page', async ({ page }) => {
    const response = await page.goto('/no-such-page?words=put-here-by-a-link');
    expect(response?.status()).toBe(404);
    await expect(page).toHaveTitle('Nage');
    await expect(page.getByRole('heading', { level: 1, name: 'Page not found' })).toBeVisible();
    await expect(page.locator('main p')).toHaveText([NOT_FOUND]);
    await expect(page.locator('body')).not.toContainText('no-such-page');
    await expect(page.locator('body')).not.toContainText('put-here-by-a-link');
    const menu = await openMenu(page);
    await expect(menu.getByText(`Signed in as ${HOUSEHOLD.displayName}`)).toBeVisible();
    await expect(menu.locator('[aria-current]')).toHaveCount(0);
    await page.getByRole('button', { name: 'Go to the home page' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Home' })).toBeVisible();
    expect(new URL(page.url()).pathname).toBe('/');
});
