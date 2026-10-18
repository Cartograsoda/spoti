//! A one-line bar of clickable page tabs and playback buttons, for mouse users

use super::{config, Constraint, Frame, Layout, Paragraph, Rect, SharedState, UIStateGuard};
use crate::{
    command::Command,
    state::{
        ClickTarget, ContextId, ContextPageType, PageState, PageType, StatusKind, TracksId,
        USER_LIKED_TRACKS_ID, USER_RECENTLY_PLAYED_TRACKS_ID,
    },
};

/// Render the control bar next to the playback window and return the remaining area
pub fn render_control_bar(
    frame: &mut Frame,
    state: &SharedState,
    ui: &mut UIStateGuard,
    rect: Rect,
) -> Rect {
    if rect.height < 2 {
        return rect;
    }
    let (bar_rect, other_rect) = match config::get_config()
        .app_config
        .layout
        .playback_window_position
    {
        config::Position::Top => {
            let chunks = Layout::vertical([Constraint::Length(1), Constraint::Fill(0)]).split(rect);
            (chunks[0], chunks[1])
        }
        config::Position::Bottom => {
            let chunks = Layout::vertical([Constraint::Fill(0), Constraint::Length(1)]).split(rect);
            (chunks[1], chunks[0])
        }
    };

    let (is_playing, shuffle, repeat) = match state.player.read().buffered_playback {
        Some(ref p) => (p.is_playing, p.shuffle_state, p.repeat_state),
        None => (false, false, rspotify::model::RepeatState::Off),
    };

    let page = ui.current_page();
    let on_tracks_page = |id: &TracksId| {
        matches!(page, PageState::Context {
            context_page_type: ContextPageType::Browsing(ContextId::Tracks(t)), ..
        } if t == id)
    };
    let tabs = [
        (
            "Library",
            Command::LibraryPage,
            page.page_type() == PageType::Library,
        ),
        (
            "Search",
            Command::SearchPage,
            page.page_type() == PageType::Search,
        ),
        (
            "Browse",
            Command::BrowsePage,
            page.page_type() == PageType::Browse,
        ),
        (
            "Liked",
            Command::LikedTrackPage,
            on_tracks_page(&USER_LIKED_TRACKS_ID),
        ),
        (
            "Recent",
            Command::RecentlyPlayedTrackPage,
            on_tracks_page(&USER_RECENTLY_PLAYED_TRACKS_ID),
        ),
        ("Top", Command::TopTrackPage, page.is_top_page()),
        ("Queue", Command::Queue, page.page_type() == PageType::Queue),
        (
            "Lyrics",
            Command::LyricsPage,
            page.page_type() == PageType::Lyrics,
        ),
        ("Back", Command::PreviousPage, false),
    ];
    let controls = [
        ("|<", Command::PreviousTrack, false),
        (
            if is_playing { "Pause" } else { "Play" },
            Command::ResumePause,
            false,
        ),
        (">|", Command::NextTrack, false),
        ("Random", Command::PlayRandom, false),
        ("Shuffle", Command::Shuffle, shuffle),
        (
            match repeat {
                rspotify::model::RepeatState::Track => "Repeat 1",
                _ => "Repeat",
            },
            Command::Repeat,
            repeat != rspotify::model::RepeatState::Off,
        ),
        ("Vol-", Command::VolumeChange { offset: -5 }, false),
        ("Vol+", Command::VolumeChange { offset: 5 }, false),
        ("Devices", Command::SwitchDevice, false),
    ];

    let button_width = |label: &str| label.len() as u16 + 2;
    let controls_width: u16 = controls.iter().map(|(l, ..)| button_width(l) + 1).sum();
    let right_x = bar_rect.x + bar_rect.width;
    let controls_x = right_x.saturating_sub(controls_width).max(bar_rect.x);

    let style = |active: bool| {
        if active {
            ui.theme.selection(true)
        } else {
            ui.theme.table_header()
        }
    };

    let mut buttons = Vec::new();
    // tabs fill the bar from the left, playback controls take priority on the right
    let mut x = bar_rect.x;
    for (label, command, active) in tabs {
        let w = button_width(label);
        if x + w > controls_x {
            break;
        }
        buttons.push((
            Rect::new(x, bar_rect.y, w, 1),
            label,
            command,
            style(active),
        ));
        x += w + 1;
    }
    let mut x = controls_x;
    for (label, command, active) in controls {
        let w = button_width(label);
        if x + w > right_x {
            break;
        }
        buttons.push((
            Rect::new(x, bar_rect.y, w, 1),
            label,
            command,
            style(active),
        ));
        x += w + 1;
    }

    for (rect, label, command, style) in buttons {
        frame.render_widget(Paragraph::new(format!(" {label} ")).style(style), rect);
        ui.push_click_region(rect, ClickTarget::Button(command));
    }

    other_rect
}

/// How long a finished action's outcome stays in the status line
const STATUS_DURATION: std::time::Duration = std::time::Duration::from_secs(6);
const MOUSE_HINT: &str =
    "Double-click: play/open   Right-click: actions   Scroll: move, or volume over the player";

/// Render the status line on the app's last row and return the remaining area
pub fn render_status_line(frame: &mut Frame, ui: &UIStateGuard, rect: Rect) -> Rect {
    use ratatui::style::{Color, Modifier, Style};

    if rect.height < 2 {
        return rect;
    }
    let chunks = Layout::vertical([Constraint::Fill(0), Constraint::Length(1)]).split(rect);

    let (text, style) = match ui.status {
        Some(ref status) if status.kind == StatusKind::Pending => {
            let wait = crate::client::RATE_LIMITED_UNTIL
                .lock()
                .and_then(|until| until.checked_duration_since(std::time::Instant::now()));
            let text = match wait {
                Some(wait) => format!(
                    "{}: waiting for Spotify rate limit ({}s)",
                    status.text,
                    wait.as_secs() + 1
                ),
                None => format!("{}...", status.text),
            };
            (text, Style::default().fg(Color::Yellow))
        }
        Some(ref status) if status.at.elapsed() < STATUS_DURATION => {
            let color = if status.kind == StatusKind::Failed {
                Color::Red
            } else {
                Color::Green
            };
            (status.text.clone(), Style::default().fg(color))
        }
        _ => (
            MOUSE_HINT.to_string(),
            Style::default().add_modifier(Modifier::DIM),
        ),
    };
    frame.render_widget(Paragraph::new(format!(" {text}")).style(style), chunks[1]);

    chunks[0]
}
