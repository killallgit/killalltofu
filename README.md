# killalltofu

Schedule automatic destruction of Terraform/OpenTofu infrastructure. Run `killalltofu 1h` from a project directory, and after that duration, the infrastructure gets torn down automatically via cron.

## Installation

### Homebrew

```bash
brew tap killallgit/killallgit
brew install killalltofu
```

Or install directly:

```bash
brew install killallgit/killallgit/killalltofu
```

### From source

```bash
cargo install --git https://github.com/killallgit/killalltofu
```

### Binary

Download pre-built binaries from [GitHub Releases](https://github.com/killallgit/killalltofu/releases).

## Usage

### Schedule destruction

```bash
cd my-infra-project
killalltofu 1h          # destroy in 1 hour
killalltofu 30m         # destroy in 30 minutes
killalltofu 2d          # destroy in 2 days
```

This registers the current directory for scheduled destruction and installs a cron job that checks every minute.

A `.killalltofu` marker file is written to the project directory with the scheduled destruction time.

### List registered projects

```bash
killalltofu list
```

```
NAME                         DESTROY AT                 ENGINE       ATTEMPTS
users-ryan-code-my-infra     2026-03-02T17:00:00Z       tofu         0/3
users-ryan-code-staging      2026-03-03T00:00:00Z       terragrunt   1/3
```

### Cancel scheduled destruction

```bash
cd my-infra-project
killalltofu cancel
```

### Destroy immediately

```bash
cd my-infra-project
killalltofu kill                    # destroy current directory
killalltofu kill --name some-key    # destroy by registry key
```

Runs the destroy command inline, blocking, and streams output to the terminal.

## How it works

1. `killalltofu <duration>` writes an entry to `~/.killalltofu/registry.json` and installs a cron entry (`* * * * * killalltofu check`) if not already present
2. Every minute, cron runs `killalltofu check`, which processes any entries past their destroy time
3. On success: removes the `.killalltofu` marker and unregisters the project
4. On failure: retries up to 3 times with backoff, then writes a `.killallerror` file

Concurrent access to the registry is protected by file locks (`~/.killalltofu/registry.lock`) — shared locks for reads, exclusive locks for writes. The registry is written atomically (write to temp file, then rename).

### Engine detection

- If `terragrunt.hcl` is present in the project directory: `terragrunt destroy -auto-approve`
- Otherwise: tries `tofu` first, falls back to `terraform` -> `destroy -auto-approve`

The absolute binary path is resolved at registration time and stored in the registry, so cron doesn't depend on PATH.

### Retry backoff

| Attempt | Timing |
|---------|--------|
| 1 | At scheduled time |
| 2 | 5 minutes after attempt 1 |
| 3 | 15 minutes after attempt 2 |
| After 3 | Writes `.killallerror`, stops retrying |

Attempt logs are saved to `~/.killalltofu/logs/`.

### File layout

```
~/.killalltofu/
  registry.json         # registered projects
  registry.lock         # file lock for concurrent access
  logs/
    <key>-attempt-<n>.log

<project-dir>/
  .killalltofu          # marker file with destroy_at timestamp
  .killallerror         # written on max retry failure
```

Add `.killalltofu` and `.killallerror` to your project's `.gitignore`.

## Testing with Docker

A test fixture is included that uses the Docker provider — no cloud account needed. Requires Docker running locally and [OpenTofu](https://opentofu.org/) installed.

```bash
cd tests/fixtures/docker-nginx
tofu init
tofu apply -auto-approve    # spins up nginx on localhost:8088
curl localhost:8088          # verify it's running
```

Then test killalltofu against it:

```bash
# schedule destruction in 2 minutes
killalltofu 2m

# or destroy immediately
killalltofu kill

# verify container is gone
docker ps | grep killalltofu-test
```

## License

MIT
