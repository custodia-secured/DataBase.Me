# Security

## Reporting a vulnerability

Please report security problems **privately**, not in a public issue. Use GitHub's private
vulnerability reporting: on the repository's **Security** tab, choose **Report a vulnerability**.

Tell us what you found, how to reproduce it, and what an attacker could do with it. We are
volunteers, but we aim to acknowledge a report within 7 days and to agree a disclosure date with
you. We will credit you unless you prefer not to be named.

## Supported versions

DataBase.Me is pre-release. Only the latest commit on `main` is supported. Once releases begin, the
latest release will be supported.

## Threat model (summary)

DataBase.Me is meant to hold a lifetime of personal information, so the design assumes the
following threats. The milestone in brackets is when each protection is built.

| Threat                                                      | Protection                                                                                                                                                                                                                |
| ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A lost or stolen laptop                                     | Optional encryption at rest: SQLCipher-compatible database encryption and per-file XChaCha20-Poly1305 for blobs, keyed from a passphrase via Argon2id, with a printable recovery kit [M9]                                 |
| A malicious or compromised plugin                           | Plugins run as WebAssembly with no ambient access; the user grants each capability; network hosts are always an explicit list; UI plugins live in sandboxed iframes with a strict CSP and never touch the app's DOM [M10] |
| Malicious files (zip bombs, malformed PDFs, path traversal) | Importers run with size and time limits and never write outside the vault; a fixture suite of hostile files runs in CI [M6]                                                                                               |
| Data sent to an AI provider without consent                 | AI is off by default; every call passes a per-space policy gate with sensitive-field redaction and is written to an audit log [M11]                                                                                       |
| A compromised sync relay or cloud drive                     | Sync is end-to-end encrypted; the relay sees only ciphertext and sizes; no hosted relay exists [M13]                                                                                                                      |
| A shared-space member overreaching                          | Roles per space and signed operations [M15]                                                                                                                                                                               |
| Losing data to the app itself                               | Append-only operation log, conflict copies instead of overwrites, Trash instead of deletion, a bulk-change guard on the files folder [M1, M5]                                                                             |
| Format rot and key loss                                     | Open, documented formats (FORMAT.md), plain-text export, recovery kit [M5, M7, M9]                                                                                                                                        |
| Network exposure                                            | The app makes no network request unless the user turns on a feature that needs one, and CI tests this [M7]; the web UI runs under a strict Content Security Policy with no remote content [M0]                            |

What DataBase.Me cannot protect against: malware running as you on an unlocked machine, and a
plain-text files folder that you choose to keep next to an encrypted vault (the app warns you when
you turn that on).

Secrets such as API keys are kept in the operating system's keychain, never in the vault or in
exports.
