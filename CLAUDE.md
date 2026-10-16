# Kule — notes for Claude

- Design doc (Turkish): https://claude.ai/code/artifact/fd4bda01-5697-42ac-b415-81b15594b8ce
  Its "Kararlar" table is the source of truth for decisions.
- Milestones: M0 engine spike (CoW copy, effects, commit/rollback/undo) ·
  M1 git effects (gitoxide) → sandbox → Cedar policy → proxy (`lonca-net` from Lonca) · M2 Tauri app
  + ACP client · M3 approval inbox, timeline, checkpoints on Lonca · M4
  parallel tasks and measurement. v0 = macOS and Linux; Windows is v1.
- Run cargo with `export PATH="$HOME/.cargo/bin:$PATH"`.
- Tests run on the real filesystem (tempdirs), never on mocks. Every fix gets
  a test that fails without it.
- Chat with the user in Turkish; code, comments and repo docs in English.
