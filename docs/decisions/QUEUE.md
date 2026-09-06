# Work Queue

This file tracks active and accepted work.

## Active

- None.

## Ready

- None.

## Accepted

- `2026-09-06-update-tool-pulls-latest` — `update_*` tools now fetch and
  fast-forward the corpus clone before the commit check, via a shared
  `mcp_common::git` module, and report the outcome in a new `remote_sync`
  response field. Packet completed; **not yet deployed** — running containers
  hold the previous binary.

## Parking Lot

- None.
