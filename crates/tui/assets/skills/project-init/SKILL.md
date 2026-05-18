---
name: project-init
description: Scaffold a new project with best-practice structure, config files, CI, linters, and documentation — tailored to the project's language and framework.
---

# Project Init

Use this skill to bootstrap a new project from scratch. Generates directory
structure, config files, CI pipeline, linter setup, and a project README.

## Workflow

1. Ask the user 2-3 questions: language/framework, project type (library, CLI,
   web app, service), and preferred license (MIT, Apache-2.0, GPL-3.0).
2. Detect or ask about existing tools: package manager, linter preference,
   testing framework.
3. **Generate structure**:
   ```
   project/
   ├── src/           # source code
   ├── tests/         # test files
   ├── docs/          # documentation
   ├── .github/       # CI workflows (GitHub Actions)
   ├── README.md      # bilingual if zh-CN detected
   ├── LICENSE        # chosen license
   ├── .gitignore     # language-appropriate
   └── Makefile       # or just/package.json equivalent
   ```
4. **CI**: Generate a GitHub Actions workflow matrix for the relevant language
   versions. Include lint, test, and build steps.
5. **Linters**: Add default config (`.eslintrc`, `pyproject.toml`, `clippy.toml`,
   etc.) with sensible defaults.
6. **README**: Include badges (CI, license, version), quick-start, install
   instructions, and a minimal usage example.
7. **Git init**: Run `git init`, create an initial commit with the scaffold.

## Language-specific notes

- **Rust**: Cargo workspace, `clippy` + `rustfmt`, `cargo-deny` for license checks.
- **Python**: `pyproject.toml` (setuptools/poetry), `ruff` for lint, `pytest`.
- **TypeScript/Node**: `tsconfig.json`, `eslint` + `prettier`, `vitest` or `jest`.
- **Go**: `go.mod`, `golangci-lint`, standard project layout.
- **Zig**: `build.zig`, `zig fmt`.

## Constraints

- Never commit API keys, tokens, or secrets to the scaffold.
- Prefer MIT for maximum compatibility unless the user specifies otherwise.
- Generate only what the user needs — don't over-scaffold.
