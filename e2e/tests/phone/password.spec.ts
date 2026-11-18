// The phone check on the change-password page (decisions 74, 115 and 121), in webkit-phone with the
// state the set-up project saved: rule 1, as for every screen that is not a named page of SPEC 10.3.
// The form is never sent: no e2e test changes the password, which would end the other tests'
// sessions (decision 112), and a wrong current password would count towards the pause of decision
// 111 on the server the tests share.
import { expectNoHorizontalScroll } from '../../lib/phone';
import { expect, test } from '../../lib/test';

/**
 * The change-password page, under the bar, does not scroll sideways on a phone.
 * Proves: SPEC 10.3 "Phone layout (G6)", rule 1, for the change-password page, which discharges
 * decision 115's assignment of its check to this slice; decision 116 (the page carries the bar).
 */
test('the change-password page does not scroll sideways', async ({ page }) => {
    const response = await page.goto('/account/password');
    expect(response?.status()).toBe(200);
    await expect(page.getByRole('heading', { level: 1, name: 'Change your password' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Menu' })).toBeVisible();
    await expectNoHorizontalScroll(page);
});
