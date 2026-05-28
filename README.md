# MCP Servers

Rust workspace for Model Context Protocol (MCP) servers powering the AI Workbench.
The goal is to improve client productivity by giving agents direct access to trusted best-practice knowledge.

- **Parent entity:** Plainsight Systems LLC — parent-org infrastructure (no operating brand).
- **Maturity:** active development.
- **Governance:** built to the [Plainsight Systems engineering philosophy](https://github.com/plainsight-systems/.github/blob/main/engineering_philosophies.md). To contribute, see [`CONTRIBUTING.md`](CONTRIBUTING.md).

## Workspace Structure

```
crates/
  mcp-common/       Shared library (Redis, LanceDB, serialization utilities)
  cpp-guidelines/   C++ Core Guidelines MCP server
  cpp-perf-guidelines/ Low-level C++ Performance Guidelines MCP server
  rust-api-guidelines/ Rust API Guidelines MCP server
  llm-proxy/        Local OpenAI-compatible proxy MCP server
  nodejs-guidelines/ Node.js Best Practices MCP server
data/                Local data directory (not committed)
  cpp-guidelines/    Cloned C++ Core Guidelines repository
  cpp-perf-guidelines/ Cloned C++ Performance Guidelines corpus repository
  rust-api-guidelines/ Cloned rust-lang/api-guidelines repository
  nodejs-guidelines/  Cloned nodebestpractices repository
  lancedb/           LanceDB vector database files
  redis/             Redis persistence (AOF/RDB)
```

## Prerequisites

- Rust (stable toolchain)
- Protocol Buffers compiler (`brew install protobuf` on macOS)
- Docker and Docker Compose (for Redis)

## Setup

1. Clone the repository and create your local environment file:

```sh
cp .env.example .env
```

2. Create the data directories and start infrastructure:

```sh
mkdir -p data/lancedb data/redis
docker compose up -d
```

To start only Redis (without running all servers):

```sh
docker compose up -d redis
```

3. Clone the guideline repositories into the data directory:

```sh
git clone https://github.com/isocpp/CppCoreGuidelines.git data/cpp-guidelines
git clone https://github.com/rust-lang/api-guidelines.git data/rust-api-guidelines
git clone https://github.com/goldbergyoni/nodebestpractices.git data/nodejs-guidelines
```

The `cpp-perf-guidelines` corpus is a self-authored companion repository (not a
third-party upstream). Clone it into the data directory as well:

```sh
git clone https://github.com/plainsight-systems/cpp-perf-guidelines.git data/cpp-perf-guidelines
```

If a target directory already exists and is not empty, remove it first or update it in place:

```sh
rm -rf data/cpp-guidelines
git clone https://github.com/isocpp/CppCoreGuidelines.git data/cpp-guidelines
# OR, if it is already a clone:
git -C data/cpp-guidelines pull --ff-only

rm -rf data/rust-api-guidelines
git clone https://github.com/rust-lang/api-guidelines.git data/rust-api-guidelines
# OR, if it is already a clone:
git -C data/rust-api-guidelines pull --ff-only
```

4. Build the workspace:

```sh
cargo build
```

## Development Workflow

- `cargo check` -- type-check the full workspace
- `cargo build` -- build all crates
- `cargo test` -- run all tests
- `cargo run -p cpp-guidelines` -- run the C++ Guidelines MCP server
- `cargo run -p cpp-perf-guidelines` -- run the C++ Performance Guidelines MCP server
- `cargo run -p rust-api-guidelines` -- run the Rust API Guidelines MCP server
- `cargo run -p llm-proxy` -- run the local LLM proxy MCP server
- `cargo run -p nodejs-guidelines` -- run the Node.js Best Practices MCP server
- `docker compose up -d redis` -- start Redis only
- `docker compose down` -- stop Redis

## Docker Compose (All Services)

To build and run Redis plus all MCP servers in containers:

```sh
docker compose up --build
```

The MCP servers listen over TCP inside Docker when `MCP_TCP_LISTEN_ADDR` is set:

- `cpp-guidelines`: `localhost:7011`
- `rust-api-guidelines`: `localhost:7012`
- `nodejs-guidelines`: `localhost:7013`
- `llm-proxy`: `localhost:7014`
- `cpp-perf-guidelines`: `localhost:7015`

All services are attached to a shared Docker network named `mcp` so they can reach Redis at `redis:6379`.

## Rust API Guidelines MCP Tools

The `rust-api-guidelines` server exposes the following MCP tools.

- `search_guidelines`
  - Input: `{ "query": string, "limit"?: number }` (`limit` defaults to 10, max 50)
  - Output: JSON object `{ results: [{ id, title, category, score, summary }] }`
- `get_guideline`
  - Input: `{ "guideline_id": string }` (for example `C-CASE`)
  - Output: JSON object `{ id, anchor, title, category, source_file, raw_markdown }`
- `list_category`
  - Input: `{ "category": string }` (for example `Naming`, `Documentation`)
  - Output: JSON object `{ category: { key, display_name, guideline_count }, guidelines: [{ id, title }] }`
- `update_guidelines`
  - Input: none
  - Output: JSON object `{ updated, commit, guideline_count }`

## LLM Proxy MCP Tools

The `llm-proxy` server exposes tools for a coordinator model to discover available local models
and delegate requests to them via an OpenAI-compatible API host.

- `list_models`
  - Input: none
  - Output: JSON object `{ object?, data: [{ id, object?, created?, owned_by? }] }`
- `ask_model`
  - Input: `{ "model": string, "prompt": string }`
  - Output: JSON object `{ text: string }`
- `chat_model`
  - Input: `{ "model": string, "messages": [{ "role": string, "content": string }] }`
  - Output: JSON object `{ text: string }`
- `generate_code`
  - Input: `{ "model": string, "language": string, "specification": string }`
  - Output: JSON object `{ text: string }` (typically code-only)
- `start_conversation`
  - Input: none
  - Output: JSON object `{ conversation_id: string }`
- `continue_conversation`
  - Input: `{ "conversation_id": string, "model": string, "prompt": string }`
  - Output: JSON object `{ text: string }`
- `end_conversation`
  - Input: `{ "conversation_id": string }`
  - Output: JSON object `{ ok: bool }`
- `get_usage_stats`
  - Input: none
  - Output: JSON object `{ redis_available: bool, models: [{ model, requests, total_tokens?, token_counted_requests, token_unknown_requests }] }`

## Node.js Best Practices MCP Tools

The `nodejs-guidelines` server exposes the following MCP tools.

- `search_guidelines`
  - Input: `{ "query": string, "limit"?: number }` (`limit` defaults to 10, max 50)
  - Output: JSON object `{ results: [{ id, title, category, score, summary }] }`
- `get_guideline`
  - Input: `{ "guideline_id": string }` (for example `1.1`)
  - Output: JSON object `{ id, anchor, title, category, source_file, raw_markdown }`
- `list_category`
  - Input: `{ "category": string }` (for example `1`, `2`, `3`)
  - Output: JSON object `{ category: { key, display_name, guideline_count }, guidelines: [{ id, title }] }`
- `update_guidelines`
  - Input: none
  - Output: JSON object `{ updated, commit, guideline_count }`

## C++ Performance Guidelines MCP Tools

The `cpp-perf-guidelines` server exposes a corpus of low-level C++ performance
guidelines (custom allocators, data layout and cache behavior, copy/move
discipline, object lifetime, embedded constraints, concurrency memory effects,
codegen, SIMD/vectorization, and telemetry/observability harnesses) — the
technique layer below the ISO C++ Core Guidelines.

- `search_guidelines`
  - Input: `{ "query": string, "limit"?: number }` (`limit` defaults to 10, max 50)
  - Output: JSON object `{ results: [{ id, title, category, score, summary }] }`
- `get_guideline`
  - Input: `{ "guideline_id": string }` (for example `MEM.1`, `CACHE.1`)
  - Output: JSON object `{ id, anchor, title, category, raw_markdown, sections, source_file }`
- `list_category`
  - Input: `{ "category": string }` (for example `memory`, `cache-layout`, `codegen`, `simd`, `telemetry`)
  - Output: JSON object `{ category: { key, display_name, guideline_count }, guidelines: [{ id, title }] }`
- `update_guidelines`
  - Input: none
  - Output: JSON object `{ updated, commit, guideline_count }`

## License

This repository is dual-licensed, following the Plainsight Systems licensing
doctrine:

- **Code** — the Rust crates and any scripts or tooling — is licensed under the
  [Apache License 2.0](LICENSE-APACHE).
- **Content** — documentation and written copy — is licensed under
  [Creative Commons Attribution 4.0 International](LICENSE-CC-BY) (CC BY 4.0).
