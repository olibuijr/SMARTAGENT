# SMARTAGENT ops

SMARTAGENT manages its own long-running services with an **internal pure-Rust
supervisor** (`crates/supervise`), not a pile of systemd units. The supervisor
spawns, tracks (pid + cmd + uptime in a semdb table), health-checks, and
self-heals the services the agent depends on. The agent can drive it directly
via the `supervise` tool.

## Services under supervision

| Service | What | Health |
|---------|------|--------|
| `scheduler` | cron daemon firing due jobs (reminders via `notify`, nightly backup) | /proc liveness |
| `chromium` | headless Chrome with CDP on `:9222` for the `browser` tool | HTTP probe `:9222/json/version` |

SearXNG runs as a docker container (`smartagent-searxng`, `--restart unless-stopped`) —
docker is its process manager. The titan embeddings endpoint is external (see `config/smartagent.conf`).

## Control

```sh
supervise status              # state / pid / health of each service
supervise up [service]        # start all (or one)
supervise down [service]      # stop all (or one)
supervise restart <service>   # restart one
supervise watch               # foreground self-healing loop (restarts dead services every 15s)
```

State: `data/supervise.semdb`. Logs: `workspaces/supervise-logs/`.

## Boot persistence

Something has to launch the supervisor once at boot. There is exactly **one**
optional systemd unit for that — it runs `supervise watch`, which then starts
and heals everything else:

```sh
mkdir -p ~/.config/systemd/user
cp ops/systemd/smartagent-supervise.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now smartagent-supervise.service
loginctl enable-linger "$USER"   # so it runs without an active login (survives reboot)
```

Don't want systemd at all? Start the supervisor any other way — `supervise watch &`
from your shell rc, a login hook, or just ask the agent to run `supervise up`.
Nothing else depends on systemd.

## Backups

`ops/backup.sh` tars `data/` (memory, secrets, schedule, semdb) to
`~/.smartagent-backups/`, keeps 7, verifies the archive. It runs nightly as a
**scheduler job** (added with `SMARTAGENT_SCHEDULE_ADMIN=1 schedule add`), so no
separate timer is needed. Restore:

```sh
tar -xzf ~/.smartagent-backups/smartagent-data-<STAMP>.tar.gz -C /path/to/SMARTAGENT
```

## Preflight

`ops/preflight.sh` starts anything down and probes every endpoint in
`config/smartagent.conf` — run it after a reboot or before a session if you're
not using the boot unit.

## Updating pi

`./pi --self-update` — pinned version in `.pi/runtime/package.json`, smoke-tested
with automatic rollback. Never automatic; normal launches never touch the network.
