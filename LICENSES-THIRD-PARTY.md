# Third-party licences

Baobox ships under MIT. This is the audit that says it can.

Automated inventory of every crate in `Cargo.lock` lives in
[`THIRD-PARTY-LICENSES.csv`](THIRD-PARTY-LICENSES.csv). What follows is the part
that needed a human to look at it.

## Summary

**632 dependencies. No copyleft obligations that reach Baobox's own code.**

| Licence | Count |
|---|---|
| MIT / Apache-2.0 (either) | 351 |
| MIT | 146 |
| Zlib / Apache-2.0 / MIT | 18 |
| Unicode-3.0 | 18 |
| BSD-2-Clause / BSD-3-Clause | 12 |
| MPL-2.0 | 5 |
| Other permissive combinations | ~49 |

## Things worth explaining

**`r-efi` — `MIT OR Apache-2.0 OR LGPL-2.1-or-later`**
An automated scan flags this because of the LGPL term, but the licence is
disjunctive: any one of the three may be chosen. Baobox takes MIT. No LGPL
obligation attaches.

**MPL-2.0 — `cssparser`, `cssparser-macros`, `dtoa-short`, `option-ext`, `selectors`**
Pulled in transitively by the webview stack. MPL-2.0 is copyleft at file
granularity: linking against it imposes nothing, but modified MPL files must be
published under MPL. Baobox does not modify any of them, so nothing follows.
If that ever changes, the changed files have to go back out under MPL-2.0.

**`mozjpeg` — IJG**
The Independent JPEG Group licence. Permissive; requires acknowledging that the
software is based in part on IJG's work. Satisfied by this file.

**`blake3` — `CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception`**
CC0 is a public-domain dedication. Nothing required.

**`opencc-fmmseg` — MIT, bundling the OpenCC dictionaries (Apache-2.0)**
Simplified/Traditional conversion. The crate is MIT; the phrase tables it
embeds come from OpenCC and are Apache-2.0. Both permissive.

## Deliberately avoided

Ghostscript is the usual answer for PDF compression and is **AGPL-3.0**. Shipping
it alongside Baobox would push the whole project to AGPL. PDF compression is
implemented directly against `lopdf` and `mozjpeg` instead — a slower path to
build, but it keeps the licence honest.

The same reasoning ruled out bundling LibreOffice for Office conversion (LGPL,
and 400 MB besides).

`zhconv` was the first choice for Simplified/Traditional conversion and was
removed once the audit caught it: it is **GPL-2.0-or-later**, with no permissive
alternative in its licence expression, so it would have pulled the whole project
into GPL. `opencc-fmmseg` (MIT) does the same job on the same class of tables.
This is exactly what the audit exists to catch.

## Regenerating

```powershell
# reads the license field of every crate from the local registry cache
pwsh scripts/audit-licenses.ps1
```

Worth re-running whenever a dependency is added. A single viral dependency is
enough to invalidate the licence the project ships under, and it is far cheaper
to catch at the point of adding it than after release.
