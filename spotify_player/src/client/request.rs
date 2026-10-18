use crate::state::{
    AlbumId, Category, ContextId, Item, ItemId, PlayableId, Playback, PlaylistId, TrackId,
};

#[derive(Clone, Debug)]
/// A request that modifies the player's playback
pub enum PlayerRequest {
    NextTrack,
    PreviousTrack,
    Resume,
    Pause,
    ResumePause,
    SeekTrack(chrono::Duration),
    Repeat,
    Shuffle,
    Volume(u8),
    ToggleMute,
    TransferPlayback(String, bool),
    StartPlayback(Playback, Option<bool>),
}

#[derive(Clone, Debug)]
/// A request to the client
pub enum ClientRequest {
    GetCurrentUser,
    GetDevices,
    GetBrowseCategories,
    GetBrowseCategoryPlaylists(Category),
    GetUserPlaylists,
    GetUserSavedAlbums,
    GetUserSavedShows,
    GetUserFollowedArtists,
    GetUserTopArtists,
    GetContext(ContextId),
    GetCurrentPlayback,
    Search(String),
    AddPlayableToQueue(PlayableId<'static>),
    AddAlbumToQueue(AlbumId<'static>),
    AddPlayableToPlaylist(PlaylistId<'static>, PlayableId<'static>),
    DeleteTrackFromPlaylist(PlaylistId<'static>, TrackId<'static>),
    ReorderPlaylistItems {
        playlist_id: PlaylistId<'static>,
        insert_index: usize,
        range_start: usize,
        range_length: Option<usize>,
        snapshot_id: Option<String>,
    },
    AddToLibrary(Item),
    DeleteFromLibrary(ItemId),
    Player(PlayerRequest),
    GetCurrentUserQueue,
    GetLyrics {
        track_id: TrackId<'static>,
    },
    #[cfg(feature = "streaming")]
    RestartIntegratedClient,
    CreatePlaylist {
        playlist_name: String,
        public: bool,
        collab: bool,
        desc: String,
    },
}

impl ClientRequest {
    /// A short description of a user-triggered request, `None` for background data fetches
    pub fn action_label(&self) -> Option<String> {
        let label = match self {
            Self::Player(request) => match request {
                PlayerRequest::NextTrack => "Next track",
                PlayerRequest::PreviousTrack => "Previous track",
                PlayerRequest::Resume => "Resume",
                PlayerRequest::Pause => "Pause",
                PlayerRequest::ResumePause => "Play/pause",
                PlayerRequest::SeekTrack(_) => "Seek",
                PlayerRequest::Repeat => "Repeat",
                PlayerRequest::Shuffle => "Shuffle",
                PlayerRequest::Volume(volume) => return Some(format!("Volume {volume}%")),
                PlayerRequest::ToggleMute => "Mute",
                PlayerRequest::TransferPlayback(..) => "Switch device",
                PlayerRequest::StartPlayback(..) => "Play",
            },
            Self::Search(query) => return Some(format!("Search \"{query}\"")),
            Self::AddPlayableToQueue(_) | Self::AddAlbumToQueue(_) => "Add to queue",
            Self::AddPlayableToPlaylist(..) => "Add to playlist",
            Self::DeleteTrackFromPlaylist(..) => "Remove from playlist",
            Self::ReorderPlaylistItems { .. } => "Reorder playlist",
            Self::AddToLibrary(_) => "Save to library",
            Self::DeleteFromLibrary(_) => "Remove from library",
            Self::CreatePlaylist { playlist_name, .. } => {
                return Some(format!("Create playlist \"{playlist_name}\""))
            }
            _ => return None,
        };
        Some(label.to_string())
    }
}
