use eframe::epaint::Color32;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

#[derive(PartialEq, PartialOrd, Debug, Clone)]
pub enum ControlMessage {
    Play(u64),
    Pause(u64),
    ChangeStem(u64, usize),
    SyncPlaybackStatus,
    Seek(u64, f64),
    Loop(u64, bool),
    Mute(u64, bool),
    SetVolume(u64, f64),
    Delete(u64),
    AddToPlaylist {
        item_id: u64,
        playlist_id: u64,
    },
    RemoveFromPlaylist {
        pos_within_playlist: usize,
        playlist_id: u64,
    },
    PlayFromPlaylist(u64),
    GlobalPause,
    GlobalStop,
}

#[derive(PartialEq, Debug, Clone)]
pub enum ImportMessage {
    Cancelled,
    Update(u64, ItemImportStatus),
    Finished(Vec<Item>),
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone)]
pub enum ItemImportStatus {
    Queued(String),
    Waiting,
    InProgress,
    Finished,
    Failed(String),
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Serialize, Deserialize)]
pub struct Stem {
    pub tag: String,
    pub path: String,
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Serialize, Deserialize)]
pub enum ItemStatus {
    Stopped,
    Loading,
    Playing,
    Paused,
}

#[derive(PartialEq, Eq, Debug, Clone, Serialize, Deserialize)]
pub enum Issue {
    FileNotFound(String),
}

impl PartialOrd for Issue {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(match (self, other) {
            (Issue::FileNotFound(a), Issue::FileNotFound(b)) => a.cmp(b),
        })
    }
}

impl Ord for Issue {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

#[derive(PartialEq, Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: u64,
    pub name: String,
    pub stems: Vec<Stem>,
    pub current_stem: usize,
    pub volume: f64,
    pub muted: bool,
    pub looped: bool,
    pub status: ItemStatus,
    pub colour: Color32,
    pub bars: Vec<u8>,
    /// The position within the track, in seconds.
    ///
    /// This should only ever be read, since it is animated by target_position.
    pub position: f64,
    /// The target (real) position within the track, in seconds.
    ///
    /// This is effectively owned by the playback thread.
    /// Changes from elsewhere will be overwritten.
    pub target_position: f64,
    pub duration: f64,
    pub issues: Vec<Issue>,
}

#[derive(PartialEq, Debug, Clone, Default, Serialize, Deserialize)]
pub struct Model {
    pub search_query: String,
    pub items: Vec<Item>,
    pub playlists: Vec<Playlist>,
    pub playlist_creation_state: Option<Playlist>,
    pub selected_playlist: Option<u64>,
    pub playing_playlist: Option<u64>,
    pub shuffle: bool,
    pub id_counter: u64,
}

impl Model {
    pub fn fresh_id(&mut self) -> u64 {
        self.id_counter += 1;
        self.id_counter
    }
}

#[derive(PartialEq, Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub items: Vec<u64>,
}

pub struct ImportState {
    pub items_in_progress: Vec<(u64, String, ItemImportStatus)>,
    pub finished: Vec<Item>,
}

pub type SharedImportState = Arc<RwLock<ImportState>>;

pub struct SharedModel {
    pub import_state: Option<(Receiver<ImportMessage>, SharedImportState)>,
    pub play_channel: Sender<ControlMessage>,
    pub model: Arc<RwLock<Model>>,
}
