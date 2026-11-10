# Contributing to Huehuecoyotl

Thanks for helping. The architecture is described in [docs/SPEC.md](docs/SPEC.md); please read
the principles in section 2 before proposing larger changes.

## Sign-off (DCO)

Every commit must carry a `Signed-off-by` line, certifying the
[Developer Certificate of Origin 1.1](https://developercertificate.org/): that you wrote the
change or otherwise have the right to submit it under the project's licence (Apache-2.0).
Add it with:

```bash
git commit -s
```

CI rejects pull requests with unsigned commits. There is no CLA.

## Workflow

- Everything goes through pull requests with CI passing. Larger changes (architecture, or a
  dependency under a licence not already in `deny.toml`) start as a short proposal in an issue.
- Add a line to `CHANGELOG.md` under "Unreleased" in the same pull request.
- Rust: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- C++/QML: `clang-format`, `qmllint` (runs in the build). Keep the C++ layer free of business
  logic and I/O (SPEC section 2, principle 3).
- No GPL dependencies, and only the Qt modules listed in `cmake/QtModuleAllowlist.cmake`.

## Building

See [README.md](README.md#building).
