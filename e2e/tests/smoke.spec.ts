// The milestone 1 smoke test (PLAN M1 slice 7; decision 73): the server built from this tree
// answers `/` with the page title and `/health` with 200, in both browser projects.
import { expect, test } from '@playwright/test';

/**
 * `/` is served and carries the title every Nage page has (base.html).
 * Proves: PLAN M1 slice 7's smoke ("`/` shows the title") in both projects of decision 31.
 */
test('the start page shows the title', async ({ page }) => {
    const response = await page.goto('/');
    expect(response?.status()).toBe(200);
    await expect(page).toHaveTitle('Nage');
});

/**
 * `/health` answers 200 on the freshly migrated database the server created at start; only the
 * status is a contract (SPEC 5.2).
 * Proves: PLAN M1 slice 7's smoke ("`/health` returns 200"); SPEC 5.2, decision 55.
 */
test('the health endpoint answers 200', async ({ request }) => {
    const response = await request.get('/health');
    expect(response.status()).toBe(200);
});
