# Pumpkin singleplayer integration — protocol 776

Decision (2026-09-06): prioritize the Rust client. Pumpkin supplies local world
simulation as a separate executable; do not implement or fork a server here.
Java is not required for this path. First gameplay target: move, break blocks,
place blocks, save/reload. True singleplayer pause is deferred. Performance must
be measured, not inferred from language choice.

## External interface

Pin Pumpkin `0.1.0-dev+26.2-26.45` (upstream commit `8d0d0d3`).
References: [release](https://github.com/Pumpkin-MC/Pumpkin/releases/tag/0.1.0-dev%2B26.2-26.45),
[configuration](https://github.com/Pumpkin-MC/Pumpkin/blob/8d0d0d3/crates/pumpkin-config/src/lib.rs),
[lifecycle](https://github.com/Pumpkin-MC/Pumpkin/blob/8d0d0d3/crates/pumpkin/src/lib.rs).

The launcher verifies an explicitly supplied executable against the release's
SHA-256 digest, creates a managed session directory, writes local-only settings,
and owns the child process. The client probes readiness with its existing
Handshake/Status/Ping path and checks protocol 776. All world data stays under
the session directory outside a Git checkout. A session lock prevents concurrent
managed launches. An existing unmanaged directory is never adopted.

SPEC: settings are launcher-owned and regenerated at every launch. The world
seed is fixed for this development milestone; Creative is the initial game mode,
with view/simulation distances of 4 chunks. Bind Java TCP to 127.0.0.1 only;
disable Bedrock, query, RCON, proxy, LAN broadcast, and plugins. Local development
uses offline mode without encryption; account authentication remains a later phase.
Normal remote-server authentication policy is unchanged.

SPEC: startup has a deadline and checks for early child exit. Shutdown sends
`stop\n` through piped stdin and waits for process exit, allowing Pumpkin to save.
If shutdown exceeds its deadline, kill and reap the child and report failure;
never report a forced stop as a successful save. Dropping the process owner is
emergency cleanup only, not a save operation. Logs go to the managed directory,
not unbounded in-memory pipes. Managed files must not be symlinks.

## Development command

`mc-client local --pumpkin /absolute/path/to/pumpkin --session /outside/repo/session`
starts the server, verifies protocol/status, and keeps it running until Ctrl-C.
`--check` stops after readiness, for smoke testing. This is a headless integration
command; playable joining/rendering is subsequent client work.

Download the executable for your platform from the pinned release above, outside
this repository. On macOS/Linux make the downloaded file executable with
`chmod u+x /path/to/pumpkin`. The launcher rejects other versions or modified
binaries. Supported release assets: macOS ARM64, Linux x86_64/ARM64 (glibc),
Windows x86_64/ARM64. Intel macOS is not supplied by this release.

For a repeatable real-server smoke test:

```sh
PUMPKIN_BIN=/absolute/path/to/pumpkin cargo test -p mc-client --test pumpkin -- --ignored
```

The test creates and removes its own temporary world outside the repository.
On PowerShell set `$env:PUMPKIN_BIN` before running the cargo command.
Console logs are replaced on each launch; Pumpkin also manages its own log files.
Use the same `--session` path to reopen its saved world. The session parent
directory must exist. Managed settings are intentionally fixed for development.

Use new test worlds. Importing valuable vanilla saves is outside this milestone:
[upstream saving report](https://github.com/Pumpkin-MC/Pumpkin/issues/2626).
The executable and its assets are not vendored or linked into the Rust workspace.
Pumpkin's GPLv3 and separate asset terms remain upstream's; this repository only
stores configuration/interface code and release metadata.

## Acceptance and next work

- Unit/process tests: release selection, hash rejection, managed-directory
  ownership, exclusive session lock, safe settings, graceful and forced shutdown.
- Client tests: readiness, protocol mismatch, startup failure/deadline, cleanup.
- Real pinned Pumpkin: launch → status 776 → stop → restart the same session.
- Repository fmt/clippy/test/doc/deny/audit gates; three-OS CI covers synthetic
  process tests. Real binary smoke tests are opt-in and keep data outside the repo.
- Next client work: bounded NBT → offline Login/Configuration/Play → chunk data
  and rendering → movement and block interaction. Pause is explicitly deferred.

## Verification (2026-09-06)

On aarch64 macOS, the SHA-256-verified release answered protocol 776 through our
client, completed its save sequence, and reopened the same session. The runtime
created a nonempty `world/level.dat`. This verifies process/status/persistence
plumbing, not player joins or chunk/block persistence, which need the next client
milestones. Upstream logged an initial level.dat creation warning and embedded
test-instance warning, then completed shutdown successfully.

Validation: 41 default workspace tests passed on macOS, plus the opt-in real
Pumpkin start/stop/reopen test. Fmt, strict Clippy, rustdoc, cargo-deny, and
cargo-audit passed (129 locked packages; only unused license-allowance warnings).
No dependency-wide update was run. Linux/Windows execution awaits the existing
CI matrix; platform-specific process-group setup is compiled there.

No performance optimization is claimed: launch-time executable hashing and
filesystem setup do not run in the render or simulation loop. The client polls
readiness/exit at bounded intervals; there is no new game-server implementation.
