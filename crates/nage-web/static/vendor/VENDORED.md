# Vendored web assets

Third-party files served by nage-web, minified as their projects distribute them (SPEC decision 46). They are fetched with `curl -fsSL -o` from the tagged source tree and never opened in an editor: the minified files end without a newline, and an editor applying `.editorconfig` would change their hash. Each SHA-256 was cross-checked against the npm package served by unpkg; both copies matched. The license files are the projects' own, unmodified.

Upgrading a file means fetching the new tag the same way, updating this table, and updating the hashes hard-coded in `crates/nage-web/tests` (and, for htmx, the indicator rules copied into `../app.css`).

## htmx.min.js

| Field | Value |
|---|---|
| Project | htmx, https://github.com/bigskysoftware/htmx |
| Version | 2.0.11 (the newest `v2.0.x` tag on 2026-10-27; htmx stays on the 2.x line, decision 46) |
| Tag | `v2.0.11` |
| Tag commit | `fa978b24e75fb03c137bf2cdae4fef0e711cf8a1` |
| Source URL | https://raw.githubusercontent.com/bigskysoftware/htmx/v2.0.11/dist/htmx.min.js |
| Cross-check | https://unpkg.com/htmx.org@2.0.11/dist/htmx.min.js (identical) |
| SHA-256 | `d6fdc75f204e6bdefa99b69bf1e6d4ac69b8a364f77929f45c13476b4000f717` |
| Bytes | 52182 |
| License | 0BSD, in `LICENSE-htmx` |

## pico.classless.min.css

| Field | Value |
|---|---|
| Project | Pico CSS, https://github.com/picocss/pico |
| Version | 2.1.1 (the classless build) |
| Tag | `v2.1.1` |
| Tag commit | `1039a4788d6abc368d5485ae6bac84a8f0e3096f` |
| Source URL | https://raw.githubusercontent.com/picocss/pico/v2.1.1/css/pico.classless.min.css |
| Cross-check | https://unpkg.com/@picocss/pico@2.1.1/css/pico.classless.min.css (identical) |
| SHA-256 | `61207a40ffc02a42d1e50143651c121beab70ed413c934c1ff84fa263ba436b0` |
| Bytes | 71040 |
| License | MIT, in `LICENSE-pico` |

## License files

| File | Source URL | SHA-256 | Bytes |
|---|---|---|---|
| `LICENSE-htmx` | https://raw.githubusercontent.com/bigskysoftware/htmx/v2.0.11/LICENSE | `d3d2456f76414f2456104660ebd65aff1c04cd7966b942bdabd63f3cdb316a38` | 642 |
| `LICENSE-pico` | https://raw.githubusercontent.com/picocss/pico/v2.1.1/LICENSE.md | `afaff063e044233f917b7807dccab022f09dca474f92b642846bee4850655bbf` | 1066 |
