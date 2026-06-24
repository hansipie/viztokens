use std::collections::HashMap;

use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Span,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::model::{Message, MessageType};

fn type_color(mt: &MessageType) -> Color {
    match mt {
        MessageType::System => Color::DarkGray,
        MessageType::User => Color::Blue,
        MessageType::Assistant => Color::Green,
        MessageType::ToolCall => Color::Yellow,
        MessageType::ToolResult => Color::Cyan,
    }
}

fn type_label(mt: &MessageType) -> &'static str {
    match mt {
        MessageType::System => "SYSTEM",
        MessageType::User => "USER",
        MessageType::Assistant => "ASSISTANT",
        MessageType::ToolCall => "TOOL CALL",
        MessageType::ToolResult => "TOOL RESULT",
    }
}

/// Returns the number of lines a message occupies (border + content lines).
pub fn message_height(msg: &Message, width: u16) -> u16 {
    // Keep height calculation on the same rendering path as the widget itself.
    // `chars().count()` is not a terminal width: CJK characters and emoji can
    // occupy two cells, while combining marks can occupy none. A mismatched
    // height makes adjacent paragraphs overlap and leaves stale characters.
    let content_lines = Paragraph::new(msg.content.as_str())
        .wrap(Wrap { trim: false })
        .line_count(width.saturating_sub(2))
        .max(1);

    content_lines.saturating_add(2).min(u16::MAX as usize) as u16
}

pub fn render_message_list(
    messages: &[(&Message, u16)],
    area: Rect,
    frame: &mut Frame,
    scroll_offset: usize,
    session_projects: &HashMap<String, String>,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let mut cumulative = 0usize;
    let mut y = area.y;
    let bottom = area.y + area.height;

    for &(msg, height) in messages {
        let h = height as usize;
        let msg_start = cumulative;
        let msg_end = cumulative + h;
        cumulative = msg_end;

        // Skip messages entirely above the viewport
        if msg_end <= scroll_offset {
            continue;
        }
        // Stop if we've rendered past the viewport
        if y >= bottom {
            break;
        }

        let color = type_color(&msg.message_type);
        let label = type_label(&msg.message_type);
        let tool_suffix = msg
            .tool_name
            .as_deref()
            .map(|n| format!("  {}", n))
            .unwrap_or_default();
        let ts = msg.timestamp.format("%H:%M:%S").to_string();
        let project = session_projects
            .get(&msg.session_id)
            .map(|p| format!("{}  ", p))
            .unwrap_or_default();
        let model_suffix = msg
            .model
            .as_deref()
            .map(|m| format!("  {}", m))
            .unwrap_or_default();
        let token_suffix = match (msg.input_tokens, msg.output_tokens) {
            (Some(i), Some(o)) => format!(
                "  in:{} out:{}",
                super::fmt_num(i as u64),
                super::fmt_num(o as u64)
            ),
            (Some(i), None) => format!("  in:{}", super::fmt_num(i as u64)),
            (None, Some(o)) => format!("  out:{}", super::fmt_num(o as u64)),
            (None, None) => String::new(),
        };
        let title = format!(
            " {}{}  {} {}{}{}",
            project, label, ts, tool_suffix, model_suffix, token_suffix
        );

        // How many virtual lines of this message are above the viewport
        let skip_lines = scroll_offset.saturating_sub(msg_start) as u16;

        // Available height for this message in the viewport
        let available = (bottom - y).min(h.saturating_sub(skip_lines as usize) as u16);
        let msg_rect = Rect {
            x: area.x,
            y,
            width: area.width,
            height: available,
        };

        // When the top border is scrolled off, drop it so it doesn't bleed into the viewport.
        // para_scroll counts content lines to skip (border line already removed from the count).
        let (borders, para_scroll) = if skip_lines == 0 {
            (Borders::ALL, 0u16)
        } else {
            (
                Borders::LEFT | Borders::RIGHT | Borders::BOTTOM,
                skip_lines - 1,
            )
        };

        let block = if skip_lines == 0 {
            Block::default()
                .borders(borders)
                .border_style(Style::default().fg(color))
                .title(Span::styled(title, Style::default().fg(color)))
        } else {
            Block::default()
                .borders(borders)
                .border_style(Style::default().fg(color))
        };

        let paragraph = Paragraph::new(msg.content.as_str())
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((para_scroll, 0));

        frame.render_widget(paragraph, msg_rect);
        y += available;
    }

    // Clear any unused space below the last message
    if y < bottom {
        frame.render_widget(
            Clear,
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: bottom - y,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    fn message(content: &str) -> Message {
        Message {
            id: None,
            session_id: "session".to_string(),
            sequence_num: 1,
            message_type: MessageType::User,
            timestamp: Utc.timestamp_opt(0, 0).unwrap(),
            content: content.to_string(),
            tool_name: None,
            tool_use_id: None,
            anthropic_msg_id: None,
            request_id: None,
            input_tokens: None,
            output_tokens: None,
            model: None,
        }
    }

    #[test]
    fn message_height_uses_terminal_cell_width_for_wide_characters() {
        // Width 6 leaves four cells inside the borders. Each CJK character
        // occupies two cells, so three characters wrap onto two content lines.
        assert_eq!(message_height(&message("界界界"), 6), 4);
    }

    #[test]
    fn message_height_matches_paragraph_wrapping() {
        assert_eq!(message_height(&message("one two three"), 8), 5);
    }

    #[test]
    fn buffer_diff_clears_wide_emoji_trailing_cell() {
        use ratatui::{buffer::Buffer, style::Style};

        let previous = Buffer::with_lines(["ab"]);
        let mut next = Buffer::with_lines(["  "]);
        next.set_string(0, 0, "⌨️", Style::new());

        let updates = previous.diff(&next);
        assert!(
            updates
                .iter()
                .any(|(x, y, cell)| (*x, *y, cell.symbol()) == (1, 0, " ")),
            "the hidden trailing cell must be explicitly cleared: {updates:?}"
        );
    }
}
