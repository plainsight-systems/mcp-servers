# Project Memory

This file is the canonical entry point for durable project context.

## Product Identity

- **Product:** MCP Servers
- **Operating brand:** None — parent-org infrastructure
- **Parent entity:** Plainsight Systems LLC
- **Repository:** mcp-servers

## Engineering Philosophy

This project is built to the Plainsight Systems engineering philosophy:
<https://github.com/plainsight-systems/.github/blob/main/engineering_philosophies.md>

## Locked Decisions

- Governance adopted 2026-05-19. Decisions predating this date are recorded in
  git history rather than here.

## Research Index

- None yet.

## Locked Decisions Added 2026-09-06

- **`update_*` fetches before it reports.** The tools previously re-indexed
  whatever was on disk and never contacted the remote, so a server could serve
  a stale corpus while returning success. Fetch-then-check is the contract, and
  the response carries `remote_sync` so "already current" is distinguishable
  from "never looked". Fast-forward only: a diverged, detached or upstream-less
  clone is reported, never rewritten.

## Active Workflow Pointers

- Queue: `QUEUE.md`
- Packets: `packets/`
- Workflow: `workflow.md`
