// Signing in through the browser, in both chains (decision 121): the sign-in page for a person who
// is not signed in, and what it answers to one who is (decision 118). The tests of the first group
// use a browser without the saved state, and the refused one types a username no other test uses,
// since the pause of decision 111 counts misses by username on the server the tests share.
import { openMenu } from '../lib/menu';
import { HOUSEHOLD, SIGNED_OUT, signIn } from '../lib/session';
import { expect, test } from '../lib/test';

const REFUSED =
    'You were not signed in because the username or the password is not right. Check both and try again; if you have forgotten your password, ask an admin to reset it.';

test.describe('a person who is not signed in', () => {
    test.use({ storageState: SIGNED_OUT });

    /**
     * Without a session the home address leads to the sign-in page; the username and the password
     * the server was set up with sign the person in and lead to the home page, whose menu says who
     * is signed in.
     * Proves: decision 121 (both chains sign in and open the home page); INV-27 and decision 106
     * through a real browser (no page without a session; a sign-in gives one).
     */
    test('the sign-in page signs the user in and leads to the home page', async ({ page }) => {
        await page.goto('/');
        expect(new URL(page.url()).pathname).toBe('/login');
        await expect(page.getByRole('heading', { level: 1, name: 'Sign in to Nage' })).toBeVisible();
        await signIn(page, HOUSEHOLD.username, HOUSEHOLD.password);
        await expect(page.getByRole('heading', { level: 1, name: 'Home' })).toBeVisible();
        expect(new URL(page.url()).pathname).toBe('/');
        const menu = await openMenu(page);
        await expect(menu.getByText(`Signed in as ${HOUSEHOLD.displayName}`)).toBeVisible();
    });

    /**
     * A person who asks for the change-password page without a session is led to the sign-in page
     * and, once signed in, to the page they asked for.
     * Proves: decision 109 (`next` carries the page asked for through the sign-in) in a real
     * browser; decision 121.
     */
    test('a sign-in asked for by another page leads back to that page', async ({ page }) => {
        await page.goto('/account/password');
        expect(new URL(page.url()).pathname).toBe('/login');
        await signIn(page, HOUSEHOLD.username, HOUSEHOLD.password);
        await expect(page.getByRole('heading', { level: 1, name: 'Change your password' })).toBeVisible();
        expect(new URL(page.url()).pathname).toBe('/account/password');
    });

    /**
     * A username nobody has is refused with 422: the sign-in page comes back with the one message
     * that says why, keeps the typed username and signs nobody in.
     * Proves: decision 106's refusal as a browser shows it; decision 121 (a refused sign-in uses a
     * fresh browser context and a username that no other test uses).
     */
    test('a sign-in with a username nobody has is refused and says why', async ({ page }) => {
        await page.goto('/login');
        const [response] = await Promise.all([
            page.waitForResponse((answer) => answer.request().method() === 'POST'),
            signIn(page, 'nobody', HOUSEHOLD.password),
        ]);
        expect(response.status()).toBe(422);
        await expect(page.locator('p.notice[role="alert"]')).toHaveText(REFUSED);
        await expect(page.getByLabel('Username')).toHaveValue('nobody');
        expect(new URL(page.url()).pathname).toBe('/login');
        await page.goto('/');
        expect(new URL(page.url()).pathname, 'the home address after the refused sign-in').toBe('/login');
    });
});

test.describe('a person who is signed in', () => {
    /**
     * Opening the sign-in page while signed in leads to the home page, not to the form.
     * Proves: decision 118 (a person already signed in who opens the sign-in page lands on the
     * home page; the HTTP tests prove the 303), in a real browser with the state the set-up
     * project saved (decision 121).
     */
    test('the sign-in page leads a signed-in person to the home page', async ({ page }) => {
        await page.goto('/login');
        await expect(page.getByRole('heading', { level: 1, name: 'Home' })).toBeVisible();
        expect(new URL(page.url()).pathname).toBe('/');
    });

    /**
     * A link to the sign-in page that names a page of Nage leads a signed-in person to that page.
     * Proves: decision 118 (a person already signed in is sent on to `next` when decision 109's
     * test accepts it; the HTTP tests prove the 303).
     */
    test('the sign-in page leads a signed-in person to the page its link asked for', async ({ page }) => {
        await page.goto('/login?next=%2Faccount%2Fpassword');
        await expect(page.getByRole('heading', { level: 1, name: 'Change your password' })).toBeVisible();
        expect(new URL(page.url()).pathname).toBe('/account/password');
    });
});
