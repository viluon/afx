use eframe::egui;
use parking_lot::RwLock;
use rodio::{Decoder, OutputStream, Sink, Source};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;

fn main() {
    let options = eframe::NativeOptions {
        drag_and_drop_support: true,
        ..Default::default()
    };

    let (tx, rx) = std::sync::mpsc::channel();
    let model = Arc::new(RwLock::new(Model::default()));

    {
        let model = model.clone();
        // start a background thread for audio playback
        std::thread::spawn(move || playback(rx, model));
    }

    eframe::run_native(
        "afx",
        options,
        Box::new(|_cc| {
            Box::new(SharedModel {
                play_channel: tx,
                model,
            })
        }),
    );
}

fn playback(rx: std::sync::mpsc::Receiver<ControlMessage>, model: Arc<RwLock<Model>>) {
    let (_stream, stream_handle) = OutputStream::try_default().unwrap();
    while let Ok(msg) = rx.recv() {
        match msg {
            ControlMessage::Play(id) => {
                let model = model.read();
                let sink = Sink::try_new(&stream_handle).unwrap();
                let item = model.items.iter().find(|item| item.id == id).unwrap();
                let file = &item.stems[item.current_stem].1;
                let file = File::open(file).unwrap();
                let source = Decoder::new(BufReader::new(file)).unwrap();
                sink.append(source.repeat_infinite());
                sink.detach();
            }
            ControlMessage::Stop(_) => todo!(),
            ControlMessage::ChangeStem(_) => todo!(),
        }
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone)]
enum ControlMessage {
    Play(u64),
    Stop(u64),
    ChangeStem(usize),
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone)]
struct Stem(String, String);

#[derive(PartialEq, PartialOrd, Debug, Clone)]
struct Item {
    id: u64,
    name: String,
    stems: Vec<Stem>,
    current_stem: usize,
    volume: f64,
    looped: bool,
    playing: bool,
}

#[derive(PartialEq, PartialOrd, Debug, Clone, Default)]
struct Model {
    items: Vec<Item>,
    id_counter: u64,
}

struct SharedModel {
    play_channel: std::sync::mpsc::Sender<ControlMessage>,
    model: Arc<RwLock<Model>>,
}

impl eframe::App for SharedModel {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut model = self.model.write();
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label("Drag-and-drop files onto the window!");

            ui.vertical(|ui| {
                if ui.button("Open file…").clicked() {
                    if let Some(paths) = rfd::FileDialog::new().pick_files() {
                        model.import_paths(paths);
                    }
                }

                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    let channel = &self.play_channel;
                    for item in model.items.iter_mut() {
                        item_widget(channel, ui, item);
                    }
                });
            });
        });

        preview_files_being_dropped(ctx);
    }
}

impl Model {
    fn import_paths(&mut self, paths: Vec<PathBuf>) {
        self.items.extend(paths.into_iter().map(|path| {
            let path = path.display().to_string();
            let i = Item {
                id: self.id_counter,
                name: path.clone(),
                stems: vec![Stem("default".to_string(), path)],
                current_stem: 0,
                volume: 1.0,
                looped: false,
                playing: false,
            };
            self.id_counter += 1;
            i
        }))
    }
}

fn item_widget(
    channel: &std::sync::mpsc::Sender<ControlMessage>,
    ui: &mut egui::Ui,
    item: &mut Item,
) {
    ui.horizontal(|ui| {
        ui.label(&item.name);
        let verb = if item.playing { "pause" } else { "play" };
        if ui.button(verb).clicked() {
            item.playing = !item.playing;
            channel
                .send(if item.playing {
                    ControlMessage::Play(item.id)
                } else {
                    ControlMessage::Stop(item.id)
                })
                .unwrap();
        }
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
