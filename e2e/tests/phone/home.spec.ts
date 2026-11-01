// The phone check on the home page, the first of SPEC 10.3's named pages (webkit-phone only).
import { expect, test } from '@playwright/test';
import { expectPhoneLayout } from '../../lib/phone';

/**
 * The home page passes the full phone check: no page-level horizontal scroll at 390 CSS px and
 * every interactive element outside running text at least 44×44 CSS px (none exists yet, so the
 * second rule is proven by tests/phone-check.spec.ts).
 * Proves: SPEC 10.3 "Phone layout (G6)" for home; decision 49's phone gate for the home page (slice 3's busy page waits for milestone 2 slice 5); decision 74.
 */
test('the home page passes the phone check', async ({ page }) => {
    const response = await page.goto('/');
    expect(response?.status()).toBe(200);
    await expectPhoneLayout(page);
});
