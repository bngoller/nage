# Vendored web assets

Third-party files served by nage-web, as their projects distribute them (SPEC decisions 46 and 126). They are fetched with `curl -fsSL -o` from the tagged source tree and never opened in an editor: the minified files end without a newline, and an editor applying `.editorconfig` would change their hash. Each SHA-256 was cross-checked against the npm package served by unpkg; both copies matched. The license files are the projects' own, unmodified.

Upgrading a file means fetching the new tag the same way, updating this table, and updating the hashes hard-coded in `crates/nage-web/tests` (and, for htmx, the indicator rules copied into `../app.css`; for a font, its address in `../app.css`, since a served file's address holds the first 16 hex digits of its SHA-256).

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

## IBM Plex fonts

| Field | Value |
|---|---|
| Project | IBM Plex, https://github.com/IBM/plex |
| Version | IBM Plex Sans 1.1.0 and IBM Plex Mono 1.1.0, the maker's Latin-1 split woff2 files, unchanged (decision 126; other alphabets fall back to the device's font) |
| Tags | `@ibm/plex-sans@1.1.0` and `@ibm/plex-mono@1.1.0` |
| Tag commit | `1da12f02587b630c07e92692d21492d722f53614` (both tags) |
| Source URL | https://github.com/IBM/plex/raw/%40ibm%2Fplex-sans%401.1.0/packages/plex-sans/fonts/split/woff2/<file> and https://github.com/IBM/plex/raw/%40ibm%2Fplex-mono%401.1.0/packages/plex-mono/fonts/split/woff2/<file>, fetched on 2026-11-20 |
| Cross-check | https://unpkg.com/@ibm/plex-sans@1.1.0/fonts/split/woff2/<file> and https://unpkg.com/@ibm/plex-mono@1.1.0/fonts/split/woff2/<file> (all four identical, checked 2026-11-20) |
| Bytes | 78224 in all |
| License | SIL Open Font License 1.1, Reserved Font Name "Plex", nothing modified, in `LICENSE-ibm-plex` |

| File | Source URL | SHA-256 | Bytes | Served at |
|---|---|---|---|---|
| `IBMPlexSans-Regular-Latin1.woff2` | https://github.com/IBM/plex/raw/%40ibm%2Fplex-sans%401.1.0/packages/plex-sans/fonts/split/woff2/IBMPlexSans-Regular-Latin1.woff2 | `b5ad7bd39f996144915f0ad9849a90183b27d8c28ad97ed98af5b1bebc51f6b1` | 20984 | `/static/IBMPlexSans-Regular-Latin1.b5ad7bd39f996144.woff2` |
| `IBMPlexSans-SemiBold-Latin1.woff2` | https://github.com/IBM/plex/raw/%40ibm%2Fplex-sans%401.1.0/packages/plex-sans/fonts/split/woff2/IBMPlexSans-SemiBold-Latin1.woff2 | `fff0ab3a88b0b4aa0b693e4f0201359a15183b08e3fa5696d1918d8f0ade8ad5` | 22260 | `/static/IBMPlexSans-SemiBold-Latin1.fff0ab3a88b0b4aa.woff2` |
| `IBMPlexMono-Regular-Latin1.woff2` | https://github.com/IBM/plex/raw/%40ibm%2Fplex-mono%401.1.0/packages/plex-mono/fonts/split/woff2/IBMPlexMono-Regular-Latin1.woff2 | `10d3c7fa7eaf48e78db24f317b64f008a75e00f63a68bb3c2afc6ef51e58674f` | 17268 | `/static/IBMPlexMono-Regular-Latin1.10d3c7fa7eaf48e7.woff2` |
| `IBMPlexMono-SemiBold-Latin1.woff2` | https://github.com/IBM/plex/raw/%40ibm%2Fplex-mono%401.1.0/packages/plex-mono/fonts/split/woff2/IBMPlexMono-SemiBold-Latin1.woff2 | `1ce95cff1c5056cb0fed049c2912823293b158b816e193a6f937f2d92b1e0f39` | 17712 | `/static/IBMPlexMono-SemiBold-Latin1.1ce95cff1c5056cb.woff2` |

## License files

| File | Source URL | SHA-256 | Bytes | Served at |
|---|---|---|---|---|
| `LICENSE-htmx` | https://raw.githubusercontent.com/bigskysoftware/htmx/v2.0.11/LICENSE | `d3d2456f76414f2456104660ebd65aff1c04cd7966b942bdabd63f3cdb316a38` | 642 | not served |
| `LICENSE-ibm-plex` | https://github.com/IBM/plex/raw/%40ibm%2Fplex-sans%401.1.0/packages/plex-sans/LICENSE.txt (identical to plex-mono's and to the npm package's) | `7e6b2818edbd8f6a01ae80641cc8f16a51080d08fb4e532be3a0b6f74adb07da` | 4456 | `/static/LICENSE-ibm-plex.7e6b2818edbd8f6a.txt`, as plain text under the name `LICENSE-ibm-plex.txt` and linked from no page (decision 126: the Open Font License travels with every copy of the fonts) |
