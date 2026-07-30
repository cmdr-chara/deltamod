# Security Policy

## Supported versions

Deltamod Community is currently distributed as a public beta. Security fixes
are applied to the newest published beta and to the current `DeltaMaster`
branch.

Official Deltamod and Deltamod Community are separate applications. Reports
about the upstream project should be sent to its maintainers unless the same
issue is reproducible in Community.

## Reporting a vulnerability

Do not include credentials, private game files, authentication cookies, or a
working exploit in a public issue.

Use GitHub's private vulnerability reporting for this repository when it is
available. Otherwise, contact the maintainer through the email address shown on
the maintainer's GitHub profile and include:

- the affected Deltamod Community version;
- Windows or Linux distribution and version;
- the feature and files involved;
- reproducible steps with sensitive values removed;
- the expected and observed result;
- any crash log or proof of impact that can be shared safely.

You should receive an acknowledgement within seven days. Please allow time for
validation and a coordinated fix before publishing technical details.

## Scope

Useful reports include:

- escaping approved installation, staging, archive, or patch paths;
- unsafe protocol or IPC handling;
- credential disclosure or insecure authentication persistence;
- arbitrary command execution;
- untrusted downloads bypassing host, redirect, type, or size validation;
- patch or import rollback failures that overwrite unrelated user data.

Mod compatibility problems, broken third-party downloads, and ordinary UI bugs
are not security vulnerabilities and should use the bug-report template.
