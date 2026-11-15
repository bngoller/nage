// The phone check of what a phone reaches at the home address (webkit-phone only). The home page is
// behind the sign-in since milestone 2 slice 5 and this suite cannot sign in until slice 6's set-up
// project, so until then the home page's own check waits (decision 115) and this file checks the
// page a first visitor is led to from `/`: the set-up page.
import { expect, test } from '@playwright/test';
import { expectPhoneLayout } from '../../lib/phone';

/**
 * A phone that opens `/` on a fresh server arrives at the set-up page, which passes the full phone
 * check: no page-level horizontal scroll at 390 CSS px and every interactive element outside running
 * text at least 44×44 CSS px.
 * Proves: decision 115 ("the home page's phone check becomes the set-up page's, with both rules");
 * SPEC W1 step 1 on a phone; decision 74.
 */
test('the home address of a fresh server leads to a set-up page that passes the phone check', async ({ page }) => {
    const response = await page.goto('/');
    expect(response?.status()).toBe(200);
    expect(new URL(page.url()).pathname).toBe('/setup');
    await expectPhoneLayout(page);
});
