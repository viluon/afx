mod colour_proxy;

use anyhow::Result;
use colour_proxy::ExtendedColourOps;
use eframe::egui::plot::{Bar, BarChart, Plot};
use eframe::egui::{Button, RichText, Slider};
use eframe::epaint::{vec2, Color32, Stroke};
use eframe::{egui, egui::Frame};
use kira::manager::backend::cpal::CpalBackend;
use kira::manager::{AudioManager, AudioManagerSettings};
use kira::sound::static_sound::{PlaybackState, StaticSoundData, StaticSoundSettings};
use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle, StreamingSoundSettings};
use kira::sound::FromFileError;
use kira::tween::Tween;
use kira::LoopBehavior;
use parking_lot::{RwLock, RwLockWriteGuard};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use tracing::{debug, info, warn, Level};
use tracing_subscriber::FmtSubscriber;

#[rustfmt::skip]
mod colours {
    use super::*;
    pub const ORANGE   : Color32 = Color32::from_rgb(240, 135, 35);
    pub const YELLOW   : Color32 = Color32::from_rgb(230, 200, 50);
    pub const PURPLE   : Color32 = Color32::from_rgb(110, 60,  200);
    pub const PINK     : Color32 = Color32::from_rgb(240, 140, 170);
    pub const BURGUNDY : Color32 = Color32::from_rgb(119, 51,  85);
    pub const SALMON   : Color32 = Color32::from_rgb(220, 130, 140);
    pub const TEAL     : Color32 = Color32::from_rgb(40,  150, 190);
    pub const BROWN    : Color32 = Color32::from_rgb(102, 51,  46);
    pub const CREAM    : Color32 = Color32::from_rgb(238, 221, 170);
    pub const RED      : Color32 = Color32::from_rgb(230, 70,  70);
    pub const GREEN    : Color32 = Color32::from_rgb(70,  175, 70);
    pub const BLUE     : Color32 = Color32::from_rgb(40,  120, 220);
}

use colours::*;
const PALETTE: [Color32; 12] = [
    ORANGE, YELLOW, PURPLE, PINK, BURGUNDY, SALMON, TEAL, BROWN, CREAM, RED, GREEN, BLUE,
];

const BARS: usize = 128;
const BAR_PLOT_WIDTH: f32 = 360.0;
const PLAYBACK_SYNC_INTERVAL: u64 = 50;

fn main() {
    coz::thread_init();

    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::TRACE)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let options = eframe::NativeOptions {
        drag_and_drop_support: true,
        ..Default::default()
    };

    let (tx, rx) = channel();
    let model = Arc::new(RwLock::new(Model::default()));

    {
        let model = model.clone();
        // start a background thread for audio playback
        {
            let tx = tx.clone();
            std::thread::spawn(move || {
                coz::thread_init();
                process_control_messages(tx, rx, model)
            });
        }
        // sync playback status every PLAYBACK_SYNC_INTERVAL ms
        let tx = tx.clone();
        std::thread::spawn(move || {
            coz::thread_init();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(PLAYBACK_SYNC_INTERVAL));
                tx.send(ControlMessage::SyncPlaybackStatus).unwrap();
            }
        });
    }

    eframe::run_native(
        "afx",
        options,
        Box::new(|cc| {
            recover(cc, tx.clone(), model.clone());

            Box::new(SharedModel {
                play_channel: tx,
                model,
            })
        }),
    );
}

/// Recover saved state of the application.
fn recover(
    cc: &eframe::CreationContext,
    tx: Sender<ControlMessage>,
    model: Arc<RwLock<Model>>,
) -> Option<()> {
    let saved = cc.storage?.get_string("model")?;
    let mut loaded: Model = match serde_json::from_str(&saved) {
        Ok(loaded) => Some(loaded),
        Err(err) => {
            eprintln!("Failed to load saved model: {}", err);
            None
        }
    }?;

    // taking the lock before any messages are sent so that the background
    // thread can't accidentally query the model before it's been loaded
    let mut model = model.write();
    for item in loaded.items.iter_mut() {
        if item.status == ItemStatus::Playing {
            item.status = ItemStatus::Loading;
            tx.send(ControlMessage::Play(item.id)).unwrap();
        } else if item.status == ItemStatus::Loading {
            item.status = ItemStatus::Stopped;
        }
    }

    *model = loaded;
    Some(())
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
            }
            Ok(())
        }
        ControlMessage::ChangeStem(_, _) => todo!(),
        ControlMessage::SyncPlaybackStatus => {
            coz::scope!("playback sync");

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
            if let Some(handle) = handles.get_mut(&id) {
                handle.seek_to(target)?;
            }
            edit_item(id, &mut |item| {
                item.target_position = target;

                String::new()
            });
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
                String::new()
            });
            return Err(err.into());
        }
    };
    info!("passing {} to manager", file);
    Ok(manager.play(sound)?)
}

fn visualise_samples(item: &mut Item, frames: &[kira::dsp::Frame]) {
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

    item.bars = bins.into_iter().map(|bin| bin / max).collect();
}

#[derive(PartialEq, PartialOrd, Debug, Clone)]
enum ControlMessage {
    Play(u64),
    Pause(u64),
    ChangeStem(u64, usize),
    SyncPlaybackStatus,
    Seek(u64, f64),
    Loop(u64, bool),
    Mute(u64, bool),
    SetVolume(u64, f64),
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Serialize, Deserialize)]
struct Stem {
    tag: String,
    path: String,
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Serialize, Deserialize)]
enum ItemStatus {
    Stopped,
    Loading,
    Playing,
    Paused,
}

#[derive(PartialEq, Debug, Clone, Serialize, Deserialize)]
struct Item {
    id: u64,
    name: String,
    stems: Vec<Stem>,
    current_stem: usize,
    volume: f64,
    muted: bool,
    looped: bool,
    status: ItemStatus,
    colour: Color32,
    bars: Vec<f32>,
    /// The position within the track, in seconds.
    ///
    /// This is effectively owned by the playback thread.
    /// Changes from elsewhere will be overwritten.
    position: f64,
    target_position: f64,
    duration: f64,
}

#[derive(PartialEq, Debug, Clone, Default, Serialize, Deserialize)]
struct Model {
    search_query: String,
    items: Vec<Item>,
    id_counter: u64,
}

struct SharedModel {
    play_channel: Sender<ControlMessage>,
    model: Arc<RwLock<Model>>,
}

impl eframe::App for SharedModel {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        coz::scope!("frame time");
        let mut model = self.model.write();
        ctx.request_repaint_after(std::time::Duration::from_millis(PLAYBACK_SYNC_INTERVAL));

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.allocate_ui_with_layout(
                vec2(ui.available_size_before_wrap().x, 0.0),
                egui::Layout::left_to_right(eframe::emath::Align::Center),
                |ui| {
                    let search_field = egui::TextEdit::singleline(&mut model.search_query)
                        .hint_text("type to search");
                    let resp = ui.add(search_field);
                    if !model.search_query.is_empty() {
                        let button = Button::new("❌").frame(false);
                        if ui.add(button).clicked()
                            || (resp.lost_focus()
                                && ui.ctx().input().key_pressed(egui::Key::Escape))
                        {
                            model.search_query.clear();
                            resp.request_focus();
                        }
                    }
                    if ui.ctx().input_mut().consume_key(egui::Modifiers::CTRL, egui::Key::F) {
                        resp.request_focus();
                    }
                },
            );

            let desired_size = ui.ctx().input().screen_rect.size();
            let desired_size = vec2(desired_size.x * 0.9, 95.0);
            ui.allocate_ui(desired_size, |ui| {
                ui.with_layout(
                    egui::Layout::left_to_right(egui::Align::LEFT).with_main_wrap(true),
                    |ui| {
                        ui.set_max_size(desired_size);
                        let channel = &self.play_channel;
                        render_items(model, channel, ui);
                    },
                )
            });
        });

        preview_files_being_dropped(ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let model = self.model.read();
        storage.set_string("model", serde_json::to_string(&*model).unwrap());
    }
}

fn render_items(
    mut model: RwLockWriteGuard<Model>,
    channel: &Sender<ControlMessage>,
    ui: &mut egui::Ui,
) {
    let lowercase_query = model.search_query.to_lowercase();
    let pat: Vec<_> = lowercase_query.split_ascii_whitespace().collect();
    for item in model
        .items
        .iter_mut()
        .filter(|item| pat.iter().all(|w| item.name.to_lowercase().contains(w)))
    {
        render_item_frame(channel, ui, item);
        if ui.available_size_before_wrap().x < BAR_PLOT_WIDTH {
            ui.end_row();
        }
    }
    let widget =
        Button::new(RichText::new("Import").heading().color(Color32::BLACK)).fill(Color32::GOLD);
    if ui.add(widget).clicked() {
        if let Some(paths) = rfd::FileDialog::new().pick_files() {
            model.import_paths(paths);
        }
    }
}

impl Model {
    fn import_paths(&mut self, paths: Vec<PathBuf>) {
        self.items.extend(paths.into_iter().flat_map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let path = path.display().to_string();
            let static_sound = match StaticSoundData::from_file(&path, StaticSoundSettings::new()) {
                Ok(sound) => sound,
                Err(e) => {
                    report_import_error(&path, e);
                    return None;
                }
            };

            let duration = static_sound.frames.len() as f64 / static_sound.sample_rate as f64;
            let id = self.id_counter;
            self.id_counter += 1;

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
                bars: vec![0.0; BARS],
                position: 0.0,
                target_position: 0.0,
                duration,
            };

            visualise_samples(&mut i, &static_sound.frames);
            Some(i)
        }))
    }
}

fn report_import_error(path: &String, e: FromFileError) {
    use std::io::ErrorKind;
    use symphonia::core::errors;

    warn!(
        "failed to load {}: {}",
        path,
        match e {
            FromFileError::NoDefaultTrack => "the file doesn't have a default track".to_string(),
            FromFileError::UnknownSampleRate =>
                "the sample rate could not be determined".to_string(),
            FromFileError::UnsupportedChannelConfiguration =>
                "the channel configuration of the file is not supported".to_string(),
            FromFileError::IoError(io_err) => match io_err.kind() {
                ErrorKind::NotFound => "the file could not be found".to_string(),
                ErrorKind::PermissionDenied => "permission to read the file was denied".to_string(),
                kind => format!("an IO error occurred: {}", kind),
            },
            FromFileError::SymphoniaError(symphonia_err) => match symphonia_err {
                errors::Error::IoError(e) => format!("symphonia encountered an I/O error: {}", e),
                errors::Error::DecodeError(e) =>
                    format!("symphonia could not decode the file: {}", e),
                errors::Error::SeekError(e) => match e {
                    errors::SeekErrorKind::Unseekable => "this file is not seekable".to_string(),
                    errors::SeekErrorKind::ForwardOnly =>
                        "this file can only be seeked forward".to_string(),
                    errors::SeekErrorKind::OutOfRange =>
                        "the seek timestamp is out of range".to_string(),
                    errors::SeekErrorKind::InvalidTrack => "the track ID is invalid".to_string(),
                },
                errors::Error::Unsupported(e) =>
                    format!("symphonia does not support this format: {}", e),
                errors::Error::LimitError(e) => format!("a limit error occurred: {}", e),
                errors::Error::ResetRequired => "symphonia requires a reset".to_string(),
            },
            _ => "an unknown error occurred".to_string(),
        }
    );
}

fn render_item_frame(channel: &Sender<ControlMessage>, ui: &mut egui::Ui, item: &mut Item) {
    Frame::group(ui.style())
        .stroke(if matches!(item.status, ItemStatus::Playing) {
            Stroke::new(1.0, Color32::WHITE)
        } else {
            ui.style().visuals.widgets.noninteractive.bg_stroke
        })
        .fill(item.colour.linear_multiply(0.03))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let text = RichText::new(&item.name[0..32.min(item.name.len())])
                        .color(Color32::WHITE)
                        .text_style(egui::TextStyle::Heading);
                    ui.label(text);
                });
                render_bar_chart(channel, ui, item);
                ui.allocate_ui_with_layout(
                    vec2(0.0, 0.0),
                    egui::Layout::left_to_right(egui::Align::Center).with_main_justify(true),
                    |ui| {
                        render_item_controls(channel, ui, item);
                    }
                );
            });
        })
        .response
        .context_menu(|ui| {
            if ui.button(RichText::new("Delete").color(RED)).clicked() {
                warn!("oops");
                ui.close_menu();
            }
        });
}

fn render_item_controls(channel: &Sender<ControlMessage>, ui: &mut egui::Ui, item: &mut Item) {
    match item.status {
        ItemStatus::Stopped | ItemStatus::Paused => {
            if ui.button(RichText::new("▶").heading()).clicked() {
                item.status = ItemStatus::Loading;
                channel.send(ControlMessage::Play(item.id)).unwrap();
            }
        }
        ItemStatus::Loading => {
            ui.spinner();
        }
        ItemStatus::Playing => {
            if ui.button(RichText::new("⏸").heading()).clicked() {
                item.status = ItemStatus::Paused;
                channel.send(ControlMessage::Pause(item.id)).unwrap();
            }
        }
    };

    let loop_button = Button::new(if item.looped { "🔁" } else { "🔂" }).frame(item.looped);
    if ui.add(loop_button).clicked() {
        item.looped = !item.looped;
        channel
            .send(ControlMessage::Loop(item.id, item.looped))
            .unwrap();
    }

    if ui.button(if item.muted { "🔇" } else { "🔈" }).clicked() {
        item.muted = !item.muted;
        channel
            .send(ControlMessage::Mute(item.id, item.muted))
            .unwrap();
    }

    let original_volume = item.volume;
    ui.add(Slider::new(&mut item.volume, 0.0001..=1.0).show_value(false));
    if original_volume != item.volume {
        channel
            .send(ControlMessage::SetVolume(item.id, item.volume))
            .unwrap();
    }

    let minutes = (item.position / 60.0).floor() as u32;
    let seconds = item.position % 60.0;
    ui.label(format!("{:01}:{:05.2}", minutes, seconds));
}

fn render_bar_chart(channel: &Sender<ControlMessage>, ui: &mut egui::Ui, item: &mut Item) {
    let id = format!("frequency graph for {}", item.id);
    let bg = ui.style().visuals.window_fill();
    let dimmed = bg.mix(0.4, &item.colour);

    item.position =
        ui.ctx()
            .animate_value_with_time(egui::Id::new(item.id), item.target_position as f32, 0.06)
            as f64;

    let plot_x = ui.cursor().left();
    let resp = Plot::new(id)
        .height(30.0)
        .width(BAR_PLOT_WIDTH)
        .include_y(1.0)
        .include_y(-1.0)
        .set_margin_fraction(vec2(0.0, 0.0))
        .allow_boxed_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .allow_zoom(false)
        .show_axes([false, false])
        .show_background(false)
        .show_x(false)
        .show_y(false)
        .show(ui, |plot| {
            let mut data = vec![];
            for (i, height) in item.bars.iter().copied().enumerate() {
                let height = height as f64;
                for direction in [-1.0, 1.0] {
                    let muted_modifier = if item.muted { 0.0001 } else { 1.0 };
                    let mut bar =
                        Bar::new(i as f64, muted_modifier * item.volume * direction * height);
                    bar.bar_width = 0.4;
                    bar.stroke = Stroke::none();
                    let fill_level =
                        ((item.position / item.duration) * item.bars.len() as f64 - i as f64)
                        .clamp(0.0, 1.0);
                    bar.fill = dimmed.mix(fill_level as f32, &item.colour);
                    data.push(bar);
                }

                coz::progress!("bar plot processing");
            }
            let chart = BarChart::new(data);
            plot.bar_chart(chart);
        });

    handle_bar_plot_interaction(channel, resp.response, plot_x, item);
}

fn handle_bar_plot_interaction(
    channel: &Sender<ControlMessage>,
    response: egui::Response,
    plot_x: f32,
    item: &mut Item,
) {
    let drag_distance = response.drag_delta().x;
    if drag_distance != 0.0 {
        let duration = item.duration as f32;
        let new_position = item.position as f32 + drag_distance * duration / BAR_PLOT_WIDTH;
        let new_position = new_position.clamp(0.0, duration) as f64;

        channel
            .send(ControlMessage::Seek(item.id, new_position))
            .unwrap();
        return;
    }
    if let Some(pos) = response
        .interact_pointer_pos()
        .filter(|_| response.clicked())
    {
        let duration = item.duration as f32;
        let new_position = (pos.x - plot_x) * duration / BAR_PLOT_WIDTH;
        let new_position = new_position.clamp(0.0, duration) as f64;
        channel
            .send(ControlMessage::Seek(item.id, new_position))
            .unwrap();
    }
}

/// Preview hovering files:
fn preview_files_being_dropped(ctx: &egui::Context) {
    use egui::*;
    use std::fmt::Write as _;

    if !ctx.input().raw.hovered_files.is_empty() {
        let mut text = "Dropping files:\n".to_owned();
        for file in &ctx.input().raw.hovered_files {
            if let Some(path) = &file.path {
                write!(text, "\n{}", path.display()).ok();
            } else if !file.mime.is_empty() {
                write!(text, "\n{}", file.mime).ok();
            } else {
                text += "\n???";
            }
        }

        let painter =
            ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("file_drop_target")));

        let screen_rect = ctx.input().screen_rect();
        painter.rect_filled(screen_rect, 0.0, Color32::from_black_alpha(192));
        painter.text(
            screen_rect.center(),
            Align2::CENTER_CENTER,
            text,
            TextStyle::Heading.resolve(&ctx.style()),
            Color32::WHITE,
        );
    }
}
