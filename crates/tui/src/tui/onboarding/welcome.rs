//! Welcome screen content for onboarding.
//! Width-aware: uses full ASCII art on wide terminals, compact text on narrow ones.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::palette;

pub fn lines() -> Vec<Line<'static>> {
    full_lines()
}

/// Full welcome screen with ASCII art (needs ~48 columns).
fn full_lines() -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        Line::from(Span::styled(
            "  ╔══════════════════════════════════════╗",
            Style::default().fg(palette::DEEPSEEK_BLUE),
        )),
        Line::from(Span::styled(
            "  ║                                      ║",
            Style::default().fg(palette::DEEPSEEK_BLUE),
        )),
        Line::from(Span::styled(
            "  ║     ██████  ███████  ██████  ██████  ║",
            Style::default().fg(palette::DEEPSEEK_SKY).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  ║     ██   ██ ██      ██      ██      ║",
            Style::default().fg(palette::DEEPSEEK_SKY).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  ║     ██   ██ █████   ██████  █████   ║",
            Style::default().fg(palette::DEEPSEEK_BLUE).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  ║     ██   ██ ██          ██ ██      ║",
            Style::default().fg(palette::DEEPSEEK_BLUE).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  ║     ██████  ███████ ██████  ██████  ║",
            Style::default().fg(palette::DEEPSEEK_SKY).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  ║                                      ║",
            Style::default().fg(palette::DEEPSEEK_BLUE),
        )),
        Line::from(Span::styled(
            "  ╚══════════════════════════════════════╝",
            Style::default().fg(palette::DEEPSEEK_BLUE),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("  v{}", env!("CARGO_PKG_VERSION")),
            Style::default().fg(palette::TEXT_MUTED),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Terminal-native AI coding agent for DeepSeek models.",
            Style::default().fg(palette::TEXT_PRIMARY),
        )),
        Line::from(Span::styled(
            "  Multi-line composer · Plan/Agent/YOLO · Sub-agents · MCP · Skills",
            Style::default().fg(palette::TEXT_MUTED),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Press Enter to start setup.  Ctrl+C to exit.",
            Style::default().fg(palette::TEXT_PRIMARY),
        )),
    ]
}

/// Compact fallback for narrow terminals (<48 cols). Caller should check width.
#[allow(dead_code)]
pub fn compact_lines() -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            "DeepSeek TUI",
            Style::default().fg(palette::DEEPSEEK_BLUE).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("v{}", env!("CARGO_PKG_VERSION")),
            Style::default().fg(palette::TEXT_MUTED),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Terminal-native AI coding agent for DeepSeek models.",
            Style::default().fg(palette::TEXT_PRIMARY),
        )),
        Line::from(Span::styled(
            "Plan / Agent / YOLO · Sub-agents · MCP · Skills · Hooks",
            Style::default().fg(palette::TEXT_MUTED),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Press Enter to start. Ctrl+C to exit.",
            Style::default().fg(palette::TEXT_PRIMARY),
        )),
    ]
}
