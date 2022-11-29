use crate::model::*;
use eframe::egui;
use parking_lot::RwLock;
use std::sync::Arc;
use std::sync::mpsc::Sender;

impl eframe::App for SharedModel {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.render_ui(ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let model = self.model.read();
        storage.set_string("model", serde_json::to_string(&*model).unwrap());
    }
}

/// Recover saved state of the application.
pub fn recover(
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
