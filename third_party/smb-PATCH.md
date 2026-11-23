# Patched `smb` crate

`third_party/smb` is the published [`smb` 0.12.1](https://crates.io/crates/smb/0.12.1) crate
with one change in its `Cargo.toml`: `sspi` is raised from `=0.21.3` to `=0.23.1`.

**Added:** `LICENSE.md`, the MIT notice from the upstream repository at commit
`7c7d4a60c930`, from which 0.12.1 was published. The crate declares MIT but does not contain the
file (issue #163). `about.toml` takes the same notice for the other `smb-*` crates.

**Why:** `sspi` 0.21.3 pins pre-release RustCrypto crates (`curve25519-dalek = 5.0.0-rc.1`),
which conflict with `russh` 0.65 (SFTP). Older `russh` releases that would fit have known
vulnerabilities. `sspi` 0.23.1 uses the final releases. The same change is proposed upstream as
[afiffon/smb-rs#189](https://github.com/afiffon/smb-rs/pull/189).

**Checked:** `tools/check-smb-patch.sh` (run in CI) downloads the crate from crates.io, verifies
its SHA-256 and confirms that nothing else differs. The SMB conformance suite passes with it.

**Remove** this folder, the `[patch.crates-io]` entry in the root `Cargo.toml` and the check
script once `smb` releases with `sspi` 0.23 or newer.
