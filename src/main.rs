mod colour_proxy;

use anyhow::Result;
use colour_proxy::ExtendedColourOps;
use eframe::egui::plot::{Bar, BarChart, Plot};
use eframe::epaint::{Color32, Stroke};
use eframe::{egui, egui::Frame};
use kira::manager::backend::cpal::CpalBackend;
use kira::manager::{AudioManager, AudioManagerSettings};
use kira::sound::static_sound::{StaticSoundHandle, StaticSoundData, StaticSoundSettings};
use kira::tween::Tween;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing_subscriber::FmtSubscriber;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use tracing::{info, warn, Level};

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

fn main() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::TRACE)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

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
    let loaded: Model = match serde_json::from_str(&saved) {
        Ok(loaded) => Some(loaded),
        Err(err) => {
            eprintln!("Failed to load saved model: {}", err);
            None
        }
    }?;

    let mut model = model.write();
    for item in loaded.items.iter() {
        if item.playing {
            tx.send(ControlMessage::Play(item.id)).unwrap();
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
    let mut handles = HashMap::<u64, StaticSoundHandle>::new();

    while let Ok(msg) = rx.recv() {
        let res = process_message(msg, &mut manager, &mut handles, &model);
        if let Err(err) = res {
            warn!("Failed to process control message: {}", err);
        }
    }
}

fn process_message(msg: ControlMessage, manager: &mut AudioManager, handles: &mut HashMap<u64, StaticSoundHandle>, model: &Arc<RwLock<Model>>) -> Result<()> {
    let edit_item = |id: u64, f: fn(&mut Item) -> i32| -> i32 {
        let mut model = model.write();
        let item = model.items.iter_mut().find(|item| item.id == id).unwrap();
        f(item)
    };
    match msg {
        ControlMessage::Play(id) => {
            if let Some(handle) = handles.get_mut(&id) {
                handle.resume(Tween::default())?;
            } else {
                let file = {
                    let model = model.read();
                    let item = model.items.iter().find(|item| item.id == id).unwrap();
                    item.stems[item.current_stem].path.clone()
                };
                info!("loading {}", file);
                let sound = StaticSoundData::from_file(&file, StaticSoundSettings::new())?;
                info!("passing {} to manager", file);
                let handle = manager.play(sound)?;
                handles.insert(id, handle);
            }
            Ok(())
        }
        ControlMessage::Pause(id) => {
            if let Some(handle) = handles.get_mut(&id) {
                handle.pause(Tween::default())?;
            }
            Ok(())
        }
        ControlMessage::ChangeStem(_, _) => todo!(),
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone)]
enum ControlMessage {
    Play(u64),
    Pause(u64),
    ChangeStem(u64, usize),
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Serialize, Deserialize)]
struct Stem {
    tag: String,
    path: String,
}

#[derive(PartialEq, Debug, Clone, Serialize, Deserialize)]
struct Item {
    id: u64,
    name: String,
    stems: Vec<Stem>,
    current_stem: usize,
    volume: f64,
    looped: bool,
    playing: bool,
    colour: Color32,
    // FIXME: remove these
    bar_width: f64,
    width_scale: f64,
    bar_count: u16,
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
                playing: false,
                colour: PALETTE[self.id_counter as usize % PALETTE.len()],
                bar_width: 0.02,
                width_scale: 1.0,
                bar_count: 40,
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
    widget_style.hovered.bg_fill = Color32::GOLD;
    widget_style.active.bg_fill = Color32::GOLD;

    Frame::group(ui.style()).show(ui, |ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(&item.name);
                let verb = if item.playing { "pause" } else { "play" };
                if ui.button(verb).clicked() {
                    item.playing = !item.playing;
                    channel
                        .send(if item.playing {
                            ControlMessage::Play(item.id)
                        } else {
                            ControlMessage::Pause(item.id)
                        })
                        .unwrap();
                }
            });
            render_bar_chart(ui, item);
        });
    });
}

fn render_bar_chart(ui: &mut egui::Ui, item: &mut Item) {
    let id = format!("frequency graph for {}", item.id);
    ui.add(egui::Slider::new(&mut item.bar_width, 0.0..=2.0).text("bar width"));
    ui.add(egui::Slider::new(&mut item.width_scale, 0.01..=3.0).text("width scale"));
    ui.add(egui::Slider::new(&mut item.bar_count, 1..=100).text("bar count"));

    Plot::new(id)
        .height(30.0)
        .width(120.0)
        .show_axes([false, false])
        .show_background(false)
        .show_x(false)
        .show_y(false)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_boxed_zoom(false)
        .show(ui, |plot| {
            let mut data = vec![];
            for i in 0..item.bar_count {
                for direction in [-1.0, 1.0] {
                    let height = (i as f64 / item.width_scale).sin() * 10.0 + 3.0;
                    let mut bar = Bar::new(i as f64 / item.width_scale, direction * height);
                    bar.bar_width = item.bar_width;
                    bar.stroke = Stroke::none();
                    bar.fill = item.colour;
                    data.push(bar);
                }
            }
            let chart = BarChart::new(data);
            plot.bar_chart(chart);
        });
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
