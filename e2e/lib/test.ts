// The suite's own `test` (decision 121): the one every spec that opens a page of Nage imports in
// place of Playwright's. Its `page` is watched from before its first navigation for any report of a
// Content Security Policy violation, in either browser (lib/csp.ts, which names the one report it
// sets aside); when the test's own steps have passed, a report fails the test, and so does a page
// in view on which htmx has not loaded with the configuration of Nage's layout (decision 40). So a
// test that takes this `page` ends on a page of Nage's layout; a spec whose pages are synthetic (the
// self-tests of the phone check and of the policy check) keeps Playwright's own `test`. `bypassCSP`
// is never set (decision 73).
import { expect, test as base } from '@playwright/test';
import { expectHtmxLoaded, expectNoPolicyViolation, watchPolicy } from './csp';

/** The suite's own options, which playwright.config.ts sets for each project. */
export interface SuiteOptions {
    /**
     * The file that holds the signed-in state of this project's server (decision 121): the set-up
     * project of the server saves it, and the project that ends the chain starts its tests from it.
     */
    stateFile: string;
}

export const test = base.extend<SuiteOptions>({
    stateFile: ['', { option: true }],
    page: async ({ page }, use, testInfo) => {
        const watch = await watchPolicy(page);
        await use(page);
        // A test whose own steps failed keeps that failure alone.
        if (testInfo.status !== 'passed') {
            return;
        }
        await expectNoPolicyViolation(watch);
        await expectHtmxLoaded(page);
    },
});

export { expect };
