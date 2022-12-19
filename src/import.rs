use crate::model::*;
use crate::ui::*;
use eframe::egui;
use kira::sound::static_sound::{StaticSoundData, StaticSoundSettings};
use kira::sound::FromFileError;
use parking_lot::{RwLock, RwLockWriteGuard};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use tracing::{debug, warn};

impl SharedModel {
    pub fn begin_import(&mut self) {
        let model = self.model.clone();
        let (sender, receiver) = channel();
        self.import_state = Some((
            receiver,
            Arc::new(RwLock::new(ImportState {
                items_in_progress: vec![],
                finished: vec![],
            })),
        ));

        std::thread::spawn(move || {
            if let Some(paths) = rfd::FileDialog::new()
                .set_title("Choose files to import")
                .pick_files()
            {
                let new_items = import_paths(
                    sender.clone(),
                    || {
                        let mut model = model.write();
                        model.fresh_id()
                    },
                    paths,
                );
                sender.send(ImportMessage::Finished(new_items)).unwrap();
            } else {
                sender.send(ImportMessage::Cancelled).unwrap();
            }
        });
    }
}

fn import_paths(
    tx: Sender<ImportMessage>,
    mut fresh_id: impl FnMut() -> u64,
    paths: Vec<PathBuf>,
) -> Vec<Item> {
    use rayon::prelude::*;

    paths
        .into_iter()
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let path = path.display().to_string();
            let id = fresh_id();
            tx.send(ImportMessage::Update(
                id,
                ItemImportStatus::Queued(name.clone()),
            ))
            .unwrap();

            (name, path, id, tx.clone())
        })
        .collect::<Vec<_>>()
        .into_par_iter()
        .flat_map(|(name, path, id, tx)| create_item(tx, id, path, name))
        .collect()
}

fn create_item(tx: Sender<ImportMessage>, id: u64, path: String, name: String) -> Option<Item> {
    tx.send(ImportMessage::Update(id, ItemImportStatus::InProgress))
        .unwrap();
    let static_sound = match StaticSoundData::from_file(&path, StaticSoundSettings::new()) {
        Ok(sound) => sound,
        Err(e) => {
            let (msg, _) = classify_from_file_err(&e);
            warn!("failed to load {}: {}", path, msg);
            tx.send(ImportMessage::Update(id, ItemImportStatus::Failed(msg)))
                .unwrap();
            return None;
        }
    };
    let duration = static_sound.frames.len() as f64 / static_sound.sample_rate as f64;
    let mut i = Item::with_default_stem(
        id,
        name,
        path,
        PALETTE[id as usize % PALETTE.len()],
        duration,
    );
    i.bars = visualise_samples(&static_sound.frames);
    tx.send(ImportMessage::Update(id, ItemImportStatus::Finished))
        .unwrap();
    Some(i)
}

pub fn process_import_message(
    msg: ImportMessage,
    ui: &mut egui::Ui,
    keep_window_open: &mut bool,
    state: &mut RwLockWriteGuard<ImportState>,
) {
    match msg {
        ImportMessage::Cancelled => {
            ui.label("Cancelled");
            *keep_window_open = false;
        }
        ImportMessage::Update(id, status) => match status {
            ItemImportStatus::Queued(name) => {
                state
                    .items_in_progress
                    .push((id, name, ItemImportStatus::Waiting));
            }
            s => {
                if let Some((_, _, status)) = state
                    .items_in_progress
                    .iter_mut()
                    .find(|(i, _, _)| *i == id)
                {
                    *status = s;
                }
            }
        },
        ImportMessage::Finished(v) => {
            debug!("process_import_message received {} items", v.len());
            state.finished = v;
        }
    }
}

fn visualise_samples(frames: &[kira::dsp::Frame]) -> Vec<u8> {
    // collect samples into bins
    let mut bins = vec![0.0; BARS];
    let mut max = 0.0f32;
    let bin_size = frames.len() / bins.len();
    debug!(
        "processing {:#?} frames with bin size {}",
        frames.len(),
        bin_size
    );

    for (i, bin) in bins.iter_mut().enumerate() {
        let start = i * bin_size;
        let end = start + bin_size;
        let mut sum = 0.0;
        for sample in frames[start..end].iter() {
            sum += sample.left.abs() * 0.5 + sample.right.abs() * 0.5;
        }
        *bin = sum / bin_size as f32;
        max = max.max(*bin);
    }

    bins.into_iter()
        .map(|bin| (255.0 * (bin / max)).round() as u8)
        .collect()
}

pub fn classify_from_file_err(e: &FromFileError) -> (String, IssueType) {
    use std::io::ErrorKind;
    use symphonia::core::errors;
    use IssueType::*;

    fn describe_io_error(kind: ErrorKind) -> (String, IssueType) {
        match kind {
            ErrorKind::NotFound => ("the file could not be found".to_string(), MissingFile),
            ErrorKind::PermissionDenied => (
                "permission to read the file was denied".to_string(),
                InaccessibleFile,
            ),
            kind => (format!("an IO error occurred: {}", kind), OtherError),
        }
    }

    match e {
        FromFileError::NoDefaultTrack => (
            "the file doesn't have a default track".to_string(),
            PlaybackProblem,
        ),
        FromFileError::UnknownSampleRate => (
            "the sample rate could not be determined".to_string(),
            PlaybackProblem,
        ),
        FromFileError::UnsupportedChannelConfiguration => (
            "the channel configuration of the file is not supported".to_string(),
            PlaybackProblem,
        ),
        FromFileError::IoError(io_err) => describe_io_error(io_err.kind()),
        FromFileError::SymphoniaError(symphonia_err) => match symphonia_err {
            errors::Error::IoError(e) => describe_io_error(e.kind()),
            errors::Error::DecodeError(e) => (
                format!("symphonia could not decode the file: {}", e),
                PlaybackProblem,
            ),
            errors::Error::SeekError(e) => match e {
                errors::SeekErrorKind::Unseekable => {
                    ("this file is not seekable".to_string(), PlaybackProblem)
                }
                errors::SeekErrorKind::ForwardOnly => (
                    "this file can only be seeked forward".to_string(),
                    PlaybackProblem,
                ),
                errors::SeekErrorKind::OutOfRange => (
                    "the seek timestamp is out of range".to_string(),
                    PlaybackProblem,
                ),
                errors::SeekErrorKind::InvalidTrack => {
                    ("the track ID is invalid".to_string(), PlaybackProblem)
                }
            },
            errors::Error::Unsupported(e) => (
                format!("symphonia does not support this format: {}", e),
                PlaybackProblem,
            ),
            errors::Error::LimitError(e) => {
                (format!("a limit error occurred: {}", e), PlaybackProblem)
            }
            errors::Error::ResetRequired => {
                ("symphonia requires a reset".to_string(), PlaybackProblem)
            }
        },
        _ => ("an unknown error occurred".to_string(), OtherError),
    }
}
