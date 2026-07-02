# Meðvitund — persistent agent gateway + heartbeat

> Goal: agents that are aware of time, what they're doing, and the plan ahead — running all day as a service Óli can attach to, interview, and hand work at any moment. (OpenClaw/Hermes pattern: gateway owns the session; clients come and go.)

## Architecture

```
┌────────────┐   unix socket .pi/gateway.sock    ┌──────────────────────────┐
│ gateway     │◄──────────────────────────────────┤ clients: gateway attach, │
│ serve       │                                   │ gateway send, statusline │
│ (supervise) │                                   └──────────────────────────┘
│   │ owns                                        
│   ▼ stdio (line-JSON RPC)                       
│ ./pi --mode rpc --session-id <agent>            
│   ▲                                             
│   │ every heartbeat_secs (120)                  
│ beat: time · elapsed · doing task · workflow    
│       step · plan ahead → steer (busy) /        
│       queue (idle) → semdb medvitund table      
└────────────┘
```

## Components

- **`crates/gateway`** (pure Rust, std only):
  - `serve` — daemon. Spawns one persistent `./pi --mode rpc --session-id gw-<agent>` child per agent (v1: single agent `main`). Listens on `gateway_socket` (config). Broadcasts agent events to attached clients; logs transcript to `data/gateway/<agent>.log`.
  - `attach [agent]` — interactive client: streams assistant text, stdin lines → `prompt`/`steer` (auto: steer when busy). Detach (Ctrl-D) leaves the agent running.
  - `send <agent> <msg>` — one-shot message (used by schedule, scripts, Óli).
  - `status` — agents, busy/idle, uptime, last beat, queue depth.
  - `stop [agent]` — graceful shutdown (child killed, session preserved — `--session-id` resumes it on next serve).
- **Heartbeat** (`beat.rs`): every `heartbeat_secs` compose from local state (no model call to build it): current time + elapsed since session start, board `doing`/`ready` snapshot (`tasks` binary), active workflow run + step, queued follow-ups. Delivery: agent **busy** → RPC `steer` (lands between turns); agent **idle** → queued and prefixed to the next message, so idle agents cost zero tokens. `--autonomous` (opt-in per agent): idle beats become `prompt`s telling the agent to continue the plan — the all-day worker mode.
- **Meðvitund log**: every beat AND every turn-end appends a row to semdb table `medvitund` (`ts, agent, state busy|idle, doing, plan, last_text ≤ 400 chars`). No vector column v1 (embeddings optional); this is the agent's self-history — interviews recall from it (`memory`/`semdb` tools already reach it).
- **Service**: run under `supervise` (`supervise add gateway ...`) or systemd --user later.

## Protocol notes (probed 2026-07-02, pi-coding-agent 0.80.3)

- Commands: line-JSON `{"id?","type":"prompt|steer|follow_up|abort|get_state|new_session|...","message?"}`.
- Events: `agent_start`/`agent_end` = busy/idle edges; `message_update.text_delta` = stream; `message_end` role=assistant = final text.
- `extension_ui_request` (statusline setWidget etc.) may be ignored — headless `-p` runs already do.
- Every child read/write goes through deadline-guarded channels — the 2026-07-02 freeze lesson (httpc connect timeout) generalized: **no unbounded blocking on another process, ever**.

## Out of scope v1

Multi-client conflict resolution (last-writer wins), TCP/remote clients (unix socket only; SSH is the remote layer), multi-agent fleets (registry is designed for it, ship single `main` first), TUI rendering in attach (plain text stream).

## Delivery steps (board-tracked)

1. `gateway` crate: child mgmt + serve + send/status (T-gateway-1)
2. heartbeat + meðvitund semdb table (T-gateway-2)
3. attach client + steer-while-busy interviews (T-gateway-3)
4. supervise unit + docs (AGENTS.md catalog, CHANGELOG) (T-gateway-4)
