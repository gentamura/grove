# Licensing and Distribution

This document records Grove's licensing decision and the checks required before
publishing source code or distributing compiled applications.

It is an engineering compliance checklist, not legal advice.

## Grove license

Grove's original source code and documentation are released under the MIT
License, except for the Grove name and tree icon:

- `Cargo.toml` declares `license = "MIT"`.
- The repository root contains the complete MIT text in `LICENSE`.
- The copyright holder is identified as Gen Tamura.
- `assets/icon.svg` and `assets/icon.icns` are explicitly excluded from MIT and
  are governed by `TRADEMARKS.md` and `assets/README.md`.

The MIT License does not require registration with a government, standards
body, or GitHub. Publishing the repository with the license notice grants the
permissions described by the license.

The brand policy permits the Grove Marks to be used with unmodified official
source and releases, including qualifying package-manager builds. Modified
versions and forks remain free to use the MIT-licensed code but must adopt a
different product name and icon.

## Current dependency review

The dependency graph was reviewed on July 30, 2026, using the locked
dependencies in `Cargo.lock` for the `aarch64-apple-darwin` target, including
normal and build dependencies.

Findings:

- GPUI 0.2.2 and its GPUI component crates are Apache-2.0.
- Grove's other direct dependencies use permissive MIT, Apache-2.0, Unlicense,
  or compatible dual-license expressions.
- No GPL-only, AGPL-only, or LGPL-only dependency was found in the active macOS
  normal/build graph.
- `option-ext` 0.2.0 is MPL-2.0 and is present through `dirs`/`dirs-sys`.
- `cbindgen` 0.28.0 is MPL-2.0 and is used only as a GPUI build dependency; it
  is not expected to be part of the resulting Grove executable.

MPL-2.0 is file-level copyleft. Its presence does not require Grove's original
source files to be relicensed, but an executable distributor must preserve the
recipient's rights and provide an effective way to obtain the covered source.

This review is a snapshot, not a permanent approval. Repeat it whenever
`Cargo.lock`, target platforms, enabled features, or packaging contents change.

## Publishing source code

Before making the repository public:

- [x] Add a complete MIT `LICENSE`.
- [x] Keep `Cargo.toml` license metadata consistent with `LICENSE`.
- [x] Document the project and its local-data behavior in `README.md`.
- [x] Confirm generated output such as `target/` is ignored.
- [ ] Review the full Git history for credentials, tokens, private session
  transcripts, personal data, and accidentally committed build artifacts.
- [x] Document the Grove name and icon as separately copyrighted brand assets
  that may be published under the conditions in `TRADEMARKS.md`.
- [ ] Commit and push all intended documentation.
- [ ] Recheck the GitHub repository contents and Actions history before changing
  visibility.

The current repository does not vendor third-party crate source code. Merely
declaring crates in `Cargo.toml` and pinning them in `Cargo.lock` does not copy
their source into this repository.

## Distributing a binary

These requirements become relevant when a person or organization gives a
compiled Grove application to someone else. A local build used only by the
builder is not a binary distribution.

Before Grove publishes an official `.app`, `.dmg`, archive, Homebrew bottle, or
other compiled package:

- [ ] Generate a complete third-party license report from the exact release
  dependency graph, preferably with `cargo-about`.
- [ ] Review every generated license choice rather than accepting all
  `OR` expressions automatically.
- [ ] Create a `THIRD_PARTY_NOTICES` file containing dependency names, versions,
  license identifiers, copyright/attribution notices, and source locations.
- [ ] Include Grove's `LICENSE`, required third-party license texts, and
  `THIRD_PARTY_NOTICES` in `Grove.app/Contents/Resources`.
- [ ] Include the applicable Grove brand notice and preserve the icon's
  copyright metadata in official packages.
- [ ] Check every Apache-2.0 dependency for a `NOTICE` file and preserve any
  applicable attribution notices.
- [ ] Provide an effective source-code location for MPL-2.0 components included
  in the executable, including `option-ext`.
- [ ] Make the notices discoverable from release documentation or an in-app
  “Open Source Licenses” view.
- [ ] Add CI that rejects unknown, unreviewed, or incompatible dependency
  licenses.
- [ ] Repeat the audit for each supported target because target-specific
  dependencies differ.

Whoever distributes a compiled package is responsible for that distribution's
license compliance. This may be Grove's maintainer, a fork owner, a package
manager, a company, or another downstream distributor.

## References

- [MIT License — Open Source Initiative](https://opensource.org/license/mit)
- [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0.html)
- [MPL 2.0 FAQ — Mozilla](https://www.mozilla.org/en-US/MPL/2.0/FAQ/)
- [GPUI license metadata](https://github.com/zed-industries/zed/blob/main/crates/gpui/Cargo.toml)
- [cargo-about](https://github.com/EmbarkStudios/cargo-about)
