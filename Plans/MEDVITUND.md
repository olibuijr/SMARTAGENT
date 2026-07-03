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
│ ./pi --mode rpc --session-id gw-<agent>         
│   ▲                                             
│   │ every heartbeat_secs (120), per agent       
│ beat: time · elapsed · doing task · workflow    
│       step · plan ahead → steer (busy) /        
│       queue (idle) → semdb medvitund table      
└────────────┘
```

## Components

- **`crates/gateway`** (pure Rust, std only):
  - `serve --agents main,builder,qa,ops` — daemon. Spawns one persistent `./pi --mode rpc --session-id gw-<agent>` child per named agent. Listens on one `gateway_socket` (config), routes by the request's `agent` field, broadcasts each agent only to that agent's attached clients, and logs transcripts to `data/gateway/<agent>.log`.
  - `attach --agent <name>` — interactive client: streams that agent's assistant text, stdin lines → `prompt`/`steer` (auto: steer when busy). Detach (Ctrl-D) leaves the agent running.
  - `send --agent <name> <msg>` — one-shot message (used by schedule, scripts, Óli); returns after delivery confirmation.
  - `steer --agent <name> <msg>` — injects into a busy named agent; returns after delivery confirmation.
  - `status --agent <name>` — one named agent's busy/idle state, last beat, queue flag, and board snapshot.
  - `agents` — TSV-style list for panels: `name, state, current task, role`.
  - `stop --agent <name>` — kills one child (session preserved — `--session-id` resumes it on next serve).
- **Heartbeat** (`beat.rs`): every `heartbeat_secs` compose from local state (no model call to build it): current time + elapsed since session start, board `doing`/`ready` snapshot (`tasks` binary), active workflow run + step, queued follow-ups. Delivery: agent **busy** → RPC `steer` (lands between turns); agent **idle** → queued and prefixed to the next message, so idle agents cost zero tokens. `--autonomous` (opt-in per agent): idle beats become `prompt`s telling the agent to continue the plan — the all-day worker mode.
- **Meðvitund log**: every beat AND every turn-end appends a row to semdb table `medvitund` (`ts, agent, state busy|idle, doing, plan, last_text ≤ 400 chars`). No vector column v1 (embeddings optional); this is the agent's self-history — interviews recall from it (`memory`/`semdb` tools already reach it). Mute-session recovery incidents are logged here too (`kind=incident`, states `compact-retry`, `rotate-session`, `rotate-failed`).
- **Mute-session self-heal**: the event pump treats an empty assistant turn with total token usage `0` (`input=output=cacheRead=cacheWrite=0`) as a mute signal. The first such turn is observed, the second sends RPC `compact` and retries through the next heartbeat, and a third archives the wedged child by rotating to a fresh `gw-<agent>-recovered-<ts>` session with a continuity note. This covers both rejected cross-model reasoning signatures and context-ceiling exhaustion in long-lived RPC sessions.
- **Service**: run under `supervise` (`supervise add gateway ...`) or systemd --user later.

## Protocol notes (probed 2026-07-02, pi-coding-agent 0.80.3)

- Commands: line-JSON `{"id?","type":"prompt|steer|follow_up|abort|get_state|new_session|...","message?"}`.
- Events: `agent_start`/`agent_end` = busy/idle edges; `message_update.text_delta` = stream; `message_end` role=assistant = final text.
- `extension_ui_request` (statusline setWidget etc.) may be ignored — headless `-p` runs already do.
- Every child read/write goes through deadline-guarded channels — the 2026-07-02 freeze lesson (httpc connect timeout) generalized: **no unbounded blocking on another process, ever**.

## Multi-agent handoff protocol

The gateway hosts several named long-lived agents (default service set: `main,builder,qa,ops`), but the board remains the lock and handoff protocol:

1. Each agent may hold at most one root-board task in `doing`.
2. A Builder-style agent that finishes implementation moves the task to `review`, not `done`, when a distinct role should verify it.
3. The handoff note is encoded in the review task text/criteria: include the intended assignee/role (`assignee: qa`, `assignee: ops`, etc.) and the exact probe evidence the reviewer should rerun.
4. Reviewers pull only review/ready work addressed to their role, verify criteria, and either check criteria + move `done` or send a concrete defect back to `backlog`.
5. `gateway agents` exposes each named agent's state so a coordinator can see parallel board occupancy without stealing another agent's task.

## Out of scope v1

Multi-client conflict resolution (last-writer wins), TCP/remote clients (unix socket only; SSH is the remote layer), dynamic runtime add/remove of agent names without restarting the daemon, TUI rendering in attach (plain text stream).

## Delivery steps (board-tracked)

1. `gateway` crate: child mgmt + serve + send/status (T-gateway-1)
2. heartbeat + meðvitund semdb table (T-gateway-2)
3. attach client + steer-while-busy interviews (T-gateway-3)
4. supervise unit + docs (AGENTS.md catalog, CHANGELOG) (T-gateway-4)
