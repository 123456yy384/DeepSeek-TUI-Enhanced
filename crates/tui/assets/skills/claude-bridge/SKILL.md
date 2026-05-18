---
name: claude-bridge
description: Auto-discover Claude Code ecosystem assets with hard privacy rules — automatically skip anything involving internal IPs, credentials, secrets, webhooks, or personal server configs.
---

# Claude Code Bridge (Auto-Discovery + Privacy Guard)

DeepSeek-TUI automatically discovers Claude Code ecosystem assets from
`~/.claude/` and project `.claude/` directories. A built-in privacy guard
blocks anything that looks like personal infrastructure or credentials.

## Hard Privacy Rules (ALWAYS enforced, cannot be overridden)

When scanning `~/.claude/`, DS-TUI automatically skips any skill, hook,
or config that matches ANY of these rules:

| Rule | Pattern | Example |
|------|---------|---------|
| Internal IPs | `192.168.*`, `10.*`, `172.16-31.*` | LAN server configs |
| Webhook URLs | `*webhook*`, `*hook.xxx*` | Feishu/Discord/Slack webhooks |
| Credentials | `*secret*`, `*token*`, `*password*`, `*api_key*`, `*credential*` | Hardcoded API keys |
| Private servers | `*internal*`, `*private*`, `*intranet*`, `*lan*`, `*内网*` | Internal infrastructure |
| Personal info | `*wechat*`, `*feishu*`, `*lark*`, `*dingtalk*` | Personal messaging bot configs |
| Phone/Email | `*phone*`, `*mobile*`, `*@*.com`, `*@*.cn` | Contact info in frontmatter |

If a skill's name, description, or body matches any rule → **silently skipped**,
logged to `~/.deepseek/claude-bridge-skipped.log`.

## Auto-Discovery

On startup, DS-TUI scans:
```
~/.claude/skills/          # User Claude Code skills
.claude/skills/            # Project Claude Code skills
~/.claude/projects/*/memory/  # Claude Code memory files
```

Skills that pass the privacy guard are listed via `/skills` with a `[claude]` tag.
They are NOT copied — DS-TUI reads them directly from the Claude Code directory.

## Commands

```
/claude list       # Show all discovered Claude skills (with skip reasons)
/claude import <name>  # Explicitly import a specific skill
/claude forget     # Stop tracking Claude Code directories
```

## Skip Log

`~/.deepseek/claude-bridge-skipped.log` records every skipped item:
```
2026-05-17 02:30:15 SKIP feishu — matches rule: webhook URLs
2026-05-17 02:30:15 SKIP lan-server — matches rule: internal IPs (192.168.x.x)
```
