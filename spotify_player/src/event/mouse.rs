//! Mouse support: clicks, double clicks, right clicks and scrolling on the regions
//! recorded by the UI during the last render

use super::{
    config, handle_global_command, handle_key_sequence, page, ClientRequest, Command, ContextId,
    Id, PageType, Playback, PlayerRequest, Result, SearchFocusState, SharedState, UIStateGuard,
    USER_LIKED_TRACKS_ID,
};
use crate::state::{ClickTarget, MutableWindowState, PageState, WindowFocus};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use rand::RngExt;

const DOUBLE_CLICK_INTERVAL: std::time::Duration = std::time::Duration::from_millis(400);
const SCROLL_LINES: usize = 3;

pub fn handle_mouse_event(
    event: MouseEvent,
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
) -> Result<()> {
    // moves arrive on every cell the cursor crosses, so skip the logging for them
    if event.kind == MouseEventKind::Moved {
        state.ui.lock().hover = Some((event.column, event.row));
        return Ok(());
    }
    tracing::debug!("Handling mouse event: {event:?}");
    let (col, row) = (event.column, event.row);

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if seek_on_progress_bar(col, row, client_pub, state)? {
                return Ok(());
            }
            let mut ui = state.ui.lock();
            let Some(region) = ui.click_region_at(col, row) else {
                return Ok(());
            };
            let now = std::time::Instant::now();
            let is_double = matches!(ui.last_click,
                Some((t, _, r)) if r == row && now.duration_since(t) < DOUBLE_CLICK_INTERVAL);
            ui.last_click = if is_double {
                None
            } else {
                Some((now, col, row))
            };

            match region.target {
                ClickTarget::Button(Command::PlayRandom) => {
                    play_random(client_pub, state, &mut ui)?;
                }
                ClickTarget::Button(command) => {
                    handle_global_command(command, client_pub, state, &mut ui, None)?;
                }
                ClickTarget::Window { focus, len, header } => {
                    if select_in_window(&mut ui, focus, region.rect, row, len, header) && is_double
                    {
                        run_command(Command::ChooseSelected, client_pub, state, &mut ui)?;
                    }
                }
                ClickTarget::PopupList { len } => {
                    let id = ui
                        .popup
                        .as_mut()
                        .and_then(|p| p.list_state_mut())
                        .map(|s| s.offset() + usize::from(row - region.rect.y));
                    if let Some(id) = id.filter(|id| *id < len) {
                        ui.popup.as_mut().unwrap().list_select(Some(id));
                        run_command(Command::ChooseSelected, client_pub, state, &mut ui)?;
                    }
                }
                ClickTarget::SearchInput => {
                    if ui.has_focused_popup() {
                        ui.popup = None;
                    }
                    if let PageState::Search { state, .. } = ui.current_page_mut() {
                        state.focus = SearchFocusState::Input;
                    }
                }
                ClickTarget::Playback => {}
            }
        }
        MouseEventKind::Down(MouseButton::Right) => {
            let mut ui = state.ui.lock();
            if let Some(region) = ui.click_region_at(col, row) {
                if let ClickTarget::Window { focus, len, header } = region.target {
                    if select_in_window(&mut ui, focus, region.rect, row, len, header) {
                        run_command(
                            Command::ShowActionsOnSelectedItem,
                            client_pub,
                            state,
                            &mut ui,
                        )?;
                    }
                }
            }
        }
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            let up = event.kind == MouseEventKind::ScrollUp;
            let mut ui = state.ui.lock();
            match ui.click_region_at(col, row).map(|r| r.target) {
                Some(ClickTarget::Playback) => change_volume(up, client_pub, state)?,
                Some(ClickTarget::Window { focus, len, .. }) => {
                    if ui.current_page_mut().set_focus(focus) {
                        if let Some(mut window) = ui.current_page_mut().focus_window_state_mut() {
                            let id = scrolled(window.selected().unwrap_or(0), up, len);
                            window.select(id);
                        }
                    }
                }
                Some(ClickTarget::PopupList { len }) => {
                    if let Some(popup) = ui.popup.as_mut() {
                        let id = scrolled(popup.list_selected().unwrap_or(0), up, len);
                        popup.list_select(Some(id));
                    }
                }
                Some(_) => {}
                None => {
                    // pages rendered as plain text (queue, help, logs) keep a scroll offset
                    if let Some(MutableWindowState::Scroll(offset)) =
                        ui.current_page_mut().focus_window_state_mut()
                    {
                        *offset = if up {
                            offset.saturating_sub(SCROLL_LINES)
                        } else {
                            *offset + SCROLL_LINES
                        };
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn scrolled(id: usize, up: bool, len: usize) -> usize {
    if up {
        id.saturating_sub(SCROLL_LINES)
    } else {
        (id + SCROLL_LINES).min(len.saturating_sub(1))
    }
}

/// Focus the clicked window and select the item under the cursor. Returns whether an item was selected.
fn select_in_window(
    ui: &mut UIStateGuard,
    focus: WindowFocus,
    rect: ratatui::layout::Rect,
    row: u16,
    len: usize,
    header: u16,
) -> bool {
    if ui.has_focused_popup() {
        ui.popup = None;
    }
    if !ui.current_page_mut().set_focus(focus) || row < rect.y + header {
        return false;
    }
    let Some(mut window) = ui.current_page_mut().focus_window_state_mut() else {
        return false;
    };
    let id = window.offset() + usize::from(row - rect.y - header);
    if id >= len {
        return false;
    }
    window.select(id);
    true
}

/// Run a command as if the user pressed its first bound shortcut
fn run_command(
    command: Command,
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
    ui: &mut UIStateGuard,
) -> Result<bool> {
    let keymap = config::get_config()
        .keymap_config
        .keymaps
        .iter()
        .find(|k| k.command == command);
    match keymap {
        Some(k) => handle_key_sequence(&k.key_sequence.clone(), client_pub, state, ui, None),
        None => handle_global_command(command, client_pub, state, ui, None),
    }
}

/// Play a random track: from the open track list if there is one, otherwise from the
/// playing context or the user's liked songs
fn play_random(
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
    ui: &mut UIStateGuard,
) -> Result<()> {
    if ui.popup.is_none() && ui.current_page().page_type() == PageType::Context {
        if let Some(k) = config::get_config()
            .keymap_config
            .keymaps
            .iter()
            .find(|k| k.command == Command::PlayRandom)
        {
            if page::handle_key_sequence_for_page(&k.key_sequence.clone(), client_pub, state, ui)? {
                return Ok(());
            }
        }
    }

    let limit = config::get_config().app_config.tracks_playback_limit;
    let context_id = state.player.read().playing_context_id();
    let data = state.data.read();

    let from_context = context_id.and_then(|id| {
        let tracks = data.context_tracks(&id).filter(|t| !t.is_empty())?;
        let uri = tracks[rand::rng().random_range(0..tracks.len())].id.uri();
        let playback = match id {
            ContextId::Album(_) | ContextId::Playlist(_) => Playback::Context(id, None),
            ContextId::Show(_) => return None,
            _ => Playback::URIs(tracks.iter().map(|t| t.id.clone().into()).collect(), None),
        };
        Some(playback.uri_offset(uri, limit))
    });

    let playback = if let Some(p) = from_context {
        p
    } else {
        {
            let tracks: Vec<_> = data.user_data.saved_tracks.values().collect();
            if tracks.is_empty() {
                return Ok(());
            }
            let uri = tracks[rand::rng().random_range(0..tracks.len())].id.uri();
            state.player.write().currently_playing_tracks_id = Some(USER_LIKED_TRACKS_ID.clone());
            Playback::URIs(tracks.iter().map(|t| t.id.clone().into()).collect(), None)
                .uri_offset(uri, limit)
        }
    };
    client_pub.send(ClientRequest::Player(PlayerRequest::StartPlayback(
        playback, None,
    )))?;
    Ok(())
}

fn change_volume(
    up: bool,
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
) -> Result<()> {
    let step = config::get_config().app_config.volume_scroll_step;
    if let Some(volume) = state
        .player
        .read()
        .buffered_playback
        .as_ref()
        .and_then(|p| p.volume)
    {
        let volume = volume as u8;
        let new_volume = if up {
            volume.saturating_add(step).min(100)
        } else {
            volume.saturating_sub(step)
        };
        client_pub.send(ClientRequest::Player(PlayerRequest::Volume(new_volume)))?;
    }
    Ok(())
}

/// Seek when the progress bar is clicked. Returns whether the click hit the bar.
fn seek_on_progress_bar(
    col: u16,
    row: u16,
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
) -> Result<bool> {
    let rect = state.ui.lock().playback_progress_bar_rect;
    if row != rect.y || col < rect.x || col >= rect.x + rect.width {
        return Ok(false);
    }
    // calculate the seek position (in ms) based on the mouse click position,
    // the progress bar's width and the track's duration (in ms)
    let player = state.player.read();
    let duration = match player.currently_playing() {
        Some(rspotify::model::PlayableItem::Track(track)) => Some(track.duration),
        Some(rspotify::model::PlayableItem::Episode(episode)) => Some(episode.duration),
        Some(rspotify::model::PlayableItem::Unknown(_)) | None => None,
    };
    if let Some(duration) = duration {
        let position_ms =
            duration.num_milliseconds() * i64::from(col - rect.x) / i64::from(rect.width);
        client_pub.send(ClientRequest::Player(PlayerRequest::SeekTrack(
            chrono::Duration::try_milliseconds(position_ms).unwrap(),
        )))?;
    }
    Ok(true)
}
