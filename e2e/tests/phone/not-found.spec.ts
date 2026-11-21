// The phone check on the page for an unknown address (decisions 74, 118 and 121), in webkit-phone
// with the state the set-up project saved: rule 1, as for every screen that is not a named page of
// SPEC 10.3.
import { expectNoHorizontalScroll } from '../../lib/phone';
import { expect, test } from '../../lib/test';

/**
 * The page for an unknown address, under the bar, does not scroll sideways on a phone; a long
 * address leads to it here. That the page never prints the address is proven by
 * tests/home.spec.ts in both chains and by the HTTP tests, not here: the style file breaks a word
 * too long for its line (`overflow-wrap` on the page), so a printed address would not widen the
 * page.
 * Proves: SPEC 10.3 "Phone layout (G6)", rule 1, for the page for an unknown address (decision
 * 121).
 */
test('the page for an unknown address does not scroll sideways', async ({ page }) => {
    const response = await page.goto(`/${'a-long-address-nobody-answers-'.repeat(8)}`);
    expect(response?.status()).toBe(404);
    await expect(page.getByRole('heading', { level: 1, name: 'Page not found' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Menu' })).toBeVisible();
    await expectNoHorizontalScroll(page);
});
