use crate::colour_proxy::ExtendedColourOps;
use crate::model::*;
use eframe::egui::plot::{Bar, BarChart, Plot};
use eframe::egui::{Button, RichText, Slider};
use eframe::epaint::{vec2, Color32, Stroke};
use eframe::{egui, egui::Frame};
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

/// This is an ephemeral struct only alive during a single call to
/// [`SharedModel::render_ui`].
struct UIState<'a> {
    model: &'a mut Model,
    channel: Sender<ControlMessage>,
}

impl<'a> UIState<'a> {
    fn new(model: &'a mut Model, channel: Sender<ControlMessage>) -> Self {
        Self { model, channel }
    }

    fn playlist_menu(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
            self.library_button(ui);
            ui.separator();
            self.playlist_list(ui);
            if !self.model.playlists.is_empty() {
                ui.separator();
            }
            self.add_playlist_button(ui);
        });
    }

    fn add_playlist_button(&mut self, ui: &mut egui::Ui) {
        let button = Button::new("➕ Add playlist").fill(GREEN.linear_multiply(0.1));
        if ui.add(button).clicked() && self.model.playlist_creation_state.is_none() {
            self.model.playlist_creation_state = Some(Playlist {
                id: self.model.fresh_id(),
                name: "New playlist".to_string(),
                description: "".to_string(),
                items: vec![],
            });
        }
    }

    fn playlist_list(&mut self, ui: &mut egui::Ui) {
        let mut to_delete = vec![];
        for playlist in self.model.playlists.iter() {
            let resp = ui.selectable_label(
                Some(playlist.id) == self.model.selected_playlist,
                &playlist.name,
            );
            if resp.clicked() {
                self.model.selected_playlist = Some(playlist.id);
            }
            resp.context_menu(|ui| {
                if ui.button(RichText::new("Delete").color(RED)).clicked() {
                    to_delete.push(playlist.id);
                    if Some(playlist.id) == self.model.selected_playlist {
                        self.model.selected_playlist = None;
                    }
                    ui.close_menu();
                }
            });
        }
        self.model.playlists.retain(|p| !to_delete.contains(&p.id));
    }

    fn library_button(&mut self, ui: &mut egui::Ui) {
        let lib = ui.selectable_label(
            self.model.selected_playlist.is_none(),
            RichText::new("📚 library").heading(),
        );
        if lib.clicked() {
            self.model.selected_playlist = None;
        }
    }

    fn search_bar(&mut self, ui: &mut egui::Ui) {
        let search_field =
            egui::TextEdit::singleline(&mut self.model.search_query).hint_text("type to search");
        let resp = ui.add(search_field);
        if !self.model.search_query.is_empty() {
            let button = Button::new("❌").frame(false);
            if ui.add(button).clicked()
                || (resp.lost_focus() && ui.ctx().input().key_pressed(egui::Key::Escape))
            {
                self.model.search_query.clear();
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

    fn items(&mut self, ui: &mut egui::Ui) {
        let filtered_ids = self.process_search();
        self.items_scroll_area(ui, filtered_ids);
    }

    // TODO rename
    fn process_search(&mut self) -> Vec<(usize, u64)> {
        let lowercase_query = self.model.search_query.to_lowercase();
        let pat: Vec<_> = lowercase_query.split_ascii_whitespace().collect();
        let selected_playlist = self.model.selected_playlist.map(|id| {
            self.model
                .playlists
                .iter()
                .find(|p| p.id == id)
                .expect("selected playlist not found")
        });

        self.search_in_playlist(selected_playlist, pat)
    }

    fn search_in_playlist(
        &self,
        selected_playlist: Option<&Playlist>,
        pat: Vec<&str>,
    ) -> Vec<(usize, u64)> {
        let items = selected_playlist
            .map(|p| {
                p.items
                    .iter()
                    .map(|id| self.model.items.iter().find(|i| i.id == *id).unwrap())
                    .collect()
            })
            .unwrap_or(self.model.items.iter().collect::<Vec<_>>());
        items
            .into_iter()
            .enumerate()
            .filter(|(_, item)| {
                pat.iter()
                    .find(|w| "playing".starts_with(**w))
                    .filter(|_| item.status == ItemStatus::Playing)
                    .is_some()
                    || pat.iter().all(|w| item.name.to_lowercase().contains(w))
            })
            .map(|(pos_within_playlist, item)| (pos_within_playlist, item.id))
            .collect::<Vec<_>>()
    }

    fn items_scroll_area(&mut self, ui: &mut egui::Ui, filtered_ids: Vec<(usize, u64)>) {
        let items_per_row = (ui.available_width() / BAR_PLOT_WIDTH).floor() as usize;
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show_rows(
                ui,
                100.0,
                filtered_ids.len() / items_per_row + 1,
                |ui, row_range| {
                    for row in row_range {
                        ui.horizontal(|ui| {
                            for i in 0..items_per_row {
                                let index = row * items_per_row + i;
                                if index >= filtered_ids.len() {
                                    break;
                                }
                                let (position_within_playlist, item_id) = filtered_ids[index];
                                // FIXME ugly data model
                                // we should really decide whether to handle
                                // mutations via message passing or whether to
                                // use mutable references. The latter is more
                                // convenient but the borrow checker doesn't
                                // like it, the former is more verbose but less
                                // error-prone and leads to more modular code.
                                let item_index = self
                                    .model
                                    .items
                                    .binary_search_by_key(&item_id, |i| i.id)
                                    .unwrap();
                                let item = &mut self.model.items[item_index];
                                item.position = ui.ctx().animate_value_with_time(
                                    egui::Id::new(item.id),
                                    item.target_position as f32,
                                    0.06,
                                ) as f64;
                                self.item_frame(position_within_playlist, ui, item_index);
                            }
                        });
                    }
                },
            );
    }

    fn item_frame(
        &mut self,
        position_within_playlist: usize,
        ui: &mut egui::Ui,
        item_index: usize,
    ) {
        let Item { status, colour, .. } = &self.model.items[item_index];

        Frame::group(ui.style())
            .stroke(if matches!(status, ItemStatus::Playing) {
                Stroke::new(1.0, Color32::WHITE)
            } else {
                ui.style().visuals.widgets.noninteractive.bg_stroke
            })
            .fill(colour.linear_multiply(0.03))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    let item = &self.model.items[item_index];

                    render_item_name(ui, item);
                    render_bar_chart(position_within_playlist, &self.channel, ui, item);

                    ui.horizontal(|ui| {
                        self.item_controls(ui, item_index);
                    });
                });
            })
            .response
            .context_menu(|ui| {
                self.item_context_menu(position_within_playlist, item_index, ui);
            });
    }

    fn item_context_menu(
        &mut self,
        pos_within_playlist: usize,
        item_index: usize,
        ui: &mut egui::Ui,
    ) {
        let item = &self.model.items[item_index];
        ui.menu_button("Add to playlist", |ui| {
            for playlist in self.model.playlists.iter() {
                if ui.button(&playlist.name).clicked() {
                    self.channel
                        .send(ControlMessage::AddToPlaylist {
                            item_id: item.id,
                            playlist_id: playlist.id,
                        })
                        .unwrap();
                    ui.close_menu();
                }
            }
        });
        if let Some(playlist_id) = self.model.selected_playlist {
            if ui.button("Remove from playlist").clicked() {
                self.channel
                    .send(ControlMessage::RemoveFromPlaylist {
                        pos_within_playlist,
                        playlist_id,
                    })
                    .unwrap();
                ui.close_menu();
            }
        }
        if ui.button(RichText::new("Delete").color(RED)).clicked() {
            self.channel.send(ControlMessage::Delete(item.id)).unwrap();
            ui.close_menu();
        }
    }

    fn item_controls(&mut self, ui: &mut egui::Ui, item_index: usize) {
        let item = &mut self.model.items[item_index];
        match item.status {
            ItemStatus::Stopped | ItemStatus::Paused => {
                if ui.button(RichText::new("▶").heading()).clicked() {
                    item.status = ItemStatus::Loading;
                    self.channel.send(ControlMessage::Play(item.id)).unwrap();
                }
            }
            ItemStatus::Loading => {
                ui.spinner();
            }
            ItemStatus::Playing => {
                if ui.button(RichText::new("⏸").heading()).clicked() {
                    item.status = ItemStatus::Paused;
                    self.channel.send(ControlMessage::Pause(item.id)).unwrap();
                }
            }
        };

        let loop_button = Button::new(if item.looped { "🔁" } else { "🔂" }).frame(item.looped);
        let resp = ui.add(loop_button).on_hover_text(if item.looped {
            "Disable looping"
        } else {
            "Enable looping"
        });
        if resp.clicked() {
            item.looped = !item.looped;
            self.channel
                .send(ControlMessage::Loop(item.id, item.looped))
                .unwrap();
        }

        if ui.button(if item.muted { "🔇" } else { "🔈" }).clicked() {
            item.muted = !item.muted;
            self.channel
                .send(ControlMessage::Mute(item.id, item.muted))
                .unwrap();
        }

        let original_volume = item.volume;
        ui.add(Slider::new(&mut item.volume, 0.0001..=1.0).show_value(false));
        if original_volume != item.volume {
            self.channel
                .send(ControlMessage::SetVolume(item.id, item.volume))
                .unwrap();
        }

        let minutes = (item.position / 60.0).floor() as u32;
        let seconds = item.position % 60.0;
        ui.label(format!("{:01}:{:05.2}", minutes, seconds));
    }

    fn add_imported_items(&mut self, items: Vec<Item>) {
        if let Some(playlist_id) = self.model.selected_playlist {
            for item in items.iter() {
                self.channel
                    .send(ControlMessage::AddToPlaylist {
                        item_id: item.id,
                        playlist_id,
                    })
                    .unwrap();
            }
        }
        self.model.items.extend(items);
    }

    fn playlist_creation_window(&mut self, ui: &mut egui::Ui) {
        if let Some(playlist) = &self.model.playlist_creation_state {
            let mut playlist = playlist.clone();

            egui::Window::new("Create playlist")
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.text_edit_singleline(&mut playlist.name);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Description:");
                        egui::TextEdit::multiline(&mut playlist.description)
                            .desired_rows(3)
                            .show(ui);
                    });

                    self.model.playlist_creation_state = Some(playlist.clone());
                    ui.horizontal(|ui| {
                        if ui.button(RichText::new("Discard").heading()).clicked() {
                            self.model.playlist_creation_state = None;
                        } else if ui.button(RichText::new("Create").heading()).clicked() {
                            self.model.playlists.push(playlist.clone());
                            self.model.playlist_creation_state = None;
                        }
                    });
                })
        } else {
            None
        };
    }

    fn render_import_progress(
        &mut self,
        rx: &Receiver<ImportMessage>,
        state: SharedImportState,
        ui: &mut egui::Ui,
    ) -> (bool, Option<Vec<Item>>) {
        let mut keep_window_open = true;
        let mut imported = None;
        let mut state = state.write();

        let title = format!(
            "Import ({}/{})",
            state
                .items_in_progress
                .iter()
                .filter(|(_, _, s)| *s == ItemImportStatus::Finished)
                .count(),
            state.items_in_progress.len()
        );

        egui::Window::new(title)
            .id(egui::Id::new("import window"))
            .scroll2([false, true])
            .resizable(false)
            .default_pos(egui::Pos2::new(
                ui.available_size_before_wrap().x / 2.0,
                ui.available_size().y / 2.0,
            ))
            .show(ui.ctx(), |ui| {
                let start_time = std::time::Instant::now();
                while let Ok(msg) = rx.try_recv() {
                    crate::import::process_import_message(
                        msg,
                        ui,
                        &mut keep_window_open,
                        &mut state,
                    );
                    if start_time.elapsed() > std::time::Duration::from_millis(30) {
                        break;
                    }
                }

                ui.vertical(|ui| {
                    if state.items_in_progress.is_empty() {
                        ui.vertical_centered(|ui| ui.heading("Waiting for file selection..."));
                        return;
                    }

                    let mut finished = 0;
                    for (_, name, status) in state.items_in_progress.iter() {
                        show_import_progress_indicator(ui, status, &mut finished, name);
                    }

                    ui.horizontal(|ui| {
                        if ui
                            .button(RichText::new("Discard").heading().color(RED))
                            .clicked()
                        {
                            keep_window_open = false;
                        }
                        let target = self.get_selected_playlist_name();
                        let import_action =
                            RichText::new(format!("Add {} tracks to {}", finished, target))
                                .heading()
                                .color(GREEN);
                        if ui.button(import_action).clicked() {
                            keep_window_open = false;
                            imported = Some(state.finished.drain(..).collect());
                        }
                    });
                });
            });
        (keep_window_open, imported)
    }

    fn get_selected_playlist_name(&self) -> &str {
        if let Some(playlist_id) = self.model.selected_playlist {
            &self
                .model
                .playlists
                .iter()
                .find(|p| p.id == playlist_id)
                .unwrap()
                .name
        } else {
            "library"
        }
    }

    fn render_top_button_bar(&mut self, ui: &mut egui::Ui) -> [egui::Response; 5] {
        let import_button = Button::new(RichText::new("Import").heading().color(Color32::BLACK))
            .fill(Color32::GOLD);
        let import_button_resp = ui.add(import_button);
        let play_resp = ui.add(
            Button::new(RichText::new("▶").heading().color(Color32::BLACK)).fill(
                if self.model.selected_playlist.is_some() {
                    Color32::GREEN
                } else {
                    Color32::GRAY
                },
            ),
        );

        let pause_resp = ui.add(
            Button::new(RichText::new("⏸").heading().color(Color32::BLACK)).fill(Color32::YELLOW),
        );
        let stop_resp = ui.add(
            Button::new(RichText::new("⏹").heading().color(Color32::BLACK)).fill(Color32::RED),
        );
        let search_to_playlist_resp = ui.add(
            Button::new(RichText::new("into playlist")),
        );

        [import_button_resp, play_resp, pause_resp, stop_resp, search_to_playlist_resp]
    }

    fn handle_playback_control_buttons(
        &mut self,
        play_resp: egui::Response,
        pause_resp: egui::Response,
        stop_resp: egui::Response,
    ) {
        if let Some(id) = self.model.selected_playlist {
            self.channel.send(ControlMessage::PlayFromPlaylist(id)).unwrap();
        }
        if pause_resp.clicked() {
            self.channel.send(ControlMessage::GlobalPause).unwrap();
        }
        if stop_resp.clicked() {
            self.channel.send(ControlMessage::GlobalStop).unwrap();
        }
    }

    /// Create a new playlist from the current search.
    fn playlist_from_search(&mut self) {
        if self.model.playlist_creation_state.is_none() {
            self.model.playlist_creation_state = Some(Playlist {
                id: self.model.fresh_id(),
                name: "new playlist".to_string(),
                description: "".to_string(),
                items: self.process_search().into_iter().map(|(_, item_id)| item_id).collect(),
            });
        }
    }
}

fn render_item_name(ui: &mut egui::Ui, item: &Item) {
    ui.vertical(|ui| {
        ui.set_max_size(vec2(BAR_PLOT_WIDTH, 0.0));

        let font_id = egui::TextStyle::Heading.resolve(ui.style());
        let mut job = eframe::epaint::text::LayoutJob::single_section(
            item.name.clone(),
            egui::TextFormat {
                font_id,
                color: Color32::WHITE,
                ..Default::default()
            },
        );

        job.wrap = eframe::epaint::text::TextWrapping {
            overflow_character: None,
            break_anywhere: true,
            max_rows: 1,
            ..Default::default()
        };

        ui.label(job).on_hover_text_at_pointer(&item.name);
    });
}

fn show_import_progress_indicator(
    ui: &mut egui::Ui,
    status: &ItemImportStatus,
    finished: &mut i32,
    name: &String,
) {
    ui.horizontal(|ui| {
        match status {
            ItemImportStatus::Queued(_) => (),
            ItemImportStatus::Waiting => {
                ui.label("…")
                    .on_hover_text_at_pointer("waiting to begin processing…");
            }
            ItemImportStatus::InProgress => {
                ui.spinner().on_hover_text_at_pointer("processing…");
            }
            ItemImportStatus::Finished => {
                ui.colored_label(GREEN, "✔")
                    .on_hover_text_at_pointer("finished");
                *finished += 1;
            }
            ItemImportStatus::Failed(err) => {
                ui.colored_label(RED, "🗙").on_hover_text_at_pointer(err);
            }
        }
        ui.label(name);
    });
}

impl SharedModel {
    pub fn render_ui(&mut self, ctx: &egui::Context) {
        let model = self.model.clone();
        let mut model = model.write();
        ctx.request_repaint_after(std::time::Duration::from_millis(PLAYBACK_SYNC_INTERVAL));

        let mut state = UIState::new(&mut model, self.play_channel.clone());

        egui::SidePanel::left("playlist menu")
            .resizable(true)
            .default_width(150.0)
            .width_range(120.0..=400.0)
            .show(ctx, |ui| {
                state.playlist_menu(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.allocate_ui_with_layout(
                vec2(ui.available_size_before_wrap().x, 0.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    state.search_bar(ui);
                    state.playlist_creation_window(ui);

                    let [import_button_response, play_resp, pause_resp, stop_resp, into_playlist_resp] =
                        state.render_top_button_bar(ui);

                    state.handle_playback_control_buttons(play_resp, pause_resp, stop_resp);
                    if into_playlist_resp.clicked() {
                        state.playlist_from_search();
                    }

                    if import_button_response.clicked() && self.import_state.is_none() {
                        self.begin_import();
                    }
                    if let Some((rx, import_state)) = &self.import_state {
                        let (keep_win_open, imported) =
                            state.render_import_progress(rx, import_state.clone(), ui);
                        if !keep_win_open {
                            self.import_state = None;
                        }
                        if let Some(items) = imported {
                            info!("importing {} items", items.len());
                            state.add_imported_items(items);
                        }
                    }
                },
            );

            ui.vertical(|ui| {
                state.items(ui);
            })
        });

        preview_files_being_dropped(ctx);
    }
}

fn render_bar_chart(
    unique_id: usize,
    channel: &Sender<ControlMessage>,
    ui: &mut egui::Ui,
    item: &Item,
) {
    let id = format!("frequency graph for {}, {}", item.id, unique_id);
    let bg = ui.style().visuals.window_fill();
    let dimmed = bg.mix(0.4, &item.colour);

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
        .show_axes([false; 2])
        .show_background(false)
        .show_x(false)
        .show_y(false)
        .show(ui, |plot| {
            let mut data = Vec::with_capacity(item.bars.len() * 2);
            for (i, height) in item.bars.iter().copied().enumerate() {
                let height = height as f64 / 255.0;
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

    handle_bar_chart_interaction(channel, resp.response, plot_x, item);
}

fn handle_bar_chart_interaction(
    channel: &Sender<ControlMessage>,
    response: egui::Response,
    plot_x: f32,
    item: &Item,
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
