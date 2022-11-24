mod colour_proxy;

use anyhow::Result;
use colour_proxy::ExtendedColourOps;
use eframe::egui::plot::{Bar, BarChart, Plot};
use eframe::epaint::{Color32, Stroke};
use eframe::{egui, egui::Frame};
use kira::manager::backend::cpal::CpalBackend;
use kira::manager::{AudioManager, AudioManagerSettings};
use kira::sound::FromFileError;
use kira::sound::static_sound::{StaticSoundData, StaticSoundSettings, PlaybackState};
use kira::sound::streaming::{StreamingSoundData, StreamingSoundSettings, StreamingSoundHandle};
use kira::tween::Tween;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use tracing::{info, warn, Level, debug};
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
const BAR_PLOT_WIDTH: f32 = 240.0;

fn main() {
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
        std::thread::spawn(move || process_control_messages(rx, model));
        // sync playback status every 100 ms
        let tx = tx.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(100));
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
            tx.send(ControlMessage::Seek(item.id, item.position)).unwrap();
        } else if item.status == ItemStatus::Loading {
            item.status = ItemStatus::Stopped;
        }
    }

    *model = loaded;
    Some(())
}

fn process_control_messages(rx: Receiver<ControlMessage>, model: Arc<RwLock<Model>>) {
    let manager = AudioManager::<CpalBackend>::new(AudioManagerSettings::default());
    if let Err(err) = manager {
        warn!("Failed to create audio manager: {}", err);
        return;
    }

    let mut manager = manager.unwrap();
    let mut handles = HashMap::<u64, StreamingSoundHandle<FromFileError>>::new();

    while let Ok(msg) = rx.recv() {
        let res = process_message(msg, &mut manager, &mut handles, &model);
        if let Err(err) = res {
            warn!("Failed to process control message: {}", err);
        }
    }
}

fn process_message(
    msg: ControlMessage,
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
            let res = if let Some(handle) = handles.get_mut(&id) {
                handle.resume(Tween::default())?;
                None
            } else {
                let file = edit_item(id, &mut |item| {
                    item.stems[item.current_stem].path.clone()
                }).unwrap();
                info!("loading {}", file);
                let sound = match StreamingSoundData::from_file(&file, StreamingSoundSettings::new()) {
                    Ok(sound) => sound,
                    Err(err) => {
                        edit_item(id, &mut |item| {
                            item.status = ItemStatus::Stopped;
                            String::new()
                        });
                        return Err(err.into());
                    }
                };
                let static_sound = StaticSoundData::from_file(&file, StaticSoundSettings::new()).unwrap();
                info!("passing {} to manager", file);
                let duration = static_sound.frames.len() as f64 / static_sound.sample_rate as f64;
                let frames = static_sound.frames;
                let handle = manager.play(sound)?;
                handles.insert(id, handle);
                Some((duration, frames))
            };
            // we ignore the option here - the edit may not go through
            // if the item was deleted in the meantime
            edit_item(id, &mut |item| {
                item.status = ItemStatus::Playing;
                if let Some((duration, frames)) = &res {
                    visualise_samples(item, frames);
                    item.duration = *duration;
                }
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
            let mut to_remove = vec![];
            for (&id, handle) in handles.iter_mut() {
                edit_item(id, &mut |item| {
                    item.position = handle.position();
                    if item.position >= item.duration || handle.state() == PlaybackState::Stopped {
                        item.status = ItemStatus::Stopped;
                        item.position = 0.0;
                        handle.stop(Tween::default()).unwrap();
                        to_remove.push(id);
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
            Ok(())
        }
    }
}

fn visualise_samples(item: &mut Item, frames: &[kira::dsp::Frame]) {
    // collect samples into bins
    let mut bins = vec![0.0; BARS];
    let mut max = 0.0f32;
    let bin_size = frames.len() / bins.len();
    debug!("processing {:#?} frames with bin size {}", frames.len(), bin_size);

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
    looped: bool,
    status: ItemStatus,
    colour: Color32,
    bars: Vec<f32>,
    position: f64,
    duration: f64,
}

#[derive(PartialEq, Debug, Clone, Default, Serialize, Deserialize)]
struct Model {
    items: Vec<Item>,
    id_counter: u64,
}

struct SharedModel {
    play_channel: Sender<ControlMessage>,
    model: Arc<RwLock<Model>>,
}

impl eframe::App for SharedModel {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut model = self.model.write();
        ctx.request_repaint_after(std::time::Duration::from_millis(100));

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label("Drag-and-drop files onto the window!");

            ui.vertical(|ui| {
                if ui.button("Import").clicked() {
                    if let Some(paths) = rfd::FileDialog::new().pick_files() {
                        model.import_paths(paths);
                    }
                }

                ui.with_layout(
                    egui::Layout::left_to_right(egui::Align::LEFT).with_main_wrap(true),
                    |ui| {
                        let channel = &self.play_channel;
                        for item in model.items.iter_mut() {
                            ui.scope(|ui| item_widget(channel, ui, item));
                        }
                    },
                );
            });
        });

        preview_files_being_dropped(ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let model = self.model.read();
        storage.set_string("model", serde_json::to_string(&*model).unwrap());
    }
}

impl Model {
    fn import_paths(&mut self, paths: Vec<PathBuf>) {
        self.items.extend(paths.into_iter().map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let path = path.display().to_string();
            let i = Item {
                id: self.id_counter,
                name,
                stems: vec![Stem {
                    tag: "default".to_string(),
                    path,
                }],
                current_stem: 0,
                volume: 1.0,
                looped: false,
                status: ItemStatus::Stopped,
                colour: PALETTE[self.id_counter as usize % PALETTE.len()],
                bars: vec![0.0; BARS],
                position: 0.0,
                duration: 0.0,
            };
            self.id_counter += 1;
            i
        }))
    }
}

fn item_widget(channel: &Sender<ControlMessage>, ui: &mut egui::Ui, item: &mut Item) {
    use rgb::*;
    let text_colour = item.colour.via_rgb(|c| c.map(|x| 255 - x));

    let style = ui.style_mut();
    let widget_style = &mut style.visuals.widgets;

    widget_style.inactive.bg_fill = item.colour;
    widget_style.inactive.fg_stroke.color = text_colour;
    widget_style.noninteractive.bg_fill = item.colour;

    Frame::group(ui.style()).show(ui, |ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(&item.name);
                match item.status {
                    ItemStatus::Stopped | ItemStatus::Paused => if ui.button("play").clicked() {
                        item.status = ItemStatus::Loading;
                        channel.send(ControlMessage::Play(item.id)).unwrap();
                    },
                    ItemStatus::Loading => {
                        ui.spinner();
                    },
                    ItemStatus::Playing => if ui.button("pause").clicked() {
                        item.status = ItemStatus::Paused;
                        channel.send(ControlMessage::Pause(item.id)).unwrap();
                    },
                };
            });
            render_bar_chart(channel, ui, item);
        });
    });
}

fn render_bar_chart(channel: &Sender<ControlMessage>, ui: &mut egui::Ui, item: &mut Item) {
    let id = format!("frequency graph for {}", item.id);
    let dimmed = item.colour.linear_multiply(0.1);

    let plot_x = ui.cursor().left();
    let resp = Plot::new(id)
        .height(30.0)
        .width(BAR_PLOT_WIDTH)
        .show_axes([false, false])
        .show_background(false)
        .show_x(false)
        .show_y(false)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_boxed_zoom(false)
        .allow_scroll(false)
        .show(ui, |plot| {
            let mut data = vec![];
            for (i, height) in item.bars.iter().copied().enumerate() {
                let height = height as f64;
                for direction in [-1.0, 1.0] {
                    let mut bar = Bar::new(i as f64, direction * height);
                    bar.bar_width = 0.15;
                    bar.stroke = Stroke::none();
                    let progress = i as f64 / item.bars.len() as f64;
                    bar.fill = if progress < item.position / item.duration { item.colour } else { dimmed };
                    data.push(bar);
                }
            }
            let chart = BarChart::new(data);
            plot.bar_chart(chart);
        });

    process_plot_events(channel, resp.response, plot_x, item);
}

fn process_plot_events(channel: &Sender<ControlMessage>, response: egui::Response, plot_x: f32, item: &mut Item) {
    let drag_distance = response.drag_delta().x;
    if drag_distance != 0.0 {
        let duration = item.duration as f32;
        let new_position = item.position as f32 + drag_distance * duration / BAR_PLOT_WIDTH;
        let new_position = new_position.clamp(0.0, duration) as f64;
        channel.send(ControlMessage::Seek(item.id, new_position)).unwrap();
        return;
    }
    if let Some(pos) = response.interact_pointer_pos().filter(|_| response.clicked()) {
        let duration = item.duration as f32;
        let new_position = (pos.x - plot_x) * duration / BAR_PLOT_WIDTH;
        let new_position = new_position.clamp(0.0, duration) as f64;
        channel.send(ControlMessage::Seek(item.id, new_position)).unwrap();
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
