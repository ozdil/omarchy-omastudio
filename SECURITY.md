# Security Policy

## Supported versions

Security fixes go into the latest release of OmaStudio. Older releases don't get updates; upgrade to the latest one.

| Version | Supported |
| :------ | :-------- |
| 3.x (latest release) | Yes |
| < 3.0 | No |

## Reporting a vulnerability

Please don't open a public issue for a security problem.

1. Report it privately through [GitHub's private vulnerability reporting](https://github.com/ozdil/omarchy-omastudio/security/advisories/new), or
2. email the maintainer at `ozan@ozanozdil.com`.

Include the OmaStudio version (`omastudio-engine --version`), what an attacker can do, and the steps or file that reproduce it. You'll get an answer within a few days. Once a fix is released, the advisory is published with credit to you unless you prefer otherwise.

The rules the code follows (subprocess limits, file permissions, IPC input checks) are described in [docs/development/security-standards.md](docs/development/security-standards.md).
