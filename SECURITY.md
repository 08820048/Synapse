# Security policy

## Supported versions

Synapse is in public preview. Security fixes are applied on `main`. There is no long-term support branch yet.

## What this project trusts

- Local Markdown files and folders chosen by the user
- The operating system trash, file dialogs, and clipboard
- Optional network requests only for HTTP(S) images and bookmark metadata

The vault is the security boundary for local paths. Relative paths, percent-encoded paths, and symbolic links must not escape the current vault.

## Reporting a vulnerability

Please **do not** open a public GitHub issue for a security problem.

Use one of these private channels:

- GitHub Security Advisories: https://github.com/08820048/Synapse/security/advisories/new
- Email: ilikexff@gmail.com

Include the affected version or commit, a reproduction, and the impact (for example path escape, unexpected network access, or data loss).

You should get an acknowledgement within 7 days. Please give us time to patch before publishing details.
