# SMARTAGENT 50-Step QA Scenario

Purpose: define the canonical 50-step QA scenario for SMARTAGENT SD-testing.

Roles:
- Test Manager = orchestrator, composes prompts and audits
- QA Agent = ./pi, authors and executes

Plan version: 1.3
Date: 2026-07-02

Amendment rule: step IDs T01–T50 are stable forever — the plan is amended with a version bump (1.1, 1.2, …), never regenerated; run sheets ARE regenerated each iteration.
Amendments: 1.1 (2026-07-02) — T19/T39 Tool fields corrected from bash to httpc/hooks (QA defect, Test Manager audit); 1.2 (2026-07-02) — T19 extended with blackhole fail-fast regression (httpc connect-timeout fix); 1.3 (2026-07-02) — run-marker parameterized qa-run-<N> (cross-vendor audit: run-collision defect).

## Phase A — Environment & headless basics (T01–T05): headless reply smoke test; tools list matches AGENT_TOOLS.md; workspace listing (folders under workspaces/, never repo root); config resolution from config/smartagent.conf (no hardcoded endpoints); session state/statusline present.
### T01 — Headless reply smoke
- **Tool:** bash
- **Action:** ./pi -p 'qa-run-<N> smoke test: reply with OK only.' < /dev/null
- **Expected:** a single OK-style reply is produced headlessly
- **Evidence:** exact stdout containing the OK reply
### T02 — Tools list matches AGENT_TOOLS.md
- **Tool:** skills
- **Action:** ./pi -p 'qa-run-<N> list all available tools, names only, comma-separated.' < /dev/null
- **Expected:** tool names correspond to the documented AGENT_TOOLS.md tool list
- **Evidence:** stdout listing the expected tool names
### T03 — Workspace listing only
- **Tool:** bash
- **Action:** ./pi -p 'qa-run-<N> list the folders under workspaces/ only; never list repo root.' < /dev/null
- **Expected:** output contains workspace folders, not repo root contents
- **Evidence:** stdout names under workspaces/
### T04 — Config resolution
- **Tool:** bash
- **Action:** ./pi -p 'qa-run-<N> read config/smartagent.conf and report the configured endpoints without hardcoding any URL.' < /dev/null
- **Expected:** config-derived endpoints are reported from config/smartagent.conf
- **Evidence:** stdout shows config-backed endpoint values
### T05 — Session state and statusline present
- **Tool:** bash
- **Action:** ./pi -p 'qa-run-<N> confirm session state and statusline are present in the live startup context.' < /dev/null
- **Expected:** startup context includes session state/statusline information
- **Evidence:** stdout includes both session state and statusline mentions

## Phase B — Memory & semantic storage (T06–T12): semdb store + semantic search; memory store + recall (global); memory store + recall with project scoping; rag ingest + retrieve; context; session-memory persistence.
### T06 — semdb store and semantic search
- **Tool:** semdb
- **Action:** ./pi -p 'qa-run-<N> store qa-run-<N>-semdb fact and then semantic-search it.' < /dev/null
- **Expected:** stored text is retrievable by semantic search
- **Evidence:** stdout shows the stored row and a matching search hit
### T07 — Global memory store and recall
- **Tool:** memory
- **Action:** ./pi -p 'qa-run-<N> remember qa-run-<N>-global-memory, then recall it from global memory.' < /dev/null
- **Expected:** memory entry is stored and recalled globally
- **Evidence:** stdout contains the remembered fact and recall result
### T08 — Project-scoped memory
- **Tool:** memory
- **Action:** ./pi -p 'qa-run-<N> store qa-run-<N>-project-memory in project scope and recall it from that project only.' < /dev/null
- **Expected:** project-scoped memory is isolated to the project store
- **Evidence:** stdout shows the project-scoped recall only
### T09 — RAG ingest and retrieve
- **Tool:** rag
- **Action:** ./pi -p 'qa-run-<N> ingest a small qa-run-<N> rag note and retrieve it by query.' < /dev/null
- **Expected:** ingested content is retrievable through RAG
- **Evidence:** stdout shows document ingestion and a cited retrieval chunk
### T10 — Context load
- **Tool:** context
- **Action:** ./pi -p 'qa-run-<N> load principal context and summarize the active identity/goals.' < /dev/null
- **Expected:** principal context is composed successfully
- **Evidence:** stdout includes the loaded identity/goals summary
### T11 — Session-memory persistence
- **Tool:** bash
- **Action:** ./pi -p 'qa-run-<N> write a unique session-memory marker and then confirm it is persisted for the next run.' < /dev/null
- **Expected:** a subsequent session can observe the persisted marker
- **Evidence:** stdout from the next run references the qa-run-<N> marker
### T12 — Semantic recall confirmation
- **Tool:** semdb
- **Action:** ./pi -p 'qa-run-<N> perform a second semantic search over qa-run-<N> stored facts and report the best match.' < /dev/null
- **Expected:** semantic search returns the strongest matching qa-run-<N> fact
- **Evidence:** stdout shows the best-match row

## Phase C — Code intelligence (T13–T18): codeindex projects listing; codeindex index + search with project scoping; codegraph build, defs, refs, callers with project scoping.
### T13 — codeindex projects listing
- **Tool:** codeindex
- **Action:** ./pi -p 'qa-run-<N> list codeindex projects and their index status.' < /dev/null
- **Expected:** workspace projects and index state are listed
- **Evidence:** stdout contains the project listing
### T14 — codeindex index
- **Tool:** codeindex
- **Action:** ./pi -p 'qa-run-<N> index the current workspace project for code search.' < /dev/null
- **Expected:** project file inventory is indexed successfully
- **Evidence:** stdout confirms a completed index
### T15 — codeindex project-scoped search
- **Tool:** codeindex
- **Action:** ./pi -p 'qa-run-<N> search within the current project for a known symbol or file name.' < /dev/null
- **Expected:** search is scoped to the selected project
- **Evidence:** stdout shows hits from that project only
### T16 — codegraph build
- **Tool:** codegraph
- **Action:** ./pi -p 'qa-run-<N> build the codegraph for the current project.' < /dev/null
- **Expected:** code graph is built for the project
- **Evidence:** stdout reports graph build completion
### T17 — codegraph defs/refs
- **Tool:** codegraph
- **Action:** ./pi -p 'qa-run-<N> query defs and refs for one symbol in the current project.' < /dev/null
- **Expected:** definitions and references are returned
- **Evidence:** stdout lists defs and refs for the symbol
### T18 — codegraph callers
- **Tool:** codegraph
- **Action:** ./pi -p 'qa-run-<N> query callers for one symbol in the current project.' < /dev/null
- **Expected:** caller relationships are returned
- **Evidence:** stdout lists caller nodes/edges

## Phase D — Web & external (T19–T24): httpc GET; search (searx) web query; browser (CDP) page read; notify (ntfy) send.
### T19 — httpc GET
- **Tool:** httpc
- **Action:** ./pi -p 'qa-run-<N> perform an httpc GET against a harmless endpoint and print the response status.' < /dev/null
- **Expected:** GET succeeds and returns a status line; against an unreachable/blackholed host the tool errors within 10s (connect timeout) instead of hanging
- **Evidence:** stdout includes the HTTP status; unreachable-host probe returns a connect ... unreachable within 10s error, wall-clock bounded
### T20 — searx web query
- **Tool:** search
- **Action:** ./pi -p 'qa-run-<N> search the web for SMARTAGENT-related public info.' < /dev/null
- **Expected:** search returns ranked results
- **Evidence:** stdout includes search hits and snippets
### T21 — browser page read
- **Tool:** browser
- **Action:** ./pi -p 'qa-run-<N> open a public page in CDP and read the visible text.' < /dev/null
- **Expected:** browser snapshot shows page title and body text
- **Evidence:** stdout includes the visible text snapshot
### T22 — notify send
- **Tool:** notify
- **Action:** ./pi -p 'qa-run-<N> send a notification that QA is running.' < /dev/null
- **Expected:** notification request succeeds
- **Evidence:** stdout confirms notification dispatch
### T23 — web result sanity
- **Tool:** search
- **Action:** ./pi -p 'qa-run-<N> verify the web search results contain at least one relevant SMARTAGENT hit.' < /dev/null
- **Expected:** at least one relevant result is observed
- **Evidence:** stdout shows a relevant result title or snippet
### T24 — browser metadata
- **Tool:** browser
- **Action:** ./pi -p 'qa-run-<N> inspect page metadata in CDP and report title plus link count.' < /dev/null
- **Expected:** title and link summary are returned
- **Evidence:** stdout includes title and link count

## Phase E — Secrets & vault (T25–T28): vault put/get roundtrip; secrets set/get; secret masking in sandbox output.
### T25 — vault roundtrip put/get
- **Tool:** vault
- **Action:** ./pi -p 'qa-run-<N> store a vault note and read it back verbatim.' < /dev/null
- **Expected:** stored note is returned unchanged
- **Evidence:** stdout matches the inserted note
### T26 — secrets set/get
- **Tool:** secrets
- **Action:** ./pi -p 'qa-run-<N> set a temporary secret and read it back through the approved path.' < /dev/null
- **Expected:** secret roundtrip succeeds through the secrets tool
- **Evidence:** stdout confirms the secret value roundtrip
### T27 — sandbox secret masking
- **Tool:** sandbox
- **Action:** ./pi -p 'qa-run-<N> run a sandbox command that tries to print secret material and confirm masking.' < /dev/null
- **Expected:** secret material is masked or inaccessible inside sandbox output
- **Evidence:** stdout shows masked or denied secret access
### T28 — vault retrieval sanity
- **Tool:** vault
- **Action:** ./pi -p 'qa-run-<N> retrieve the qa-run-<N> vault note again and verify the content is stable.' < /dev/null
- **Expected:** vault note remains readable and unchanged
- **Evidence:** stdout repeats the same note content

## Phase F — Tasks, workflow, schedule (T29–T36): tasks add with criteria; move to doing; crit check; criteria-gated done; WIP-limit refusal; workflow start task-run + evidence-gated advance; schedule add/list/remove.
### T29 — tasks add with criteria
- **Tool:** tasks
- **Action:** ./pi -p 'qa-run-<N> add a task with acceptance criteria for QA scenario validation.' < /dev/null
- **Expected:** task is created with criteria
- **Evidence:** stdout shows the new task id and criteria
### T30 — tasks move to doing
- **Tool:** tasks
- **Action:** ./pi -p 'qa-run-<N> move the newly created QA task to doing.' < /dev/null
- **Expected:** task transitions to doing
- **Evidence:** stdout confirms the move
### T31 — crit check
- **Tool:** tasks
- **Action:** ./pi -p 'qa-run-<N> check the first acceptance criterion on the QA task.' < /dev/null
- **Expected:** the criterion is marked complete
- **Evidence:** stdout shows the checked criterion
### T32 — criteria-gated done
- **Tool:** tasks
- **Action:** ./pi -p 'qa-run-<N> complete the QA task only after all criteria pass.' < /dev/null
- **Expected:** done is accepted only when criteria are satisfied
- **Evidence:** stdout confirms criteria-gated completion
### T33 — WIP-limit refusal
- **Tool:** tasks
- **Action:** ./pi -p 'qa-run-<N> attempt to exceed the doing WIP limit and report the refusal.' < /dev/null
- **Expected:** the board refuses over-capacity work
- **Evidence:** stdout contains the WIP-limit refusal
### T34 — workflow start task-run
- **Tool:** workflow
- **Action:** ./pi -p 'qa-run-<N> start a task-run workflow for the QA task.' < /dev/null
- **Expected:** workflow run is created and linked to the task
- **Evidence:** stdout includes the workflow run id
### T35 — evidence-gated advance
- **Tool:** workflow
- **Action:** ./pi -p 'qa-run-<N> advance the workflow step with real evidence only.' < /dev/null
- **Expected:** step advances only with valid evidence
- **Evidence:** stdout shows the evidence-accepted transition
### T36 — schedule add/list/remove
- **Tool:** schedule
- **Action:** ./pi -p 'qa-run-<N> add a one-shot reminder, list schedules, then remove it.' < /dev/null
- **Expected:** schedule entry can be created, listed, and removed
- **Evidence:** stdout shows add/list/remove lifecycle

## Phase G — Agent infrastructure (T37–T43): skills match + show; hooks audit shows entries; sandbox run isolation; mcp listing; orchestrate parallel sub-run; supervise status.
### T37 — skills match
- **Tool:** skills
- **Action:** ./pi -p 'qa-run-<N> match the best skill for a QA test plan request.' < /dev/null
- **Expected:** an appropriate skill is selected
- **Evidence:** stdout names the matched skill
### T38 — skills show
- **Tool:** skills
- **Action:** ./pi -p 'qa-run-<N> show the matched skill details.' < /dev/null
- **Expected:** skill body/details are displayed
- **Evidence:** stdout includes the skill description
### T39 — hooks audit
- **Tool:** hooks
- **Action:** ./pi -p 'qa-run-<N> read the hooks audit trail and confirm entries exist.' < /dev/null
- **Expected:** hook audit contains one or more entries
- **Evidence:** stdout lists audited hook firings
### T40 — sandbox isolation
- **Tool:** sandbox
- **Action:** ./pi -p 'qa-run-<N> prove sandbox isolation by echoing a safe marker and checking it runs in isolation.' < /dev/null
- **Expected:** sandboxed command runs and is isolated
- **Evidence:** stdout shows the safe marker from sandbox
### T41 — mcp listing
- **Tool:** mcp
- **Action:** ./pi -p 'qa-run-<N> list available MCP servers or tools.' < /dev/null
- **Expected:** MCP tool inventory is returned
- **Evidence:** stdout includes the MCP listing
### T42 — orchestrate parallel sub-run
- **Tool:** orchestrate
- **Action:** ./pi -p 'qa-run-<N> fan out two parallel subagents for independent QA checks.' < /dev/null
- **Expected:** parallel subagent run is started and summarized
- **Evidence:** stdout shows both agent results
### T43 — supervise status
- **Tool:** supervise
- **Action:** ./pi -p 'qa-run-<N> report service health for scheduler and chromium.' < /dev/null
- **Expected:** service status is reported
- **Evidence:** stdout shows supervise status output

## Phase H — Evals & meta (T44–T47): evals log + score; commands; statusline segments healthy; injected AGENT_TOOLS context present.
### T44 — evals log
- **Tool:** evals
- **Action:** ./pi -p 'qa-run-<N> log a simple evaluation case for the QA scenario.' < /dev/null
- **Expected:** evaluation case is stored
- **Evidence:** stdout includes the logged case id
### T45 — evals score
- **Tool:** evals
- **Action:** ./pi -p 'qa-run-<N> score the logged evaluation case and report pass/fail.' < /dev/null
- **Expected:** scoring result is produced
- **Evidence:** stdout contains the score or verdict
### T46 — commands
- **Tool:** bash
- **Action:** ./pi -p 'qa-run-<N> invoke the slash-command style status summary and report it.' < /dev/null
- **Expected:** command output is returned in-chat
- **Evidence:** stdout includes the command-rendered summary
### T47 — injected AGENT_TOOLS context
- **Tool:** bash
- **Action:** ./pi -p 'qa-run-<N> confirm the injected AGENT_TOOLS context is present in this session.' < /dev/null
- **Expected:** AGENT_TOOLS context appears in the live session
- **Evidence:** stdout mentions AGENT_TOOLS context

## Phase I — Negative & regression guards (T48–T50): a file write with nothing in doing is BLOCKED by the hook; a path-traversal project name (e.g. ../../etc) is rejected; no /tmp usage anywhere — scratch only under .scratch/qa/ and artifacts only under Testing/.
### T48 — Write blocked with nothing in doing
- **Tool:** platform
- **Action:** ./pi -p 'qa-run-<N> attempt a file write while nothing is in doing and report the hook block.' < /dev/null
- **Expected:** the write is blocked by the hook
- **Evidence:** stdout contains the hook block message
### T49 — Path traversal rejected
- **Tool:** platform
- **Action:** ./pi -p 'qa-run-<N> try a project name like ../../etc and report the rejection.' < /dev/null
- **Expected:** path traversal is rejected
- **Evidence:** stdout contains the rejection reason
### T50 — No /tmp usage
- **Tool:** platform
- **Action:** ./pi -p 'qa-run-<N> verify no /tmp usage exists and scratch stays under .scratch/qa/ with artifacts only under Testing/.' < /dev/null
- **Expected:** no /tmp references appear; scratch stays in .scratch/qa/ and artifacts stay under Testing/
- **Evidence:** stdout explicitly confirms the no-/tmp rule

## Regeneration protocol — the exact command the Test Manager runs each iteration:
./pi -p 'Read Testing/TESTPLAN.md and generate Testing/runs/RUN-<next>.md: all 50 boxes unchecked, run metadata filled, previous-run link set, and unresolved defects copied into Defect carry-over from the latest run sheet.' < /dev/null
Substitution rule: <N> is the current run number; the executor substitutes it literally (run 1 used qa-run-1) so iterations never collide and teardown can key on the qa-run-<N>- prefix.

## Defect protocol — a failed step is a PLATFORM defect: file a kanban task titled 'QA defect T<NN>: <symptom>' with the evidence; the cause is fixed in the platform (skill, hook, crate, extension, doc), never by softening the step; the step re-runs next iteration.

## Teardown & data hygiene — how run-created artifacts (tasks, memory entries, vault/secret keys, schedules, rag docs — all prefixed qa-run-<N>-) are removed after each run so iterations stay independent.
