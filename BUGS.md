# Bugs

[CLI] `--version` / `--help` get filtered by `should_go_remote` outside an abrasive workspace and forwarded to cargo, even though they're abrasive subcommands — should be handled by the abrasive CLI regardless of workspace context

[CLI] Remote environment errors (missing `pkg-config`, missing system libs from build scripts) are buried in the cargo wall-of-text — the CLI should surface them distinctly instead of letting them drown in build output

[PROTOCOL] WebSocket Ping/Pong keepalive is missing — tungstenite doesn't auto-pong on the sync API, and the read loop silently `continue`s past Ping/Pong frames, so long builds can't detect dead peers. Needs either an explicit ping loop or a read timeout policy
