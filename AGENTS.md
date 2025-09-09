# AGENTS.md

This file provides guidance to agents when working with code in this repository.

- Config selection: the CLI uses config.production.toml by default and only falls back to config.toml when the default path was used and config.production.toml is missing ([src/main.rs](src/main.rs)).
- At startup, main ensures config.llm_logging.log_dir exists; missing directories are created ([src/main.rs](src/main.rs)).
- Workspace bootstrap: a Git repo is always initialized in agent.working_dir and an initial commit is auto-created if the repo has no commits to guarantee HEAD exists ([src/core/agent.rs](src/core/agent.rs)).
- Set BORG_USE_MOCK_LLM=true to replace all configured providers with a single mock “default” provider (overrides real API keys) ([src/main.rs](src/main.rs)).
- Ask LLM selection priority: llm.default → llm.code_generation → first available llm entry ([src/main.rs](src/main.rs)).
- Planning LLM fallback uses PLANNING_API_KEY, else OPENAI_API_KEY when no planning entry exists in config ([src/core/planning.rs](src/core/planning.rs)).
- Codegen tools protocol: models must emit JSON lines like {"tool":"Name","args":[...]} and will iterate up to code_generation.max_tool_iterations (default 25) before generating final edits ([src/code_generation/llm_generator.rs](src/code_generation/llm_generator.rs)).
- File targeting: if a code block lacks a nearby file indicator (“file:”/“filename:”/“for file …”), the change defaults to src/main.rs (be explicit to avoid accidental writes) ([src/code_generation/llm_generator.rs](src/code_generation/llm_generator.rs)).
- CodeImprovement parsing also expects “// File: path/to.rs” inside code fences; without it, a single default target is used and may be wrong ([src/core/strategies/code_improvement.rs](src/core/strategies/code_improvement.rs)).
- Commit and merge messages are LLM-generated; the mainline branch is chosen dynamically (master if present, else main). Conflicts abort with no auto-resolution ([src/core/strategies/code_improvement.rs](src/core/strategies/code_improvement.rs)).
- Current plan execution gap: CodeImprovement’s plan does not execute apply/test steps by default; merges can occur without actual applied changes if not handled explicitly ([src/core/strategies/code_improvement.rs](src/core/strategies/code_improvement.rs)).
- Test runners parse “test result:” summaries; rustfmt/clippy are optional and skipped if not installed; coverage/linting defaults are stubbed unless overridden ([src/testing/comprehensive.rs](src/testing/comprehensive.rs), [src/testing/test_runner.rs](src/testing/test_runner.rs)).
- Integration tests generate per-test configs with mock providers and temp workspaces and rely on env!("CARGO_BIN_EXE_borg"); prefer BORG_USE_MOCK_LLM to avoid external API dependencies ([tests/cli_test.rs](tests/cli_test.rs)).
- Quality gate (non-default): `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings -W clippy::cognitive_complexity && cargo test --all --locked && cargo audit --deny warnings && cargo build --release --locked --all-features` ([.roo/rules/linting.md](.roo/rules/linting.md)).
- The borg git subcommand bypasses full Agent/Strategy initialization and invokes the LLM code generator directly ([src/main.rs](src/main.rs)).