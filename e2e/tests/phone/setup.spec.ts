// The phone check on the set-up page, a named page of SPEC 10.3 (decisions 105, 115 and 121). The
// set-up page exists only while the server has no user, so this file runs in fresh-phone, the first
// project of the phone server's chain, before that server's set-up project. No test here sends a
// form that would be accepted: the server stays without a user for the set-up that follows.
import { MIN_TAP_TARGET_PX, expectPhoneLayout } from '../../lib/phone';
import { expect, test } from '../../lib/test';

/**
 * The set-up page passes the full phone check, and rule 2 measures real elements on it: the five
 * boxes, the currency list, the button and the starter-chart tick box, which is ticked and at least
 * 44×44 CSS px itself, so it is a tap target as it stands.
 * Proves: SPEC 10.3 "Phone layout (G6)" for the set-up page; decision 105 (the large tick box meets
 * decision 74's rule as it stands, ticked by default); decision 115; decision 74.
 */
test('the set-up page passes the phone check', async ({ page }) => {
    const response = await page.goto('/setup');
    expect(response?.status()).toBe(200);
    await expect(page.locator('form input:not([type="hidden"]), form select, form button')).toHaveCount(8);
    const tick = page.locator('input[type="checkbox"][name="starter_chart"]');
    await expect(tick).toBeChecked();
    const box = await tick.boundingBox();
    expect(box?.width ?? 0, 'width of the starter-chart tick box').toBeGreaterThanOrEqual(MIN_TAP_TARGET_PX);
    expect(box?.height ?? 0, 'height of the starter-chart tick box').toBeGreaterThanOrEqual(MIN_TAP_TARGET_PX);
    await expectPhoneLayout(page);
});

/**
 * A refused set-up still passes the full phone check: sent with every box empty, the form comes back
 * with 422 and four problems, one for each box an empty form breaks (the book name, the username,
 * the display name and the password; the second password box, as empty as the first, has none),
 * and the long messages cause no horizontal scroll. Nothing is set up by it.
 * Proves: SPEC 10.3 "Phone layout (G6)" for the set-up page in its refused state; decision 105 (a
 * refused form is shown again with every problem at once).
 */
test('a refused set-up form passes the phone check', async ({ page }) => {
    await page.goto('/setup');
    const [response] = await Promise.all([
        page.waitForResponse((answer) => answer.request().method() === 'POST'),
        page.getByRole('button', { name: 'Set up Nage' }).click(),
    ]);
    expect(response.status()).toBe(422);
    await expect(page.locator('small.problem')).toHaveCount(4);
    await expectPhoneLayout(page);
});
