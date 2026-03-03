# killalltofu — Self-Destructing Terraform/OpenTofu CLI

## Overview

A Rust CLI that schedules automatic destruction of Terraform/OpenTofu infrastructure projects. Run `killalltofu 1h` from a project directory, and after that duration, the infrastructure gets torn down automatically via cron.

## CLI Interface

```
killalltofu <time>              # Register current dir for destruction (30s, 5m, 1h, 1d, 2w)
killalltofu list                # Show all registered projects with status
killalltofu cancel              # Cancel current dir (alias: delete)
killalltofu delete              # Alias for cancel
killalltofu kill                # Destroy current dir immediately (blocking, streams output)
killalltofu kill --name <key>   # Destroy a registered project by sanitized name
killalltofu check               # Internal: called by cron every minute
```

## Architecture

### Registry

`~/.killalltofu/registry.json` — single source of truth.

Each entry keyed by sanitized directory path (`/Users/ryan/Code/my-infra` -> `users-ryan-code-my-infra`):

```json
{
  "users-ryan-code-my-infra": {
    "path": "/Users/ryan/Code/my-infra",
    "destroy_at": "2026-03-02T17:00:00Z",
    "engine": "tofu",
    "attempts": 0,
    "last_attempt_at": null
  }
}
```

### Engine Detection

- If `terragrunt.hcl` exists in the project dir -> `terragrunt destroy -auto-approve`
- Otherwise -> `tofu destroy -auto-approve`

### Cron Setup

- Single crontab entry: `* * * * * /path/to/killalltofu check`
- Auto-installed on first `killalltofu <time>` invocation
- Self-heals: re-installs if entry is missing on subsequent runs

### Destroy Flow (via `check`)

1. Read registry
2. For each entry where `now >= destroy_at`:
   - Check backoff eligibility based on `last_attempt_at`
   - `cd` into project path
   - Run detected engine's destroy command
   - **Success**: delete `.killalltofu`, remove from registry
   - **Failure**: increment `attempts`, record `last_attempt_at`, write log
3. If `attempts >= 3`: write `.killallerror` to project dir, remove from registry

### Retry & Backoff

- Attempt 1: immediately at `destroy_at`
- Attempt 2: 5 minutes after attempt 1
- Attempt 3: 15 minutes after attempt 2
- After 3 failures: write `.killallerror`, unregister

### `kill` Command

Bypasses cron entirely. Runs destroy inline, blocking, streams output to terminal. On success: cleanup. On failure: print error directly (user is watching).

## File Layout

```
~/.killalltofu/
  registry.json
  logs/
    <key>-attempt-<n>.log

<project-dir>/
  .killalltofu            # Marker file with destroy_at timestamp
  .killallerror           # Written on max retry failure
```

### Log Format

```
Engine: tofu
Path: /Users/ryan/Code/my-infra
Timestamp: 2026-03-02T17:00:00Z
Exit code: 1
--- OUTPUT ---
<full stdout+stderr>
```

### `.killallerror` Format

```
Engine: tofu
Attempts: 3
Last attempt: 2026-03-02T17:20:00Z
See logs: ~/.killalltofu/logs/users-ryan-code-my-infra-attempt-*.log
--- LAST OUTPUT ---
<full stdout+stderr from last attempt>
```

### `list` Output

```
NAME                         DESTROY AT              ENGINE      ATTEMPTS
users-ryan-code-my-infra     2026-03-02T17:00:00Z    tofu        0/3
users-ryan-code-staging      2026-03-03T00:00:00Z    terragrunt  1/3
```

## Dependencies

- `clap` 4.5 (derive) — CLI parsing
- `serde` + `serde_json` — registry serialization
- `chrono` — timestamp handling
- `anyhow` — error handling
- `tokio` — async command execution (if needed, otherwise std::process)
- `humantime` — parsing duration strings (1h, 30s, 2d)

## CI/CD

Mirrors killallgit's pipeline:

- **Test job**: clippy (deny warnings) + cargo test
- **Build matrix**: linux x86_64, linux aarch64, macos aarch64, macos x86_64
- **Release**: triggered by GitHub Release creation, uploads tarballs + SHA256 checksums
- **Homebrew**: repository dispatch to `killallgit/homebrew-killallgit` for formula update
- No Windows target (cron-based, Unix only)

## Project Structure

```
killalltofu/
  .github/workflows/ci.yml
  src/
    main.rs           # Entry point, CLI definition
    registry.rs       # Registry read/write/sanitize
    destroy.rs        # Engine detection, destroy execution
    cron.rs           # Crontab installation/verification
    logging.rs        # Log file writing
    time.rs           # Duration parsing, backoff logic
  tests/
    integration_test.rs
  Cargo.toml
  Cargo.lock
  Taskfile.yml
  LICENSE
  .gitignore
```
