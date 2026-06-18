# Security Policy

## Supported Versions

pkgtrace is currently in early development. Security updates are applied only to the latest release.

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |
| < 0.1   | :x:                |

As the project matures and stable releases are tagged, this table will be expanded accordingly.

---

## Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

If you discover a vulnerability in pkgtrace — especially one that could affect package removal logic, cache integrity, or arbitrary command execution via package names — please report it privately.

### How to Report

Open a [GitHub Security Advisory](https://github.com/oboobotenefiok/pkgtrace/security/advisories/new) on this repository. This keeps the disclosure private until a fix is ready.

If you are unable to use GitHub's advisory system, send a plain-text email describing the issue. Include:

- A clear description of the vulnerability
- Steps to reproduce it
- The version of pkgtrace affected
- Your assessment of the potential impact
- Any suggested fix, if you have one

### What to Expect

| Timeline | What happens |
| -------- | ------------ |
| Within 48 hours | You receive an acknowledgment that your report was received |
| Within 7 days | You receive an initial assessment — confirmed, needs more info, or not a vulnerability |
| Within 30 days | A fix is developed and a patched release is prepared (critical issues may be faster) |
| After the fix ships | You are credited in the CHANGELOG and release notes, unless you prefer to remain anonymous |

### If the Vulnerability Is Accepted

You will be kept in the loop throughout the fix process. We will coordinate a disclosure date with you before anything is made public. Credit will be given in the release notes.

### If the Vulnerability Is Declined

You will receive a clear explanation of why it was not considered a security issue. If you disagree with the assessment, you are welcome to discuss it further via the same private channel before going public.

---

## Scope

The following are considered in scope for security reports:

- Unsafe package removal that could destroy critical Termux packages without warning
- Cache poisoning that causes incorrect packages to be flagged for removal
- Path traversal or arbitrary file write via package names or export paths
- Command injection through unescaped package names passed to shell commands
- Privilege escalation or unexpected behavior on rooted devices

The following are **out of scope**:

- Bugs that only affect output formatting or display
- Issues in third-party tools that pkgtrace calls (`pkg`, `pip`, `cargo`, etc.)
- Feature requests or general usability concerns (open a regular issue for those)

---

## Philosophy

pkgtrace operates on your installed packages and can remove software from your device. We take that responsibility seriously. Even a single incorrect autoremove decision can break a Termux environment. Security and correctness are treated as the same priority here.
