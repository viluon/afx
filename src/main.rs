use eframe::egui;
use rodio::{dynamic_mixer::DynamicMixer, Decoder, OutputStream, Sink};
use std::fs::File;
use std::io::BufReader;

fn main() {
    let options = eframe::NativeOptions {
        drag_and_drop_support: true,
        ..Default::default()
    };

    // start a background thread
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (_stream, stream_handle) = OutputStream::try_default().unwrap();
        while let Ok(file) = rx.recv() {
            let sink = Sink::try_new(&stream_handle).unwrap();
            let file = File::open(file).unwrap();
            let source = Decoder::new(BufReader::new(file)).unwrap();
            sink.append(source);
            sink.detach();
        }
    });

    eframe::run_native(
        "afx",
        options,
        Box::new(|_cc| {
            Box::new(Model {
                play_channel: tx,
                sinks: vec![],
                picked_path: None,
            })
        }),
    );
}

struct Model {
    play_channel: std::sync::mpsc::Sender<String>,
    sinks: Vec<Sink>,
    picked_path: Option<String>,
}

impl eframe::App for Model {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label("Drag-and-drop files onto the window!");

            ui.vertical(|ui| {
                if ui.button("Open file…").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_file() {
                        self.picked_path = Some(path.display().to_string());
                    }
                }

                if let Some(picked_path) = &self.picked_path {
                    ui.horizontal(|ui| {
                        ui.label("Picked file:");
                        ui.monospace(picked_path);

                        if ui.button("play").clicked() {
                            self.play_channel.send(picked_path.clone()).unwrap();
                        }
                    });
                }
            });
        });

        preview_files_being_dropped(ctx);
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
