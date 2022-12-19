mod app;
mod colour_proxy;
mod import;
mod model;
mod ui;

use model::*;
use ui::*;

use anyhow::Result;
use kira::manager::backend::cpal::CpalBackend;
use kira::manager::{AudioManager, AudioManagerSettings};
use kira::sound::static_sound::PlaybackState;
use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle, StreamingSoundSettings};
use kira::sound::FromFileError;
use kira::tween::Tween;
use kira::LoopBehavior;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use tracing::{info, warn, Level};
use tracing_subscriber::FmtSubscriber;

use crate::import::classify_from_file_err;

fn main() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::TRACE)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let options = eframe::NativeOptions {
        drag_and_drop_support: true,
        ..Default::default()
    };

    rayon::ThreadPoolBuilder::new()
        .start_handler(|_| {
            use thread_priority::*;
            set_current_thread_priority(ThreadPriority::Min).unwrap();
        })
        .build_global()
        .unwrap();

    let (tx, rx) = channel();
    let model = Arc::new(RwLock::new(Model::default()));

    {
        let model = model.clone();
        // start a background thread for audio playback
        {
            let tx = tx.clone();
            std::thread::spawn(move || process_control_messages(tx, rx, model));
        }
        // sync playback status every PLAYBACK_SYNC_INTERVAL ms
        let tx = tx.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(PLAYBACK_SYNC_INTERVAL));
            tx.send(ControlMessage::SyncPlaybackStatus).unwrap();
        });
    }

    eframe::run_native(
        "afx",
        options,
        Box::new(|cc| {
            app::recover(cc, tx.clone(), model.clone());

            Box::new(SharedModel {
                import_state: None,
                play_channel: tx,
                model,
            })
        }),
    );
}

fn process_control_messages(
    tx: Sender<ControlMessage>,
    rx: Receiver<ControlMessage>,
    model: Arc<RwLock<Model>>,
) {
    let manager = AudioManager::<CpalBackend>::new(AudioManagerSettings::default());
    if let Err(err) = manager {
        warn!("Failed to create audio manager: {}", err);
        return;
    }

    let mut manager = manager.unwrap();
    let mut handles = HashMap::<u64, StreamingSoundHandle<FromFileError>>::new();

    while let Ok(msg) = rx.recv() {
        let res = process_message(msg, &tx, &mut manager, &mut handles, &model);
        if let Err(err) = res {
            warn!("Failed to process control message: {}", err);
        }
    }
}

fn process_message(
    msg: ControlMessage,
    tx: &Sender<ControlMessage>,
    manager: &mut AudioManager,
    handles: &mut HashMap<u64, StreamingSoundHandle<FromFileError>>,
    model: &Arc<RwLock<Model>>,
) -> Result<()> {
    // string return value because lol no lambda generics :(
    let edit_item = |id: u64, f: &mut dyn FnMut(&mut Item) -> String| {
        let mut model = model.write();
        model.items.iter_mut().find(|item| item.id == id).map(f)
    };

    match msg {
        ControlMessage::Play(id) => {
            if let Some(handle) = handles.get_mut(&id) {
                handle.resume(Tween::default())?;
            } else {
                let handle = begin_playback(model, id, edit_item, manager)?;
                handles.insert(id, handle);
            }
            // we ignore the option here - the edit may not go through
            // if the item was deleted in the meantime
            edit_item(id, &mut |item| {
                item.status = ItemStatus::Playing;
                String::new()
            });
            Ok(())
        }
        ControlMessage::Pause(id) => {
            if let Some(handle) = handles.get_mut(&id) {
                handle.pause(Tween::default())?;
                edit_item(id, &mut |item| {
                    item.status = ItemStatus::Paused;
                    String::new()
                });
            }
            Ok(())
        }
        ControlMessage::ChangeStem(_, _) => todo!(),
        ControlMessage::SyncPlaybackStatus => {
            let mut to_remove = vec![];
            for (&id, handle) in handles
                .iter_mut()
                .filter(|(_, h)| h.state() != PlaybackState::Paused)
            {
                edit_item(id, &mut |item| {
                    item.target_position = handle.position();

                    if item.position >= item.duration || handle.state() == PlaybackState::Stopped {
                        item.target_position = 0.0;

                        to_remove.push(id);
                        if item.looped {
                            // FIXME this is a hack, since looping behaviour
                            // can't be changed via a handle
                            item.status = ItemStatus::Loading;
                            tx.send(ControlMessage::Play(id)).unwrap();
                        } else {
                            item.status = ItemStatus::Stopped;
                            handle.stop(Tween::default()).unwrap();
                        }
                    }
                    String::new()
                });
            }
            for id in to_remove {
                handles.remove(&id);
            }
            Ok(())
        }
        ControlMessage::Seek(id, target) => {
            let mut defer_to_sync = false;
            if let Some(handle) = handles.get_mut(&id) {
                handle.seek_to(target)?;
                if handle.state() == PlaybackState::Playing {
                    defer_to_sync = true;
                }
            }

            // FIXME there's still the issue of seeking a paused handle and then
            // letting it play. Leads to glitchy behaviour.
            if !defer_to_sync {
                edit_item(id, &mut |item| {
                    item.target_position = target;
                    String::new()
                });
            }
            Ok(())
        }
        ControlMessage::Loop(id, _do_loop) => {
            if let Some(_handle) = handles.get_mut(&id) {
                // TODO: implement looping via handles once it's supported
            }
            Ok(())
        }
        ControlMessage::Mute(id, mute) => {
            if let Some(handle) = handles.get_mut(&id) {
                let model = model.read();
                let item = model.items.iter().find(|item| item.id == id).unwrap();
                handle.set_volume(if mute { 0.0 } else { item.volume }, Tween::default())?;
            }
            Ok(())
        }
        ControlMessage::SetVolume(id, volume) => {
            if let Some(handle) = handles.get_mut(&id) {
                handle.set_volume(volume, Tween::default())?;
            }
            Ok(())
        }
        ControlMessage::Delete(id) => {
            if let Some(mut handle) = handles.remove(&id) {
                handle.stop(Tween::default())?;
            }
            let mut model = model.write();
            model.items.retain(|item| item.id != id);
            model.playlists.iter_mut().for_each(|playlist| {
                playlist.items.retain(|item| *item != id);
            });
            Ok(())
        }
        ControlMessage::AddToPlaylist {
            item_id,
            playlist_id,
        } => {
            let mut model = model.write();
            let playlist = model
                .playlists
                .iter_mut()
                .find(|playlist| playlist.id == playlist_id)
                .unwrap();
            playlist.items.push(item_id);
            Ok(())
        }
        ControlMessage::RemoveFromPlaylist {
            pos_within_playlist,
            playlist_id,
        } => {
            let mut model = model.write();
            let playlist = model
                .playlists
                .iter_mut()
                .find(|playlist| playlist.id == playlist_id)
                .unwrap();
            playlist.items.remove(pos_within_playlist);
            Ok(())
        }
        ControlMessage::PlayFromPlaylist(id) => {
            let mut model = model.write();
            // TODO if another playlist is playing, stop it
            // begin playback of the first item in the playlist
            // TODO if the playlist is empty, do nothing

            model.playing_playlist = Some(id);

            Ok(())
        }
        ControlMessage::GlobalPause => {
            let mut model = model.write();
            for (id, handle) in handles.iter_mut() {
                handle.pause(Tween::default())?;
                model
                    .items
                    .iter_mut()
                    .find(|item| item.id == *id)
                    .unwrap()
                    .status = ItemStatus::Paused;
            }
            Ok(())
        }
        ControlMessage::GlobalStop => {
            let mut model = model.write();
            for (id, handle) in handles.iter_mut() {
                handle.stop(Tween::default())?;
                let item = model.items.iter_mut().find(|item| item.id == *id).unwrap();
                item.status = ItemStatus::Stopped;
                item.target_position = 0.0;
            }
            handles.clear();
            Ok(())
        }
    }
}

fn begin_playback(
    model: &Arc<RwLock<Model>>,
    id: u64,
    mut edit_item: impl FnMut(u64, &mut dyn FnMut(&mut Item) -> String) -> Option<String>,
    manager: &mut AudioManager,
) -> Result<StreamingSoundHandle<FromFileError>> {
    let (file, position, looped, muted, volume) = {
        let model = model.read();
        let item = model.items.iter().find(|item| item.id == id).unwrap();
        let path = item.stems[item.current_stem].path.clone();
        (path, item.position, item.looped, item.muted, item.volume)
    };
    info!("loading {}", file);
    let settings = StreamingSoundSettings::new()
        .start_position(position)
        .volume(if muted { 0.0 } else { volume })
        .loop_behavior(if looped {
            Some(LoopBehavior {
                start_position: 0.0,
            })
        } else {
            None
        });
    let sound = match StreamingSoundData::from_file(&file, settings) {
        Ok(sound) => sound,
        Err(err) => {
            edit_item(id, &mut |item| {
                item.status = ItemStatus::Stopped;
                let (msg, typ) = classify_from_file_err(&err);
                item.issues.push((typ, msg));
                String::new()
            });
            return Err(err.into());
        }
    };
    info!("passing {} to manager", file);
    Ok(manager.play(sound)?)
}

#[cfg(test)]
mod test {
    use super::*;
    use eframe::epaint::Color32;

    fn build_test_model() -> Model {
        let path = "samples/416529__inspectorj__bird-whistling-single-robin-a.wav".to_string();
        Model {
            items: vec![
                Item::with_default_stem(
                    0,
                    "test 0".to_string(),
                    path.clone(),
                    Color32::BLACK,
                    1.0,
                ),
                Item::with_default_stem(
                    1,
                    "test 1".to_string(),
                    path.clone(),
                    Color32::BLACK,
                    1.0,
                ),
                Item::with_default_stem(
                    2,
                    "test 2".to_string(),
                    path,
                    Color32::BLACK,
                    1.0,
                ),
            ],
            ..Model::default()
        }
    }

    #[test]
    fn file_not_found() -> Result<()> {
        // create a temporary directory and try to play a nonexistent file from it
        let path = {
            let tempdir = tempfile::tempdir()?;
            let path = tempdir
                .path()
                .join("nonexistent.wav")
                .to_str()
                .unwrap()
                .to_string();
            tempdir.close()?;
            path
        };
        let model = {
            let mut m = build_test_model();
            m.items[0].stems[0].path = path;
            m
        };
        let mut manager = AudioManager::<CpalBackend>::new(AudioManagerSettings::default())?;
        let mut handles = HashMap::new();

        let msg = ControlMessage::Play(0);

        let model = Arc::new(RwLock::new(model));
        let (rx, _tx) = channel();
        #[allow(unused_must_use)]
        {
            process_message(msg, &rx, &mut manager, &mut handles, &model);
        }

        let model = &*model.read();

        assert_eq!(model.items[0].status, ItemStatus::Stopped);
        assert_eq!(model.items[0].issues.len(), 1);
        assert_eq!(model.items[0].issues[0].0, IssueType::MissingFile);
        Ok(())
    }

    #[test]
    fn play_and_pause() -> Result<()> {
        let model = build_test_model();
        let mut manager = AudioManager::<CpalBackend>::new(AudioManagerSettings::default())?;
        let mut handles = HashMap::new();

        let model = Arc::new(RwLock::new(model));
        let (rx, _tx) = channel();

        process_message(ControlMessage::Play(0), &rx, &mut manager, &mut handles, &model)?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(model.read().items[0].status, ItemStatus::Playing);

        process_message(ControlMessage::Pause(0), &rx, &mut manager, &mut handles, &model)?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(model.read().items[0].status, ItemStatus::Paused);

        Ok(())
    }

    #[test]
    fn play_many() -> Result<()> {
        let model = build_test_model();
        let mut manager = AudioManager::<CpalBackend>::new(AudioManagerSettings::default())?;
        let mut handles = HashMap::new();

        let model = Arc::new(RwLock::new(model));
        let (rx, _tx) = channel();

        process_message(ControlMessage::Play(0), &rx, &mut manager, &mut handles, &model)?;
        process_message(ControlMessage::Play(1), &rx, &mut manager, &mut handles, &model)?;
        process_message(ControlMessage::Play(2), &rx, &mut manager, &mut handles, &model)?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(model.read().items[0].status, ItemStatus::Playing);
        assert_eq!(model.read().items[1].status, ItemStatus::Playing);
        assert_eq!(model.read().items[2].status, ItemStatus::Playing);

        process_message(ControlMessage::GlobalPause, &rx, &mut manager, &mut handles, &model)?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(model.read().items[0].status, ItemStatus::Paused);
        assert_eq!(model.read().items[1].status, ItemStatus::Paused);
        assert_eq!(model.read().items[2].status, ItemStatus::Paused);

        process_message(ControlMessage::GlobalStop, &rx, &mut manager, &mut handles, &model)?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(model.read().items[0].status, ItemStatus::Stopped);
        assert_eq!(model.read().items[1].status, ItemStatus::Stopped);
        assert_eq!(model.read().items[2].status, ItemStatus::Stopped);

        Ok(())
    }

    #[test]
    fn seek() -> Result<()> {
        let model = build_test_model();
        let mut manager = AudioManager::<CpalBackend>::new(AudioManagerSettings::default())?;
        let mut handles = HashMap::new();

        let model = Arc::new(RwLock::new(model));
        let (rx, _tx) = channel();

        process_message(ControlMessage::Play(0), &rx, &mut manager, &mut handles, &model)?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(model.read().items[0].status, ItemStatus::Playing);

        process_message(ControlMessage::Seek(0, 0.5), &rx, &mut manager, &mut handles, &model)?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(model.read().items[0].status, ItemStatus::Playing);
        assert_eq!(model.read().items[0].target_position, 0.5);
        // TODO requires syncs

        Ok(())
    }
}
