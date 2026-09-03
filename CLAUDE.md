# CLAUDE.md

Follow **[AGENTS.md](AGENTS.md)**.

Claude-specific:
- `~/.claude/CLAUDE.md` (global) also applies: no subagents, no redundant
  verification passes, stay within the requested scope.
- End commit messages with a
  `Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>` trailer (plus a
  `Claude-Session:` link when the session provides one).
- No repo-specific hooks, subagents, or compaction settings.
