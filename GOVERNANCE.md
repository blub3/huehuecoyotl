# Governance

- **Decisions.** The founder, Björn Blankenhagen, is currently the sole maintainer and has the
  final say on all decisions.
- **Maintainers.** Contributors are invited to become maintainers after sustained, good
  contributions. Maintainers have merge rights and are listed in `MAINTAINERS`.
- **Changes.** All changes go through pull requests with CI passing. Once there are two
  maintainers, each change needs a review from the other. Larger changes (architecture,
  dependencies with a new licence) start as a written proposal in an issue.
- **Releases.** Only maintainers create release tags. A signed tag on `main` triggers CI, which
  builds, signs and publishes every package. While CI is unavailable, a maintainer may build the
  Windows packages from that tag on a dedicated Windows build VM. The tests must pass there, and
  the release notes must say the packages were built on the VM and include their checksums.
  Nothing else is built on a personal machine. Signing keys live only in CI secrets, never on the
  VM, and all maintainer accounts use two-factor authentication.
- **Versions.** Semantic versioning, with a changelog updated in each pull request.
- **Name and accounts.** The project name, domain and store accounts are held by the founder.
  Who else has access is recorded privately by the maintainers, so the project survives one
  person becoming unavailable. If the project grows, these can move to a fiscal host or
  foundation.
- **Conduct.** See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
