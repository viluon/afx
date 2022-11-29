use crate::colour_proxy::ExtendedColourOps;
use crate::model::*;
use eframe::{egui, egui::Frame};
use eframe::egui::{Button, RichText, Slider};
use eframe::egui::plot::{Bar, BarChart, Plot};
use eframe::epaint::{vec2, Color32, Stroke};
use parking_lot::{RwLockWriteGuard};
use std::sync::mpsc::{Receiver, Sender};
use tracing::info;

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

pub use colours::*;
pub const PALETTE: [Color32; 12] = [
    ORANGE, YELLOW, PURPLE, PINK, BURGUNDY, SALMON, TEAL, BROWN, CREAM, RED, GREEN, BLUE,
];

pub const BARS: usize = 128;
pub const BAR_PLOT_WIDTH: f32 = 360.0;
pub const PLAYBACK_SYNC_INTERVAL: u64 = 50;

impl SharedModel {
    pub fn render_ui(&mut self, ctx: &egui::Context) {
        let model = self.model.clone();
        let mut model = model.write();
        ctx.request_repaint_after(std::time::Duration::from_millis(PLAYBACK_SYNC_INTERVAL));

        egui::SidePanel::left("playlist menu")
            .resizable(true)
            .default_width(150.0)
            .width_range(120.0..=400.0)
            .show(ctx, |ui| {
                // TODO: implement playlists
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    let mut s = true;
                    ui.toggle_value(&mut s, RichText::new("📚 library").heading());
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.allocate_ui_with_layout(
                vec2(ui.available_size_before_wrap().x, 0.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    render_search_bar(&mut model, ui);
                },
            );

            let desired_size = ui.ctx().input().screen_rect.size();
            let desired_size = vec2(desired_size.x * 0.9, 95.0);

            ui.with_layout(
                egui::Layout::left_to_right(egui::Align::LEFT).with_main_wrap(true),
                |ui| {
                    ui.set_max_size(desired_size);
                    let channel = &self.play_channel;
                    render_items(&mut model, channel, ui);
                    let import_button =
                        Button::new(RichText::new("Import").heading().color(Color32::BLACK))
                            .fill(Color32::GOLD);
                    if ui.add(import_button).clicked() && self.import_state.is_none() {
                        self.begin_import();
                    }

                    if let Some((rx, state)) = &self.import_state {
                        let (keep_win_open, imported) =
                            render_import_progress(rx, state.clone(), ui);
                        if !keep_win_open {
                            self.import_state = None;
                        }
                        if let Some(items) = imported {
                            info!("importing {} items", items.len());
                            model.items.extend(items);
                        }
                    }
                },
            )
        });

        preview_files_being_dropped(ctx);
    }
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
                    },
                );
            });
        })
        .response
        .context_menu(|ui| {
            if ui.button(RichText::new("Delete").color(RED)).clicked() {
                channel.send(ControlMessage::Delete(item.id)).unwrap();
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
                    let fill_level = ((item.position / item.duration) * item.bars.len() as f64
                        - i as f64)
                        .clamp(0.0, 1.0);
                    bar.fill = dimmed.mix(fill_level as f32, &item.colour);
                    data.push(bar);
                }
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
    let response = response.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
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


fn render_search_bar(model: &mut RwLockWriteGuard<Model>, ui: &mut egui::Ui) {
    let search_field =
        egui::TextEdit::singleline(&mut model.search_query).hint_text("type to search");
    let resp = ui.add(search_field);
    if !model.search_query.is_empty() {
        let button = Button::new("❌").frame(false);
        if ui.add(button).clicked()
            || (resp.lost_focus() && ui.ctx().input().key_pressed(egui::Key::Escape))
        {
            model.search_query.clear();
            resp.request_focus();
        }
    }
    if ui
        .ctx()
        .input_mut()
        .consume_key(egui::Modifiers::CTRL, egui::Key::F)
    {
        resp.request_focus();
    }
}

fn render_items(
    model: &mut RwLockWriteGuard<Model>,
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
}

fn render_import_progress(
    rx: &Receiver<ImportMessage>,
    state: SharedImportStatus,
    ui: &mut egui::Ui,
) -> (bool, Option<Vec<Item>>) {
    let mut keep_window_open = true;
    let mut imported = None;

    egui::Window::new("Import")
        .scroll2([false, true])
        .resizable(false)
        .default_pos(egui::Pos2::new(
            ui.available_size_before_wrap().x / 2.0,
            ui.available_size().y / 2.0,
        ))
        .show(ui.ctx(), |ui| {
            let mut state = state.write();

            if let Ok(msg) = rx.try_recv() {
                crate::import::process_import_message(msg, ui, &mut keep_window_open, &mut state);
            }

            ui.vertical(|ui| {
                if state.0.is_empty() {
                    ui.vertical_centered(|ui| ui.heading("Waiting for file selection..."));
                    return;
                }

                for (_, name, status) in state.0.iter() {
                    ui.horizontal(|ui| {
                        match status {
                            ItemImportStatus::Queued(_) => (),
                            ItemImportStatus::InProgress => {
                                ui.spinner();
                            }
                            ItemImportStatus::Finished => {
                                ui.colored_label(GREEN, "✔");
                            }
                            ItemImportStatus::Failed(err) => {
                                ui.colored_label(RED, "🗙").on_hover_text_at_pointer(err);
                            }
                        }
                        ui.label(name);
                    });
                }

                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Discard").color(RED)).clicked() {
                        keep_window_open = false;
                    }
                    if ui.button(RichText::new("Import").color(GREEN)).clicked() {
                        keep_window_open = false;
                        imported = Some(state.1.drain(..).collect());
                    }
                });
            });
        });
    (keep_window_open, imported)
}
