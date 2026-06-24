use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use ratatui::{backend::TestBackend, Terminal};

use viztokens::model::{Message, MessageType};
use viztokens::tui::widgets::message_list::render_message_list;
use viztokens::watcher::parser::parse_line;

fn make_messages(n: usize) -> Vec<Message> {
    let types = [
        MessageType::User,
        MessageType::Assistant,
        MessageType::ToolCall,
        MessageType::ToolResult,
        MessageType::System,
    ];
    (0..n)
        .map(|i| Message {
            id: Some(i as i64),
            session_id: "bench-session".to_string(),
            sequence_num: i as u64,
            message_type: types[i % types.len()].clone(),
            timestamp: chrono::Utc::now(),
            content: "x".repeat(200),
            tool_name: if i % 5 == 2 {
                Some("bash".to_string())
            } else {
                None
            },
            tool_use_id: None,
            anthropic_msg_id: None,
            request_id: None,
            input_tokens: Some(10),
            output_tokens: Some(5),
            model: Some("claude-sonnet-4-6".to_string()),
        })
        .collect()
}

fn bench_render(c: &mut Criterion) {
    let mut group = c.benchmark_group("render");
    for size in [100usize, 1000, 10000] {
        let messages = make_messages(size);
        let refs: Vec<(&Message, u16)> = messages
            .iter()
            .map(|message| {
                (
                    message,
                    viztokens::tui::widgets::message_list::message_height(message, 200),
                )
            })
            .collect();
        group.bench_with_input(BenchmarkId::from_parameter(size), &refs, |b, msgs| {
            let backend = TestBackend::new(200, 50);
            let mut terminal = Terminal::new(backend).unwrap();
            b.iter(|| {
                terminal
                    .draw(|f| {
                        render_message_list(
                            msgs,
                            f.area(),
                            f,
                            0,
                            &std::collections::HashMap::new(),
                        );
                    })
                    .unwrap();
            });
        });
    }
    group.finish();
}

fn bench_parse(c: &mut Criterion) {
    let fixtures = vec![
        r#"{"sessionId":"s","timestamp":"2026-06-23T12:00:00Z","message":{"role":"user","content":[{"type":"text","text":"What files are here?"}]}}"#,
        r#"{"sessionId":"s","timestamp":"2026-06-23T12:00:01Z","message":{"id":"m1","role":"assistant","content":[{"type":"text","text":"Here are the files."}],"model":"claude-sonnet-4-6","usage":{"input_tokens":10,"output_tokens":5}}}"#,
        r#"{"sessionId":"s","timestamp":"2026-06-23T12:00:02Z","requestId":"r1","message":{"id":"m2","role":"assistant","content":[{"type":"tool_use","id":"tu1","name":"bash","input":{"command":"ls"}}],"model":"claude-sonnet-4-6","usage":{"input_tokens":15,"output_tokens":8}}}"#,
        r#"{"sessionId":"s","timestamp":"2026-06-23T12:00:03Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu1","content":"file.txt"}]}}"#,
    ];

    let mut group = c.benchmark_group("parse");
    for &size in &[1000usize, 10000] {
        let lines: Vec<&str> = (0..size).map(|i| fixtures[i % fixtures.len()]).collect();
        group.bench_with_input(BenchmarkId::from_parameter(size), &lines, |b, ls| {
            b.iter(|| {
                for (i, line) in ls.iter().enumerate() {
                    let _ = parse_line(line, "bench-session", i as u64);
                }
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_render, bench_parse);
criterion_main!(benches);
