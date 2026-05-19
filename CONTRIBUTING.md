# Contributing

Thanks for considering a contribution to `mcp-servers`.

## Engineering philosophy

This project is built to the Plainsight Systems engineering philosophy. Please
read it before contributing:
<https://github.com/plainsight-systems/.github/blob/main/engineering_philosophies.md>

## Workflow

- Non-trivial changes start with a packet in `docs/decisions/packets/`; see
  `docs/decisions/workflow.md`.
- One change, one intent — do not bundle unrelated changes.
- Non-trivial changes require deterministic verification (tests), not just a
  green build.

## Pull requests

- Keep the change focused and reviewable.
- Run `cargo check` and `cargo test` before opening a pull request.
- State what changes, why, and how it was verified.

## Licensing

By contributing, you agree your contributions are licensed under this
repository's terms: code under Apache-2.0, documentation and content under
CC BY 4.0. See `LICENSE-APACHE` and `LICENSE-CC-BY`.

## Security

Do not report security vulnerabilities in public issues. See `SECURITY.md`.

## Code of conduct

Participation in this project is governed by `CODE_OF_CONDUCT.md`.
