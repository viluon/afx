use crate::model::*;
use anyhow::{anyhow, Result};
use eframe::egui;
use parking_lot::RwLock;
use std::sync::mpsc::Sender;
use std::sync::Arc;

impl eframe::App for SharedModel {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.render_ui(ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let model = self.model.read();
        storage.set_string("model", serialize(&*model).unwrap());
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }
}

fn serialize<T: serde::Serialize + ?Sized>(value: &T) -> Result<String> {
    Ok(base64::encode(lz4_flex::compress_prepend_size(
        &rmp_serde::to_vec(value)?,
    )))
}

fn deserialize<T: for<'de> serde::Deserialize<'de>>(saved: impl AsRef<[u8]>) -> Result<T> {
    base64::decode(saved)
        .map_err(|e| anyhow!(e))
        .and_then(|decoded| lz4_flex::decompress_size_prepended(&decoded).map_err(|e| anyhow!(e)))
        .and_then(|decompressed| rmp_serde::from_slice(&decompressed).map_err(|e| anyhow!(e)))
}

/// Recover saved state of the application.
pub fn recover(
    cc: &eframe::CreationContext,
    tx: Sender<ControlMessage>,
    model: Arc<RwLock<Model>>,
) -> Option<()> {
    let saved = cc.storage?.get_string("model")?;
    let mut loaded: Model = match deserialize(saved) {
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
