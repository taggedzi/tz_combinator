# Security Policy

Thank you for helping protect `tz_combinator` and its users. Please report
security vulnerabilities privately so they can be investigated before public
disclosure.

## Supported versions

This is a pre-1.0 project maintained by one unpaid developer.

| Version | Support |
|---|---|
| Latest release | Best-effort security fixes |
| `master` | Development branch; fixes may land here first |
| Older releases | Not actively supported |

Users should update to the latest release. Backports may not be available.

## Report a vulnerability

Use
[GitHub's private vulnerability reporting](https://github.com/taggedzi/tz_combinator/security/advisories/new)
when it is available. Do not open a public issue containing exploit details,
secrets, sensitive paths, or information that would put users at risk.

If private reporting is unavailable, open a minimal
[public issue](https://github.com/taggedzi/tz_combinator/issues/new) stating
only that you need a private channel for a security report. Do not include the
vulnerability details.

Please include what you know:

- the affected version, commit, interface, and platform;
- the type and potential impact of the vulnerability;
- the conditions needed to trigger it;
- minimal reproduction steps or a proof of concept;
- whether the issue is already public or known to others; and
- any suggested mitigation.

Reports may be incomplete. A clear summary is enough to begin. Do not include
unrelated personal information or real secrets; use unmistakably fake test
values.

## Response expectations

There is no fixed response or remediation service-level agreement. The
maintainer has limited capacity, disabilities, and responsibilities outside
this unpaid project. Reports are prioritized by severity, exploitability,
affected users, and the availability of a safe fix.

If you receive no acknowledgement after 14 days, one follow-up is welcome.
Silence does not mean the report is unimportant.

When capacity permits, the maintainer will:

1. confirm receipt and ask for missing information;
2. assess severity and affected versions;
3. coordinate a fix, tests, release, and disclosure as appropriate; and
4. credit the reporter if requested and safe to do so.

Some reports may be closed when they describe intended behavior, require
trusted local access without crossing a security boundary, or cannot be
reproduced. The reasoning will be explained when practical.

## Safe research and disclosure

Please:

- test only systems and data you own or are authorized to use;
- avoid privacy violations, service disruption, destructive testing, and
  unnecessary data access;
- stop after demonstrating the issue with the minimum access required;
- give the project a reasonable opportunity to investigate before disclosure;
  and
- coordinate public disclosure when doing so would reduce risk to users.

This project will not pursue action against good-faith research that follows
these guidelines. This statement does not authorize testing of third-party
systems or override applicable law.

## Security model

The project processes potentially hostile inputs and filesystem paths. Its
current guarantees and deployment limitations are documented in
[docs/security-and-deployment.md](docs/security-and-deployment.md).
