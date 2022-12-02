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
        self.import_state = Some((receiver, Arc::new(RwLock::new((vec![], vec![])))));

        std::thread::spawn(move || {
            if let Some(paths) = rfd::FileDialog::new().pick_files() {
                let new_items = import_paths(
                    sender.clone(),
                    || {
                        let mut model = model.write();
                        let id = model.id_counter;
                        model.id_counter += 1;
                        id
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
            let msg = report_import_error(e);
            warn!("failed to load {}: {}", path, msg);
            tx.send(ImportMessage::Update(id, ItemImportStatus::Failed(msg)))
                .unwrap();
            return None;
        }
    };
    let duration = static_sound.frames.len() as f64 / static_sound.sample_rate as f64;
    let mut i = Item {
        id,
        name,
        stems: vec![Stem {
            tag: "default".to_string(),
            path,
        }],
        current_stem: 0,
        volume: 1.0,
        muted: false,
        looped: false,
        status: ItemStatus::Stopped,
        colour: PALETTE[id as usize % PALETTE.len()],
        bars: vec![],
        position: 0.0,
        target_position: 0.0,
        duration,
    };
    i.bars = visualise_samples(&static_sound.frames);
    tx.send(ImportMessage::Update(id, ItemImportStatus::Finished))
        .unwrap();
    Some(i)
}

pub fn process_import_message(
    msg: ImportMessage,
    ui: &mut egui::Ui,
    keep_window_open: &mut bool,
    state: &mut RwLockWriteGuard<ImportStatus>,
) {
    match msg {
        ImportMessage::Cancelled => {
            ui.label("Cancelled");
            *keep_window_open = false;
        }
        ImportMessage::Update(id, status) => match status {
            ItemImportStatus::Queued(name) => {
                state.0.push((id, name, ItemImportStatus::Waiting));
            }
            s => {
                if let Some((_, _, status)) = state.0.iter_mut().find(|(i, _, _)| *i == id) {
                    *status = s;
                }
            }
        },
        ImportMessage::Finished(v) => {
            debug!("render_import_progress received {} items", v.len());
            state.1 = v;
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

fn report_import_error(e: FromFileError) -> String {
    use std::io::ErrorKind;
    use symphonia::core::errors;

    match e {
        FromFileError::NoDefaultTrack => "the file doesn't have a default track".to_string(),
        FromFileError::UnknownSampleRate => "the sample rate could not be determined".to_string(),
        FromFileError::UnsupportedChannelConfiguration => {
            "the channel configuration of the file is not supported".to_string()
        }
        FromFileError::IoError(io_err) => match io_err.kind() {
            ErrorKind::NotFound => "the file could not be found".to_string(),
            ErrorKind::PermissionDenied => "permission to read the file was denied".to_string(),
            kind => format!("an IO error occurred: {}", kind),
        },
        FromFileError::SymphoniaError(symphonia_err) => match symphonia_err {
            errors::Error::IoError(e) => format!("symphonia encountered an I/O error: {}", e),
            errors::Error::DecodeError(e) => format!("symphonia could not decode the file: {}", e),
            errors::Error::SeekError(e) => match e {
                errors::SeekErrorKind::Unseekable => "this file is not seekable".to_string(),
                errors::SeekErrorKind::ForwardOnly => {
                    "this file can only be seeked forward".to_string()
                }
                errors::SeekErrorKind::OutOfRange => {
                    "the seek timestamp is out of range".to_string()
                }
                errors::SeekErrorKind::InvalidTrack => "the track ID is invalid".to_string(),
            },
            errors::Error::Unsupported(e) => {
                format!("symphonia does not support this format: {}", e)
            }
            errors::Error::LimitError(e) => format!("a limit error occurred: {}", e),
            errors::Error::ResetRequired => "symphonia requires a reset".to_string(),
        },
        _ => "an unknown error occurred".to_string(),
    }
}
