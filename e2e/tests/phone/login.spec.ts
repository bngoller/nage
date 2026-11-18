// The phone check on the sign-in page, a named page of SPEC 10.3 (decisions 74, 115 and 121), in
// webkit-phone. Its tests use a browser without the saved state, since a signed-in person never
// sees this page (decision 118), and the refused one types a username no other test uses.
import { expectPhoneLayout } from '../../lib/phone';
import { HOUSEHOLD, SIGNED_OUT, signIn } from '../../lib/session';
import { expect, test } from '../../lib/test';

test.use({ storageState: SIGNED_OUT });

/**
 * The sign-in page passes the full phone check, and rule 2 measures real elements on it: the two
 * boxes and the button.
 * Proves: SPEC 10.3 "Phone layout (G6)" for the sign-in page, which discharges decision 115's
 * assignment of its check to this slice; decision 74.
 */
test('the sign-in page passes the phone check', async ({ page }) => {
    const response = await page.goto('/login');
    expect(response?.status()).toBe(200);
    await expect(page.getByRole('heading', { level: 1, name: 'Sign in to Nage' })).toBeVisible();
    await expect(page.locator('form input:not([type="hidden"]), form button')).toHaveCount(3);
    await expectPhoneLayout(page);
});

/**
 * A refused sign-in still passes the full phone check: the page comes back with 422 and its
 * message above the form, and the long sentence causes no horizontal scroll.
 * Proves: SPEC 10.3 "Phone layout (G6)" for the sign-in page in its refused state; decision 121 (a
 * refused sign-in types a username that no other test uses).
 */
test('a refused sign-in passes the phone check', async ({ page }) => {
    await page.goto('/login');
    const [response] = await Promise.all([
        page.waitForResponse((answer) => answer.request().method() === 'POST'),
        signIn(page, 'nobody-on-a-phone', HOUSEHOLD.password),
    ]);
    expect(response.status()).toBe(422);
    await expect(page.locator('p.notice[role="alert"]')).toBeVisible();
    await expectPhoneLayout(page);
});
