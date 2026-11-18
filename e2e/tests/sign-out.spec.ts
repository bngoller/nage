// Signing out through the menu, in both chains (decisions 116, 117, 119 and 121). Each test signs
// in itself, in a browser without the saved state, so the session it ends is its own and the tests
// that share the server keep theirs.
import { openMenu } from '../lib/menu';
import { HOUSEHOLD, SIGNED_OUT, signIn } from '../lib/session';
import { expect, test } from '../lib/test';

test.use({ storageState: SIGNED_OUT });

/**
 * A person signs in, opens the menu and presses Sign out: the sign-in page says once, as a status
 * and not as an alert, that they are signed out; a reload does not say it again; and the home
 * address then leads to the sign-in page, since the session is gone.
 * Proves: decision 116 (Sign out is a button of the menu that posts to `/logout`); decision 117
 * (the sign-out's one-time message appears on the next page, once, and a reload shows it no more);
 * decision 106 (the session ends); decision 121 (a test that signs out uses a fresh browser
 * context).
 */
test('Sign out in the menu leads to the sign-in page, which says so once', async ({ page }) => {
    await page.goto('/login');
    await signIn(page, HOUSEHOLD.username, HOUSEHOLD.password);
    await expect(page.getByRole('heading', { level: 1, name: 'Home' })).toBeVisible();

    await (await openMenu(page)).getByRole('button', { name: 'Sign out' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Sign in to Nage' })).toBeVisible();
    expect(new URL(page.url()).pathname).toBe('/login');
    await expect(page.locator('p.notice')).toHaveText(['You are signed out.']);
    await expect(page.locator('p.notice')).toHaveAttribute('role', 'status');

    await page.reload();
    await expect(page.getByRole('heading', { level: 1, name: 'Sign in to Nage' })).toBeVisible();
    await expect(page.locator('p.notice')).toHaveCount(0);

    await page.goto('/');
    expect(new URL(page.url()).pathname, 'the home address after the sign-out').toBe('/login');
    await expect(page.locator('p.notice')).toHaveCount(0);
});

/**
 * After Sign out, the browser's Back button shows the sign-in page and not the home page it showed
 * before: every page of Nage is sent with `Cache-Control: no-store`, so going back asks Nage again,
 * which sends a browser without a session to the sign-in page, and no balance is on screen.
 * Measured: Playwright's WebKit asks Nage again on Back with or without the header, so in the
 * phone chain this test proves what the person sees and cannot notice a lost header; without the
 * header Chromium shows the earlier home page from its memory, so the chromium-desktop run and the
 * HTTP tests guard the header, as decision 121 says.
 * Proves: decision 119 (every answer but a served asset carries `Cache-Control: no-store`, so Back
 * after a sign-out leads to the sign-in page); INV-27 as seen from the device (no page behind the
 * sign-in is shown without a valid session); decision 121 (a test that signs out uses a fresh
 * browser context).
 */
test('Back after Sign out shows the sign-in page, not the home page', async ({ page }) => {
    await page.goto('/login');
    await signIn(page, HOUSEHOLD.username, HOUSEHOLD.password);
    await expect(page.getByRole('heading', { level: 1, name: 'Home' })).toBeVisible();

    await (await openMenu(page)).getByRole('button', { name: 'Sign out' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Sign in to Nage' })).toBeVisible();

    await page.goBack();
    await expect(page.getByRole('heading', { level: 1, name: 'Sign in to Nage' })).toBeVisible();
    expect(new URL(page.url()).pathname, 'the address after Back').toBe('/login');
    await expect(page.getByRole('heading', { level: 1, name: 'Home' })).toHaveCount(0);
    await expect(page.getByText('Net worth')).toHaveCount(0);
});
