use crate::{
    config::{self, Theme},
    key,
    ui::{self, Orientation},
    utils::filtered_items_from_query,
};

#[cfg(feature = "image")]
use crate::ui::cover_image::CoverImage;
#[cfg(feature = "image")]
use ratatui_image::picker::Picker;

pub type UIStateGuard<'a> = parking_lot::MutexGuard<'a, UIState>;

mod page;
mod popup;

pub use page::*;
pub use popup::*;

#[cfg(feature = "image")]
#[derive(Default)]
pub struct ImageRenderInfo {
    pub url: String,
    pub render_area: ratatui::layout::Rect,
    pub state: Option<CoverImage>,
}

#[cfg(feature = "image")]
impl std::fmt::Debug for ImageRenderInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageRenderInfo")
            .field("url", &self.url)
            .field("render_area", &self.render_area)
            .field("state", &self.state.is_some())
            .finish()
    }
}

/// Application's UI state
#[derive(Debug)]
pub struct UIState {
    pub is_running: bool,
    pub theme: config::Theme,
    pub input_key_sequence: key::KeySequence,
    pub orientation: ui::Orientation,

    pub history: Vec<PageState>,
    pub popup: Option<PopupState>,

    /// The rectangle representing the playback progress bar,
    /// which is mainly used to handle mouse click events (for seeking command)
    pub playback_progress_bar_rect: ratatui::layout::Rect,

    /// Count prefix for vim-style navigation (e.g., 5j, 10k)
    pub count_prefix: Option<usize>,

    /// Clickable areas of the last rendered frame, used to route mouse events
    pub click_regions: Vec<ClickRegion>,
    /// Time and position of the last left click, used to detect double clicks
    pub last_click: Option<(std::time::Instant, u16, u16)>,
    /// Last known mouse position, used to highlight what's under the cursor
    pub hover: Option<(u16, u16)>,
    /// Outcome of the latest user action, shown in the status line
    pub status: Option<ActionStatus>,

    #[cfg(feature = "image")]
    pub last_cover_image_render_info: ImageRenderInfo,

    #[cfg(feature = "image")]
    pub picker: Picker,
}

impl UIState {
    pub fn current_page(&self) -> &PageState {
        self.history.last().expect("non-empty history")
    }

    pub fn current_page_mut(&mut self) -> &mut PageState {
        self.history.last_mut().expect("non-empty history")
    }

    pub fn new_search_popup(&mut self) {
        self.current_page_mut().select(0);
        self.popup = Some(PopupState::Search {
            query: String::new(),
        });
    }

    pub fn new_page(&mut self, page: PageState) {
        self.popup = None;
        if let Some(current_page) = self.history.last() {
            if &page == current_page {
                return;
            }
        }
        self.history.push(page);
    }

    /// Return whether there exists a focused popup.
    ///
    /// Currently, only search popup is not focused when it's opened.
    pub fn has_focused_popup(&self) -> bool {
        match self.popup.as_ref() {
            None => false,
            Some(popup) => !matches!(popup, PopupState::Search { .. }),
        }
    }

    pub fn push_click_region(&mut self, rect: Rect, target: ClickTarget) {
        self.click_regions.push(ClickRegion { rect, target });
    }

    /// The click region at a terminal position. Later regions win, as they are drawn on top.
    pub fn click_region_at(&self, column: u16, row: u16) -> Option<ClickRegion> {
        let pos = ratatui::layout::Position::new(column, row);
        self.click_regions
            .iter()
            .rev()
            .find(|r| r.rect.contains(pos))
            .copied()
    }

    /// Get a list of items possibly filtered by a search query if exists a search popup
    pub fn search_filtered_items<'a, T: std::fmt::Display>(&self, items: &'a [T]) -> Vec<&'a T> {
        match self.popup {
            Some(PopupState::Search { ref query }) => filtered_items_from_query(query, items),
            _ => items.iter().collect::<Vec<_>>(),
        }
    }
}

use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy)]
pub struct ClickRegion {
    pub rect: Rect,
    pub target: ClickTarget,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClickTarget {
    /// A list/table window of the current page. `header` is the number of
    /// non-item rows (e.g. a table header) above the first item.
    Window {
        focus: WindowFocus,
        len: usize,
        header: u16,
    },
    /// The list inside the current popup
    PopupList {
        len: usize,
    },
    SearchInput,
    Button(crate::command::Command),
    /// The playback window, where scrolling changes the volume
    Playback,
}

/// Which window of the current page a click region belongs to
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowFocus {
    /// The page has a single window
    Single,
    Library(LibraryFocusState),
    Search(SearchFocusState),
    Artist(ArtistFocusState),
}

impl Default for UIState {
    fn default() -> Self {
        Self {
            is_running: true,
            theme: Theme::default(),
            input_key_sequence: key::KeySequence { keys: vec![] },
            orientation: match crossterm::terminal::size() {
                Ok((columns, rows)) => ui::Orientation::from_size(columns, rows),
                Err(err) => {
                    tracing::warn!("Unable to get terminal size, error: {err:#}");
                    Orientation::default()
                }
            },

            history: vec![PageState::Library {
                state: LibraryPageUIState::new(),
            }],
            popup: None,

            playback_progress_bar_rect: Rect::default(),

            count_prefix: None,

            click_regions: Vec::new(),
            last_click: None,
            hover: None,
            status: None,

            #[cfg(feature = "image")]
            last_cover_image_render_info: ImageRenderInfo::default(),

            // Will be reinitialize later in ui/mod.rs after init_ui()
            #[cfg(feature = "image")]
            picker: Picker::halfblocks(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActionStatus {
    /// Identifies the request, so a slow earlier request can't overwrite a newer status
    pub id: u64,
    pub text: String,
    pub kind: StatusKind,
    pub at: std::time::Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Pending,
    Done,
    Failed,
}
