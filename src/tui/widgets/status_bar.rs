use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};


pub struct StatusBarState<'a> {
    pub watching_count: usize,
    pub session_count: usize,
    pub filter_label: &'a str,
    pub parse_errors: u32,
    pub follow_mode: bool,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
}

pub fn render_header(state: &StatusBarState, area: Rect, frame: &mut Frame) {
    let (status_icon, status_text, status_color) = if state.watching_count > 0 {
        ("●", "WATCHING", Color::Green)
    } else if state.session_count == 0 {
        ("○", "WAITING", Color::DarkGray)
    } else {
        ("□", "ENDED", Color::Red)
    };

    let sessions_label = format!(
        "  {}/{} sessions  ",
        state.watching_count, state.session_count
    );

    let follow_label = if state.follow_mode { "[follow]" } else { "[scroll]" };
    let follow_color = if state.follow_mode { Color::Green } else { Color::Yellow };

    let mut spans = vec![
        Span::raw(" viztokens  "),
        Span::styled(
            format!("{} {}", status_icon, status_text),
            Style::default().fg(status_color),
        ),
        Span::raw(sessions_label),
        Span::styled(follow_label, Style::default().fg(follow_color)),
        Span::raw(format!("  filter: {}", state.filter_label)),
    ];

    if state.parse_errors > 0 {
        spans.push(Span::styled(
            format!("  ⚠ {}", state.parse_errors),
            Style::default().fg(Color::Yellow),
        ));
    }

    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}

pub fn render_footer(state: &StatusBarState, area: Rect, frame: &mut Frame) {
    let token_label = format!(
        "  in:{} out:{}",
        super::fmt_num(state.total_input_tokens),
        super::fmt_num(state.total_output_tokens)
    );
    let line = Line::from(vec![
        Span::raw(" [↑↓] Scroll  [f] Filter  [End] Follow  [q] Quit"),
        Span::styled(token_label, Style::default().fg(Color::DarkGray)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}
