// The self-test of the policy check (decisions 40 and 121; lib/csp.ts), in both chains: a real page
// of Nage passes it, and synthetic pages that carry Nage's real policy prove that it fails on an
// inline script, on an inline style and on htmx loaded without the layout's configuration, and that
// the one report it sets aside, WebKit's own for a `select`, is that report and no other. Each
// synthetic page is answered by the test itself at an address of the server, with the policy header
// copied from one of the server's own answers, so the browser enforces the policy the household's
// pages have. The tests take Playwright's own `test`: the suite's `test` would fail them at their
// end, which is what they prove of the helper. They need no session. The last two take the
// suite's own `test` (as `suiteTest`) and are marked to fail: each ends on a page that the suite's
// `test` must fail, which is what they prove of the suite's `test`.
import { expect, test, type APIRequestContext, type Page } from '@playwright/test';
import { POLICY_HEADER, expectHtmxLoaded, expectNoPolicyViolation, watchPolicy } from '../lib/csp';
import { SIGNED_OUT } from '../lib/session';
import { test as suiteTest } from '../lib/test';

test.use({ storageState: SIGNED_OUT });

function html(head: string, body: string): string {
    return `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Nage</title>${head}</head><body>${body}</body></html>`;
}

/** The policy the server sends, read from an answer that needs no session (decision 40: every response carries it). */
async function realPolicy(request: APIRequestContext): Promise<string> {
    const policy = (await request.get('/health')).headers()[POLICY_HEADER] ?? '';
    expect(policy, 'the policy of the server\'s own answers').toContain("script-src 'self'");
    expect(policy, 'the policy of the server\'s own answers').toContain("style-src 'self'");
    return policy;
}

/** Opens a synthetic page at `path` of the server, answered here with the server's real policy. */
async function openUnderThePolicy(page: Page, request: APIRequestContext, path: string, content: string): Promise<void> {
    const policy = await realPolicy(request);
    await page.route(`**${path}`, (route) =>
        route.fulfill({
            status: 200,
            contentType: 'text/html; charset=utf-8',
            headers: { [POLICY_HEADER]: policy },
            body: content,
        }),
    );
    const response = await page.goto(path);
    expect(response?.headers()[POLICY_HEADER], 'the policy of the synthetic page').toBe(policy);
}

/**
 * The sign-in page, a real page of Nage, passes the whole check: watched from before its first
 * navigation it reports nothing, and htmx has loaded on it with `allowEval` and
 * `includeIndicatorStyles` off.
 * Proves: decision 40's check as decision 121 carries it out (no report of a Content Security
 * Policy violation with htmx loaded, in both browsers); the watch itself breaks no policy.
 */
test('a page of Nage reports no violation and has htmx loaded with its configuration', async ({ page }) => {
    const watch = await watchPolicy(page);
    const response = await page.goto('/login');
    expect(response?.status()).toBe(200);
    await expectHtmxLoaded(page);
    expect(await watch.reports()).toEqual([]);
    await expectNoPolicyViolation(watch);
});

/**
 * A synthetic page that keeps the policy, with no inline code, reports nothing: the watch does not
 * mistake its own presence, or a page without htmx, for a violation.
 * Proves: decision 121's self-test, the passing side (a report comes only from a violation).
 */
test('a synthetic page that keeps the policy reports nothing', async ({ page, request }) => {
    const watch = await watchPolicy(page);
    await openUnderThePolicy(page, request, '/csp-check/clean', html('', '<p id="text">Nothing inline here.</p>'));
    await expect(page.locator('#text')).toBeVisible();
    expect(await watch.reports()).toEqual([]);
    await expectNoPolicyViolation(watch);
});

/**
 * An inline script under the real policy does not run, is reported, and fails the check.
 * Proves: decision 121's self-test (a page with the real policy and an inline script makes the
 * check fail); SPEC 5.3 (the policy refuses inline scripts in both browsers).
 */
test('an inline script under the real policy fails the check', async ({ page, request }) => {
    const watch = await watchPolicy(page);
    await openUnderThePolicy(
        page,
        request,
        '/csp-check/inline-script',
        html('', '<p id="text">before</p><script>document.getElementById("text").textContent = "the inline script ran";</script>'),
    );
    await expect(page.locator('#text')).toHaveText('before');
    expect((await watch.reports()).length, 'reports of the inline script').toBeGreaterThan(0);
    await expect(expectNoPolicyViolation(watch)).rejects.toThrow();
});

/**
 * An inline style under the real policy is not applied, is reported, and fails the check: a `style`
 * element and a `style` attribute alike.
 * Proves: decision 121's self-test (a page with the real policy and an inline style makes the check
 * fail); SPEC 5.3 (the policy refuses inline styles in both browsers).
 */
test('an inline style under the real policy fails the check', async ({ context, request }) => {
    for (const [path, head, body] of [
        ['/csp-check/style-element', '<style>#text { display: none; }</style>', '<p id="text">still shown</p>'],
        ['/csp-check/style-attribute', '', '<p id="text" style="display: none">still shown</p>'],
    ]) {
        const page = await context.newPage();
        const watch = await watchPolicy(page);
        await openUnderThePolicy(page, request, path, html(head, body));
        await expect(page.locator('#text'), `the text of ${path}`).toBeVisible();
        expect((await watch.reports()).length, `reports of ${path}`).toBeGreaterThan(0);
        await expect(expectNoPolicyViolation(watch)).rejects.toThrow();
    }
});

/**
 * A `select` without a `style` attribute fails nothing in either browser, so the set-up page with
 * its currency list passes the check. WebKit raises an inline-style report of its own for the
 * element, which the watch sets aside; Chromium raises none, and nothing is set aside there.
 * Proves: the one exception of lib/csp.ts (WebKit's own report for a `select` is set aside, not
 * counted); that WebKit still raises it and Chromium does not, so the exception is dropped when
 * its reason goes; decision 121's check on a page with a `select`.
 */
test('a select alone fails nothing, and the report WebKit raises for it is set aside', async ({ page, request, browserName }) => {
    const watch = await watchPolicy(page);
    await openUnderThePolicy(page, request, '/csp-check/select', html('', '<select id="list"><option>CAD</option></select>'));
    await expect(page.locator('#list')).toBeVisible();
    expect(await watch.reports()).toEqual([]);
    await expectNoPolicyViolation(watch);
    expect((await watch.setAside()).length, 'reports set aside').toBe(browserName === 'webkit' ? 1 : 0);
});

/**
 * A `select` that carries a `style` attribute is reported and fails the check like any element
 * with an inline style: the exception covers a `select` without the attribute and nothing more.
 * Proves: the exception of lib/csp.ts is narrow (an inline style on a `select` still fails the
 * check in both browsers); SPEC 5.3.
 */
test('a select that carries a style attribute fails the check', async ({ page, request }) => {
    const watch = await watchPolicy(page);
    await openUnderThePolicy(
        page,
        request,
        '/csp-check/select-with-style',
        html('', '<select id="list" style="display: none"><option>CAD</option></select>'),
    );
    await expect(page.locator('#list')).toBeVisible();
    expect((await watch.reports()).length, 'reports of the style attribute').toBeGreaterThan(0);
    await expect(expectNoPolicyViolation(watch)).rejects.toThrow();
});

/**
 * htmx loaded without the layout's configuration is caught twice, on a page opened after a page of
 * Nage that reported nothing: it adds its indicator styles as an inline style, which the policy
 * refuses and the watch reports, and the assertion on its configuration fails, since both settings
 * are then on.
 * Proves: decision 40 (the base layout's htmx configuration, `allowEval` and
 * `includeIndicatorStyles` off, is what keeps htmx inside the policy) and decision 121 (the check
 * asserts that htmx loaded with its meta configuration).
 */
test('htmx without the configuration of the layout fails the check', async ({ page, request }) => {
    const watch = await watchPolicy(page);
    await page.goto('/login');
    const htmx = await page.locator('script[src]').getAttribute('src');
    expect(htmx, 'the address of the htmx script on a page of Nage').toMatch(/^\/static\/htmx\.min\.[0-9a-f]+\.js$/);
    expect(await watch.reports(), 'reports of the sign-in page').toEqual([]);

    await openUnderThePolicy(
        page,
        request,
        '/csp-check/htmx-defaults',
        html(`<script src="${htmx}" defer></script>`, '<p id="text">htmx with its defaults</p>'),
    );
    await expect(page.locator('#text')).toBeVisible();
    await expect(expectHtmxLoaded(page)).rejects.toThrow();
    expect((await watch.reports()).length, 'reports of the indicator styles htmx adds').toBeGreaterThan(0);
    await expect(expectNoPolicyViolation(watch)).rejects.toThrow();
});

/**
 * A page that keeps the policy and has no htmx reports nothing, yet fails the assertion that htmx
 * has loaded: the two halves of the check are separate.
 * Proves: decision 121 (the check asserts that htmx loaded; a layout that lost its script would
 * fail every test that opens a page, although no violation is reported).
 */
test('a page without htmx fails the assertion that it has loaded', async ({ page, request }) => {
    const watch = await watchPolicy(page);
    await openUnderThePolicy(page, request, '/csp-check/no-htmx', html('', '<p id="text">no script at all</p>'));
    await expect(page.locator('#text')).toBeVisible();
    expect(await watch.reports()).toEqual([]);
    await expect(expectHtmxLoaded(page)).rejects.toThrow();
});

/**
 * The suite's own `test` (lib/test.ts) fails a test whose page broke the policy, although every step
 * of the test passed: the page has htmx loaded with the layout's configuration and an inline script,
 * which the policy refuses. The test is marked as one that must fail, so it passes only when the
 * suite's `test` fails it.
 * Proves: decision 121 (the policy check is applied to the page of every test that takes the suite's
 * `test`; a `test` that lost the check would let this one pass, which fails the run).
 */
suiteTest('the suite\'s test fails a test whose page broke the policy', async ({ page, request }) => {
    suiteTest.fail();
    await page.goto('/login');
    const htmx = await page.locator('script[src]').getAttribute('src');
    await openUnderThePolicy(
        page,
        request,
        '/csp-check/suite-inline-script',
        html(
            `<meta name="htmx-config" content='{"allowEval":false,"includeIndicatorStyles":false}'><script src="${htmx}" defer></script>`,
            '<p id="text">before</p><script>document.getElementById("text").textContent = "the inline script ran";</script>',
        ),
    );
    await expect(page.locator('#text')).toHaveText('before');
});

/**
 * The suite's own `test` fails a test that ends on a page without htmx, although the page reported
 * nothing and every step of the test passed. The test is marked as one that must fail.
 * Proves: decision 121 (the suite's `test` asserts that htmx loaded on the page in view; a `test`
 * that lost the assertion would let this one pass, which fails the run).
 */
suiteTest('the suite\'s test fails a test that ends on a page without htmx', async ({ page, request }) => {
    suiteTest.fail();
    await openUnderThePolicy(page, request, '/csp-check/suite-no-htmx', html('', '<p id="text">no script at all</p>'));
    await expect(page.locator('#text')).toBeVisible();
});
