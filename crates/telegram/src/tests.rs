use super::{
    build_gateway_prompt, chunks, command_help, command_menu_body, progress_event_for_stream_line,
    progress_frame, progress_frame_for_verbosity, progress_scope_key, sanitize_progress_body,
    should_emit_progress, should_emit_progress_for_verbosity, simulate_stream_frames,
    slash_command, slash_name, streaming_preview, telegram_commands, ProgressEvent, VerbosityLevel,
};

#[test]
fn shared_registry_marks_telegram_and_tui_commands_in_parity() {
    let registry = super::parse_command_registry();
    assert!(registry.iter().all(|c| c.telegram == c.tui), "{registry:?}");
    assert!(registry.iter().any(|c| c.name == "assign"));
}

#[test]
fn group_mentions_are_recognized_and_cleaned_for_gateway_prompt() {
    assert!(super::is_group_mention(
        "supergroup",
        "@olafurs_bot can you answer?",
        "olafurs_bot"
    ));
    assert!(super::is_group_mention(
        "group",
        "hey @Olafurs_Bot, status?",
        "olafurs_bot"
    ));
    assert!(!super::is_group_mention(
        "private",
        "@olafurs_bot status?",
        "olafurs_bot"
    ));
    assert!(!super::is_group_mention(
        "supergroup",
        "hello everyone",
        "olafurs_bot"
    ));
    assert_eq!(
        super::strip_bot_mention("hey @olafurs_bot, status?", "olafurs_bot"),
        "hey status?"
    );
}

#[test]
fn telegram_agent_assignment_keyboard_is_multiple_choice() {
    let markup = super::agent_assignment_menu_markup();
    assert!(markup.contains("inline_keyboard"), "{markup}");
    for role in ["Coordinator", "Builder", "QA", "Ops"] {
        assert!(markup.contains(role), "{markup}");
    }
    let now = 1000;
    assert_eq!(
        super::callback_agent_assignment("assign:qa", now, now),
        Some("QA")
    );
}

#[test]
fn command_menu_lists_supported_slashes() {
    let body = command_menu_body();
    let help = command_help();
    let commands = telegram_commands();
    for c in &commands {
        assert!(
            body.contains(&format!("\"command\":\"{}\"", c.name)),
            "{body}"
        );
        assert!(
            help.contains(&format!("• /{} — {}", c.name, c.description)),
            "{help}"
        );
    }
    assert!(body.contains("\"commands\""));
}

#[test]
fn help_catalog_matches_registered_menu_exactly() {
    let help = command_help();
    let lines = help.lines().skip(1).collect::<Vec<_>>();
    let commands = telegram_commands();
    assert_eq!(lines.len(), commands.len(), "{help}");
    for (line, c) in lines.iter().zip(commands.iter()) {
        assert_eq!(*line, format!("• /{} — {}", c.name, c.description));
    }
    assert!(help.chars().count() < 2000, "{help}");
}

#[test]
fn formatter_wraps_slash_and_agent_outputs() {
    let board = super::format_telegram_response(
        super::ResponseKind::Board,
        "READY (1)\nT-1 p1 Demo [0/1✓]",
    );
    assert!(board.starts_with("📋 Board"), "{board}");
    assert!(board.contains("READY (1)"), "{board}");
    assert!(board.contains("Next: use /tasks"), "{board}");

    let answer = super::format_telegram_response(
        super::ResponseKind::AgentAnswer,
        "First line\nSecond line",
    );
    assert!(answer.starts_with("💬 Answer"), "{answer}");
    assert!(answer.contains("• First line"), "{answer}");
    assert!(answer.contains("• Second line"), "{answer}");
}

#[test]
fn formatter_preserves_bullets_code_and_tables() {
    let body = "• bullet\n```\ncode_with_*_chars\n```\n| A | B |";
    let out = super::format_telegram_response(super::ResponseKind::Memory, body);
    assert!(out.starts_with("🧠 Memory"), "{out}");
    assert!(out.contains("• bullet"), "{out}");
    assert!(out.contains("```\ncode_with_*_chars\n```"), "{out}");
    assert!(out.contains("| A | B |"), "{out}");
}

#[test]
fn formatter_snapshots_common_response_categories() {
    let cases = [
            (
                super::ResponseKind::AgentAnswer,
                "Done\nEvidence captured",
                "💬 Answer\n• Done\n• Evidence captured",
            ),
            (
                super::ResponseKind::TaskList,
                "T-1 p1 — Fix thing\nT-2 p2 — Test thing",
                "🧾 Ready tasks\n• T-1 p1 — Fix thing\n• T-2 p2 — Test thing\nNext: use /board for WIP and blockers.",
            ),
            (
                super::ResponseKind::Status,
                "scheduler: OK\ngateway: OK",
                "🩺 Status\n• scheduler: OK\n• gateway: OK",
            ),
            (
                super::ResponseKind::Confirmation,
                "Remembered for this chat.",
                "✅ Done\n• Remembered for this chat.",
            ),
            (
                super::ResponseKind::Blocker,
                "Command unavailable",
                "⛔ Blocked\n• Command unavailable\nNext: ask an admin or use an allowed chat.",
            ),
        ];
    for (kind, input, expected) in cases {
        assert_eq!(super::format_telegram_response(kind, input), expected);
    }
}

#[test]
fn formatter_preserves_small_tables_for_task_blocker_and_status_views() {
    let table = "| Item | State |\n|---|---|\n| T-1 | blocked |";
    for kind in [
        super::ResponseKind::TaskList,
        super::ResponseKind::Blocker,
        super::ResponseKind::Status,
    ] {
        let out = super::format_telegram_response(kind, table);
        assert!(out.contains("| Item | State |"), "{out}");
        assert!(out.contains("|---|---|"), "{out}");
        assert!(out.contains("| T-1 | blocked |"), "{out}");
    }
}

#[test]
fn formatted_outputs_are_concise_and_chunk_safe() {
    let out = super::format_telegram_response(
        super::ResponseKind::Board,
        &format!("READY\n{}", "T-1 demo\n".repeat(420)),
    );
    assert!(out.chars().count() <= 4001, "{}", out.chars().count());
    let chunks = super::final_message_chunks(&out);
    assert!(!chunks.is_empty());
    assert!(chunks.iter().all(|c| c.chars().count() <= 4000));
}

#[test]
fn markdown_escape_covers_dynamic_control_chars() {
    let escaped = super::escape_markdown("a_b *c* [d](e) `f`");
    assert_eq!(escaped, "a\\_b \\*c\\* \\[d\\]\\(e\\) \\`f\\`");
}

#[test]
fn blocker_formatter_uses_safe_sections() {
    let out = super::format_telegram_response(super::ResponseKind::Blocker, &super::safe_denial());
    assert!(out.starts_with("⛔ Blocked"), "{out}");
    assert!(out.contains("• Sorry,"), "{out}");
    assert!(out.contains("Next:"), "{out}");
}

#[test]
fn slash_commands_accept_bot_suffix() {
    let out = slash_command("/help@smartagent_bot", "1", "", "u1")
        .expect("recognized")
        .unwrap();
    assert!(out.contains("/board"), "{out}");
    assert_eq!(slash_name("/memory@smartagent_bot status"), Some("memory"));
    assert!(slash_command("/unknown@smartagent_bot", "1", "", "u1").is_none());
}

#[test]
fn slash_help_output_is_concise_and_streamable() {
    let out = slash_command("/help", "chat-a", "main", "u1")
        .expect("recognized")
        .unwrap();
    assert!(out.starts_with("SMARTAGENT commands:"), "{out}");
    assert!(out.contains("• /help — Show SMARTAGENT commands"), "{out}");
    let preview = streaming_preview("/help", &out);
    assert!(
        preview.starts_with("💭 /help\nSMARTAGENT commands:"),
        "{preview}"
    );
    assert!(preview.chars().count() <= 4000, "{preview}");
}

#[test]
fn slash_response_preview_is_streaming_shaped() {
    let preview = streaming_preview("/board", "READY\nT-1 demo");
    assert!(preview.starts_with("💭 /board\nREADY"), "{preview}");
    assert!(preview.ends_with('\u{2026}'), "{preview}");
}

#[test]
fn progress_events_map_to_telegram_safe_messages() {
    let cases = [
        (ProgressEvent::Planning, "🧭 Planning the next step…"),
        (ProgressEvent::ToolUse, "🔧 Using tools…"),
        (ProgressEvent::Waiting, "⏳ Waiting for a result…"),
        (ProgressEvent::Verifying, "✅ Verifying before replying…"),
        (ProgressEvent::Error, "⚠️ Error while working…"),
        (ProgressEvent::FinalAnswer, "💬 Final answer ready."),
    ];
    for (event, msg) in cases {
        assert_eq!(event.telegram_message(), msg);
        assert!(msg.chars().count() < 80, "{msg}");
        assert!(!msg.contains('\n'), "{msg}");
    }
}

#[test]
fn progress_stream_lines_select_events_and_frames() {
    assert_eq!(
        progress_event_for_stream_line("calling tool browser"),
        ProgressEvent::ToolUse
    );
    assert_eq!(
        progress_event_for_stream_line("waiting for result"),
        ProgressEvent::Waiting
    );
    assert_eq!(
        progress_event_for_stream_line("verify tests"),
        ProgressEvent::Verifying
    );
    assert_eq!(
        progress_event_for_stream_line("tool failed with error"),
        ProgressEvent::ToolUse
    );
    assert_eq!(
        progress_event_for_stream_line("request failed"),
        ProgressEvent::Error
    );
    let frame = progress_frame(ProgressEvent::ToolUse, "running cargo test");
    assert!(frame.starts_with("🔧 Using tools…"), "{frame}");
    assert!(frame.contains("running cargo test"), "{frame}");
}

#[test]
fn verbosity_command_is_listed_and_scope_local() {
    let help = command_help();
    assert!(help.contains("/verbosity"), "{help}");
    let dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("telegram-verbosity-test.semdb");
    let _ = std::fs::remove_file(&path);
    let mut db = semdb::storage::Db::create(&path).unwrap();
    super::set_verbosity_in_db(&mut db, "chat-a", "main", "u1", VerbosityLevel::Debug).unwrap();
    assert_eq!(
        super::verbosity_from_db(&db, "chat-a", "main", "u1"),
        VerbosityLevel::Debug
    );
    assert_eq!(
        super::verbosity_from_db(&db, "chat-a", "topic-2", "u1"),
        VerbosityLevel::Normal
    );
}

#[test]
fn verbosity_controls_progress_and_sanitizes_debug_body() {
    assert!(!should_emit_progress_for_verbosity(
        VerbosityLevel::Quiet,
        None,
        10_000
    ));
    assert!(!should_emit_progress_for_verbosity(
        VerbosityLevel::Normal,
        None,
        10_000
    ));
    assert!(should_emit_progress_for_verbosity(
        VerbosityLevel::Verbose,
        None,
        0
    ));
    assert!(!should_emit_progress_for_verbosity(
        VerbosityLevel::Verbose,
        Some(1_000),
        2_000
    ));
    assert!(should_emit_progress_for_verbosity(
        VerbosityLevel::Debug,
        Some(1_000),
        2_500
    ));
    let verbose = progress_frame_for_verbosity(
        VerbosityLevel::Verbose,
        ProgressEvent::ToolUse,
        "password=sekret cargo test",
    );
    assert_eq!(verbose, "🔧 Using tools…");
    let debug = progress_frame_for_verbosity(
        VerbosityLevel::Debug,
        ProgressEvent::ToolUse,
        "Bearer token bot123 failed",
    );
    assert!(debug.starts_with("🔧 Using tools…"), "{debug}");
    assert!(!debug.contains("bot123"), "{debug}");
    assert!(sanitize_progress_body("Authorization: Bearer token bot123").contains("[redacted]"));
}

#[test]
fn multi_tool_progress_frames_are_structured_at_verbose_and_debug() {
    let events = [
        (ProgressEvent::Planning, "plan next tool calls"),
        (
            ProgressEvent::ToolUse,
            "calling tool secrets with password=hidden",
        ),
        (ProgressEvent::Waiting, "waiting for browser"),
        (ProgressEvent::Verifying, "verify cargo test"),
        (ProgressEvent::Error, "request failed"),
    ];
    let verbose = events
        .iter()
        .map(|(event, body)| progress_frame_for_verbosity(VerbosityLevel::Verbose, *event, body))
        .collect::<Vec<_>>();
    assert_eq!(verbose[0], "🧭 Planning the next step…");
    assert_eq!(verbose[1], "🔧 Using tools…");
    assert_eq!(verbose[2], "⏳ Waiting for a result…");
    assert_eq!(verbose[3], "✅ Verifying before replying…");
    assert_eq!(verbose[4], "⚠️ Error while working…");
    let debug = progress_frame_for_verbosity(
        VerbosityLevel::Debug,
        ProgressEvent::ToolUse,
        "calling tool secrets --password=hidden",
    );
    assert!(debug.starts_with("🔧 Using tools…"), "{debug}");
    assert!(debug.contains("calling tool"), "{debug}");
    assert!(!debug.contains("hidden"), "{debug}");
    let tool = super::telegram_tool_status("🛠 memory running --password=hidden").unwrap();
    assert!(tool.starts_with("🛠 memory running"), "{tool}");
    assert!(!tool.contains("hidden"), "{tool}");
}

#[test]
fn progress_scope_and_rate_limit_are_chat_thread_local() {
    assert_ne!(
        progress_scope_key("chat-a", "main"),
        progress_scope_key("chat-b", "main")
    );
    assert_ne!(
        progress_scope_key("chat-a", "main"),
        progress_scope_key("chat-a", "topic-2")
    );
    assert!(should_emit_progress(None, 0));
    assert!(!should_emit_progress(Some(1_000), 2_000));
    assert!(should_emit_progress(Some(1_000), 2_500));
}

#[test]
fn final_message_chunks_preserve_order_without_duplicates() {
    let text = format!("A{}\nB{}", "a".repeat(3998), "b".repeat(100));
    let out = super::final_message_chunks(&text);
    assert_eq!(out.len(), 2, "{out:?}");
    assert!(out[0].starts_with('A'), "{out:?}");
    assert!(out[1].starts_with('B'), "{out:?}");
    assert_eq!(out.join(""), text);
    assert!(out.iter().all(|c| c.chars().count() <= 4000));
}

#[test]
fn telegram_retry_classifier_is_narrow() {
    assert!(super::is_retryable_telegram_error(
        "Too Many Requests: retry after 3"
    ));
    assert!(super::is_retryable_telegram_error("curl timed out"));
    assert!(super::is_retryable_telegram_error(
        "HTTP 503 temporarily unavailable"
    ));
    assert!(!super::is_retryable_telegram_error(
        "Bad Request: chat not found"
    ));
}

#[test]
fn observation_rows_capture_failures_without_secrets() {
    let mut db = test_db("telegram-observability");
    super::log_observation_to_db(
        &mut db,
        "command",
        "chat-a",
        "thread-1",
        "board",
        "ok",
        42,
        2,
        "tool_markers=1",
        "",
    )
    .unwrap();
    super::log_observation_to_db(
        &mut db,
        "send",
        "chat-a",
        "thread-1",
        "sendMessage",
        "error",
        7,
        1,
        "retry_failed",
        "Bearer token bot123 failed",
    )
    .unwrap();
    let rows = db
        .index
        .values()
        .map(|e| e.meta.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rows.contains("\"status\":\"ok\""), "{rows}");
    assert!(rows.contains("\"chunks\":2"), "{rows}");
    assert!(rows.contains("retry_failed"), "{rows}");
    assert!(!rows.contains("bot123"), "{rows}");
    assert!(rows.contains("[redacted]"), "{rows}");
}

#[test]
fn blocked_resolution_keyboard_and_callbacks_are_parseable() {
    let kb = super::blocked_keyboard("T-123");
    assert!(kb.contains("inline_keyboard"), "{kb}");
    assert!(kb.contains("block:unblock:T-123"), "{kb}");
    assert!(kb.contains("block:reassign:T-123"), "{kb}");
    assert!(kb.contains("block:drop:T-123"), "{kb}");
    assert!(kb.contains("block:text:T-123"), "{kb}");
    let act = super::block_callback_action("block:text:T-123").unwrap();
    assert_eq!(act.action, "text");
    assert_eq!(act.id, "T-123");
    assert!(super::block_callback_action("model:codex/gpt-5.4-mini").is_none());
}

#[test]
fn blocked_task_id_parser_ignores_no_tasks() {
    let ids = super::blocked_task_ids("T-1\tready\tp1\tDemo\nno tasks\nT-2\tdoing\tp2\tOther");
    assert_eq!(ids, vec!["T-1".to_string(), "T-2".to_string()]);
}

#[test]
fn free_text_resolution_usage_is_safe() {
    let out = super::resolve_block_text("T-123").unwrap();
    assert!(out.contains("Usage: /resolve T-123"), "{out}");
}

#[test]
fn telegram_tool_status_is_sanitized() {
    assert_eq!(
        super::telegram_tool_status("🛠 memory running…").unwrap(),
        "🛠 memory running…"
    );
    assert_eq!(super::telegram_tool_status("⚙ memory"), None);
    let preview = super::stream_preview("partial reply", "🛠 tasks ✓");
    assert!(preview.contains("partial reply"));
    assert!(preview.contains("🛠 tasks ✓"));
}

#[test]
fn simulated_stream_frames_show_incremental_answer() {
    let frames = simulate_stream_frames(&["Hel", "Hello\\nworld"]);
    assert_eq!(frames[0], "Hel");
    assert_eq!(frames[1], "Hello\nworld");
}

#[test]
fn simulated_stream_frames_show_tool_wrench_before_final() {
    let frames = simulate_stream_frames(&[
        "Starting",
        "__SMARTAGENT_INFO__🛠 memory running…",
        "Final answer",
    ]);
    assert!(frames[1].contains("Starting"), "{:?}", frames);
    assert!(frames[1].contains("🛠 memory running…"), "{:?}", frames);
    assert_eq!(frames[2], "Final answer");
}

#[test]
fn simulated_stream_frames_fallback_when_thinking_tokens_unsupported() {
    let frames = simulate_stream_frames(&["__SMARTAGENT_THINKING__hidden chain"]);
    assert_eq!(frames[0], super::thinking_fallback());
    assert!(!frames[0].contains("hidden chain"), "{:?}", frames);
}

#[test]
fn simulated_stream_frames_do_not_leak_between_chats() {
    let chat_a = simulate_stream_frames(&["chat A", "__SMARTAGENT_INFO__🛠 tasks ✓"]);
    let chat_b = simulate_stream_frames(&["chat B"]);
    assert!(chat_a.last().unwrap().contains("🛠 tasks ✓"), "{:?}", chat_a);
    assert_eq!(chat_b.last().unwrap(), "chat B");
    assert!(!chat_b.last().unwrap().contains("tasks"), "{:?}", chat_b);
}

#[test]
fn history_scope_separates_chats_and_threads() {
    assert_ne!(
        super::history_scope("1", "main"),
        super::history_scope("2", "main")
    );
    assert_ne!(
        super::history_scope("1", "10"),
        super::history_scope("1", "11")
    );
    assert_eq!(super::history_scope("chat:1", ""), "chat_1:main");
}

fn test_db(name: &str) -> semdb::storage::Db {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-scratch")
        .join(name);
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("telegram.semdb");
    let _ = std::fs::remove_file(&path);
    semdb::storage::Db::create(&path).unwrap()
}

#[test]
fn history_prune_is_scope_local() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-scratch/telegram-history");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("history-prune.semdb");
    let _ = std::fs::remove_file(&path);
    let mut db = semdb::storage::Db::create(&path).unwrap();
    for i in 0..4 {
        db.put(
            &format!("hist:chat_a:main:{i:020}:0:in"),
            "{}",
            super::VEC0.to_vec(),
        )
        .unwrap();
    }
    db.put(
        "hist:chat_b:main:00000000000000000000:0:in",
        "{}",
        super::VEC0.to_vec(),
    )
    .unwrap();
    super::prune_history(&mut db, "chat_a:main", 2).unwrap();
    assert_eq!(
        db.index
            .keys()
            .filter(|id| id.starts_with("hist:chat_a:main:"))
            .count(),
        2
    );
    assert!(db
        .index
        .contains_key("hist:chat_b:main:00000000000000000000:0:in"));
}

#[test]
fn inbound_and_reply_history_round_trip_in_scope() {
    let mut db = test_db("telegram-history-roundtrip");
    super::log_history_to_db(
        &mut db,
        &super::HistoryEvent {
            direction: "in",
            chat: "chat-a",
            thread: "main",
            user: "u1",
            from: "oli",
            update_id: 1,
            message_id: "m1",
            reply_to_update: 0,
            ts: 10,
            text: "remember the blue key",
        },
    )
    .unwrap();
    super::log_history_to_db(
        &mut db,
        &super::HistoryEvent {
            direction: "out",
            chat: "chat-a",
            thread: "main",
            user: "bot",
            from: "assistant",
            update_id: 2,
            message_id: "m2",
            reply_to_update: 1,
            ts: 11,
            text: "blue key noted",
        },
    )
    .unwrap();
    let h = super::scoped_history_from_db(&db, "chat-a", "main").unwrap();
    assert!(h.contains("- oli: remember the blue key"), "{h}");
    assert!(h.contains("- assistant: blue key noted"), "{h}");
}

#[test]
fn scoped_history_does_not_leak_between_chats() {
    let mut db = test_db("telegram-history-isolation");
    for (chat, text) in [("chat-a", "alpha secret"), ("chat-b", "beta secret")] {
        super::log_history_to_db(
            &mut db,
            &super::HistoryEvent {
                direction: "in",
                chat,
                thread: "main",
                user: "u1",
                from: "user",
                update_id: if chat == "chat-a" { 1 } else { 2 },
                message_id: "m",
                reply_to_update: 0,
                ts: if chat == "chat-a" { 10 } else { 11 },
                text,
            },
        )
        .unwrap();
    }
    let a = super::scoped_history_from_db(&db, "chat-a", "main").unwrap();
    let b = super::scoped_history_from_db(&db, "chat-b", "main").unwrap();
    assert!(
        a.contains("alpha secret") && !a.contains("beta secret"),
        "{a}"
    );
    assert!(
        b.contains("beta secret") && !b.contains("alpha secret"),
        "{b}"
    );
}

#[test]
fn headless_telegram_simulation_context_persists_across_turns() {
    let mut db = test_db("telegram-headless-sim");
    super::log_history_to_db(
        &mut db,
        &super::HistoryEvent {
            direction: "in",
            chat: "sim-chat",
            thread: "main",
            user: "u1",
            from: "tester",
            update_id: 1,
            message_id: "m1",
            reply_to_update: 0,
            ts: 10,
            text: "My project codename is aurora",
        },
    )
    .unwrap();
    super::log_history_to_db(
        &mut db,
        &super::HistoryEvent {
            direction: "out",
            chat: "sim-chat",
            thread: "main",
            user: "agent",
            from: "agent",
            update_id: 2,
            message_id: "m2",
            reply_to_update: 1,
            ts: 11,
            text: "I will remember aurora in this chat context.",
        },
    )
    .unwrap();

    let second_turn_context = super::scoped_history_from_db(&db, "sim-chat", "main").unwrap();
    assert!(
        second_turn_context.contains("aurora"),
        "{second_turn_context}"
    );
    assert!(
        second_turn_context.contains("tester"),
        "{second_turn_context}"
    );
    assert!(
        second_turn_context.contains("agent"),
        "{second_turn_context}"
    );
}

#[test]
fn reset_clears_only_current_chat_thread_history() {
    let mut db = test_db("telegram-history-reset");
    for (chat, thread, text, ts) in [
        ("chat-a", "main", "delete me", 10),
        ("chat-a", "topic-2", "keep thread", 11),
        ("chat-b", "main", "keep chat", 12),
    ] {
        super::log_history_to_db(
            &mut db,
            &super::HistoryEvent {
                direction: "in",
                chat,
                thread,
                user: "u1",
                from: "user",
                update_id: ts,
                message_id: "m",
                reply_to_update: 0,
                ts,
                text,
            },
        )
        .unwrap();
    }
    assert_eq!(
        super::reset_context_in_db(&mut db, "chat-a", "main").unwrap(),
        1
    );
    assert!(super::scoped_history_from_db(&db, "chat-a", "main").is_none());
    assert!(super::scoped_history_from_db(&db, "chat-a", "topic-2")
        .unwrap()
        .contains("keep thread"));
    assert!(super::scoped_history_from_db(&db, "chat-b", "main")
        .unwrap()
        .contains("keep chat"));
}

#[test]
fn stop_cancel_token_is_scope_local() {
    let mut db = test_db("telegram-stop-scope");
    super::set_cancel_token_in_db(&mut db, "chat-a", "main", 10).unwrap();
    assert_eq!(
        super::cancel_token_from_db(&db, "chat-a", "main").unwrap(),
        10
    );
    assert_eq!(
        super::cancel_token_from_db(&db, "chat-a", "topic-2").unwrap(),
        0
    );
    assert_eq!(
        super::cancel_token_from_db(&db, "chat-b", "main").unwrap(),
        0
    );
    assert!(super::stop_requested_in_db(&db, "chat-a", "main", 9));
    assert!(!super::stop_requested_in_db(&db, "chat-a", "main", 10));
    assert!(!super::stop_requested_in_db(&db, "chat-b", "main", 9));
}

#[test]
fn stop_command_is_listed_and_visible() {
    let body = command_menu_body();
    let help = command_help();
    assert!(body.contains("\"command\":\"stop\""), "{body}");
    assert!(help.contains("/stop"), "{help}");
    assert_eq!(slash_name("/stop@smartagent_bot"), Some("stop"));
}

#[test]
fn remember_command_is_listed_and_usable() {
    let body = command_menu_body();
    let help = command_help();
    assert!(body.contains("\"command\":\"remember\""), "{body}");
    assert!(body.contains("Remember a fact"), "{body}");
    assert!(help.contains("/remember"), "{help}");
    assert_eq!(
        slash_name("/remember@smartagent_bot fact"),
        Some("remember")
    );
    let usage = slash_command("/remember", "50020485", "", "u1")
        .expect("recognized")
        .unwrap();
    assert!(usage.contains("Usage: /remember"), "{usage}");
}

#[test]
fn command_permissions_are_classified() {
    assert_eq!(
        super::command_class("help"),
        Some(super::CommandClass::UserSafe)
    );
    assert_eq!(
        super::command_class("remember"),
        Some(super::CommandClass::ChatScoped)
    );
    assert_eq!(
        super::command_class("model"),
        Some(super::CommandClass::AdminOnly)
    );
    assert_eq!(
        super::command_class("status"),
        Some(super::CommandClass::AdminOnly)
    );
}

#[test]
fn unauthorized_chat_gets_safe_denial_without_state() {
    let out = slash_command("/board", "not-allowed", "", "u1")
        .expect("recognized")
        .unwrap();
    assert_eq!(out, super::safe_denial());
    assert!(!out.contains("READY"), "{out}");
    assert!(!out.contains("T-"), "{out}");
}

#[test]
fn admin_only_commands_require_admin_when_admins_configured() {
    assert!(super::authorize_slash_command_with_lists(
        "model",
        "chat-a",
        "admin-user",
        "chat-a",
        "admin-user",
        ""
    )
    .is_ok());
    assert!(super::authorize_slash_command_with_lists(
        "model",
        "chat-a",
        "normal-user",
        "chat-a",
        "admin-user",
        ""
    )
    .is_err());
    assert!(super::authorize_slash_command_with_lists(
        "remember",
        "chat-a",
        "normal-user",
        "chat-a",
        "admin-user",
        ""
    )
    .is_ok());
}

#[test]
fn model_command_lists_keyboard_and_stores_scope_local_preference() {
    let body = command_menu_body();
    let help = command_help();
    assert!(body.contains("\"command\":\"model\""), "{body}");
    assert!(help.contains("/model"), "{help}");
    assert_eq!(
        super::normalize_model_choice("1"),
        Some(super::TELEGRAM_MODELS[0])
    );
    assert_eq!(
        super::normalize_model_choice(super::TELEGRAM_MODELS[1]),
        Some(super::TELEGRAM_MODELS[1])
    );
    let markup = super::model_menu_markup();
    assert!(markup.contains("inline_keyboard"), "{markup}");
    assert!(markup.contains("model:"), "{markup}");

    let mut db = test_db("telegram-model-pref");
    super::set_model_preference_in_db(&mut db, "chat-a", "main", "u1", super::TELEGRAM_MODELS[1])
        .unwrap();
    assert_eq!(
        super::selected_model_from_db(&db, "chat-a", "main", "u1").as_deref(),
        Some(super::TELEGRAM_MODELS[1])
    );
    assert!(super::selected_model_from_db(&db, "chat-a", "main", "u2").is_none());
    assert!(super::selected_model_from_db(&db, "chat-b", "main", "u1").is_none());
}

#[test]
fn model_callback_is_scoped_and_stale_protected() {
    let now = 100_000;
    assert_eq!(
        super::callback_model_choice(&format!("model:{}", super::TELEGRAM_MODELS[0]), now, now),
        Some(super::TELEGRAM_MODELS[0])
    );
    assert_eq!(super::callback_model_choice("other:data", now, now), None);
    assert_eq!(
        super::callback_model_choice(
            &format!("model:{}", super::TELEGRAM_MODELS[0]),
            now - super::CALLBACK_MAX_AGE_SECS - 1,
            now
        ),
        None
    );
}

#[test]
fn model_callback_selection_returns_confirmation() {
    let ok = super::set_model_preference_in_db;
    let mut db = test_db("telegram-model-callback-confirm");
    ok(
        &mut db,
        "chat-a",
        "topic-1",
        "u1",
        super::TELEGRAM_MODELS[2],
    )
    .unwrap();
    assert_eq!(
        super::selected_model_from_db(&db, "chat-a", "topic-1", "u1").as_deref(),
        Some(super::TELEGRAM_MODELS[2])
    );
    assert!(
        super::set_model_preference("chat-a", "topic-1", "u1", "999")
            .unwrap()
            .contains("Choose a model")
    );
}

#[test]
fn chunks_long_messages() {
    let s = "x".repeat(9000);
    let c = chunks(&s, 4096);
    assert_eq!(c.len(), 3);
    assert!(c.iter().all(|x| x.chars().count() <= 4096));
}

#[test]
fn gateway_prompt_names_current_chat_scope() {
    let p1 = build_gateway_prompt("oli", "u1", "hello", "chat-a", "thread-1");
    let p2 = build_gateway_prompt("oli", "u1", "hello", "chat-b", "thread-2");
    assert!(p1.contains("chat/thread chat-a/thread-1"), "{p1}");
    assert!(p2.contains("chat/thread chat-b/thread-2"), "{p2}");
    assert!(!p1.contains("chat-b"), "{p1}");
    assert!(!p2.contains("chat-a"), "{p2}");
}

#[test]
fn verbosity_command_is_listed_and_stores_scope_local_preference() {
    let help = command_help();
    assert!(help.contains("/verbosity"), "{help}");
    assert!(help.contains("/verbose"), "{help}");
    assert_eq!(
        slash_name("/verbosity@smartagent_bot quiet"),
        Some("verbosity")
    );
    assert_eq!(slash_name("/verbose@smartagent_bot quiet"), Some("verbose"));
    assert_eq!(
        super::command_class("verbose"),
        Some(super::CommandClass::ChatScoped)
    );
    let out = slash_command("/verbose quiet", "50020485", "alias-thread", "alias-user")
        .expect("slash command handled")
        .expect("slash command ok");
    assert!(out.contains("Telegram verbosity set to quiet"), "{out}");
    assert_eq!(
        super::verbosity("50020485", "alias-thread", "alias-user"),
        super::VerbosityLevel::Quiet
    );
    let mut db = test_db("telegram-verbosity-pref");
    super::set_verbosity_in_db(
        &mut db,
        "chat-a",
        "main",
        "u1",
        super::VerbosityLevel::Quiet,
    )
    .unwrap();
    assert_eq!(
        super::verbosity_from_db(&db, "chat-a", "main", "u1"),
        super::VerbosityLevel::Quiet
    );
    assert_eq!(
        super::verbosity_from_db(&db, "chat-a", "main", "u2"),
        super::VerbosityLevel::Normal
    );
    assert_eq!(
        super::verbosity_from_db(&db, "chat-b", "main", "u1"),
        super::VerbosityLevel::Normal
    );
}

#[test]
fn verbosity_filters_progress_task_workflow_notifications() {
    let mut db = test_db("telegram-verbosity-filter");
    super::set_verbosity_in_db(&mut db, "chat-a", "", "*", super::VerbosityLevel::Quiet).unwrap();
    assert_eq!(
        super::verbosity_from_db(&db, "chat-a", "", "*"),
        super::VerbosityLevel::Quiet
    );
    assert!(!super::notification_allowed_in_db(
        &db, "chat-a", "", "*", "progress"
    ));
    assert!(!super::notification_allowed_in_db(
        &db, "chat-a", "", "*", "task"
    ));
    assert!(!super::notification_allowed_in_db(
        &db, "chat-a", "", "*", "workflow"
    ));
    assert!(super::notification_allowed_in_db(
        &db, "chat-b", "", "*", "task"
    ));
}
