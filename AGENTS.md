# AGENTS.md — instructions for AI coding agents

## Prime directives

1. **Clean-room only.** Never copy Mojang bytecode, decompiled Java, mappings text,
   or assets into the repo, issues, PRs, or commit messages. Reference by
   URL + version + symbol name only (e.g. `net.minecraft.client.main.Main @ 26.2`).
2. **YAGNI / KISS / DRY.** Smallest change that satisfies the spec + test. No speculative
   modding API, no extra backends, no abstraction before second use.
3. **Docs first.** Before code, update/consult `docs/AI-GUIDE.md` + relevant `docs/*.md`.
   If spec is missing, add a `SPEC` note + test, don't guess physics/constants.
4. **Optimization with evidence.** No premature optimization. Benchmark with `criterion`
   or `tracy` before/after. Prefer algorithmic wins (meshing, culling, caching).
5. **Cross-platform.** No `std::os::windows`-only or unix-only code without `cfg` gate
   + CI coverage. Test on Linux/macOS/Windows via CI matrix.
6. **Latest stable.** Prefer latest stable crates. Run `cargo update` deliberately,
   record in PR. No nightly features.

## Commands (must pass before push)

```sh
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo deny check
cargo audit
```

## Crate boundaries

- `mc-protocol`: pure serialization (VarInt, NBT, packets 776). No rendering, no I/O beyond `tokio::io`.
- `mc-auth`: OAuth + token cache. Never logs secrets. Uses OS keychain or encrypted file with 0600.
- `mc-world`: data structures only. No `wgpu`, no network.
- `mc-render`: `wgpu` only. No game logic; takes mesh/command structs from `mc-client`.
- `mc-client`: orchestrates tick (20 TPS) + render interpolation. Owns timing.
- `mc-launcher`: resolves piston-meta, verifies SHA1, launches. Never vendors jars.

Max file ~400 lines. Split by responsibility, not by layer bingo.

## Commit/PR rules (mandatory for humans and AI agents)

- Divide work into reviewable sections; **commit every section separately**.
  One section = one concern (e.g. tooling, docs, one crate, one feature).
- Conventional commit names: `feat:`, `fix:`, `docs:`, `chore:`, `chore(deps):`,
  with scope where useful (e.g. `feat(protocol): ...`). Imperative mood,
  ≤72-char subject, body explains *why* when non-obvious.
- Each commit must pass the Commands above on its own (at minimum fmt+clippy+test).
- **Never attribute yourself in commits.** No `Co-authored-by`, `Assisted-by`,
  `Generated-by`, `Helpful-`, collaborator, or helper trailers, in messages or PRs.
  The commit author is the human owner; agents stay invisible in history.
- PR must list: spec reference (docs path + protocol version), tests added, perf impact, platforms checked.
- CI (`.github/workflows/ci.yml`) is required on `main`. Do not bypass, amend pushed
  commits, or use `--no-verify`.
