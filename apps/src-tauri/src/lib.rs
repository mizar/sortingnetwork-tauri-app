// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod commands;
mod sorting_network_check_v2;
mod threadpool;
use std::sync::{Arc, Mutex};
use tauri::{Listener, Manager};

#[derive(Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct SortingNetworkVerifyId(u32);

impl SortingNetworkVerifyId {
    pub fn inc(&mut self) -> u32 {
        self.0 = self.0.wrapping_add(1);
        self.0
    }
    pub fn get(&self) -> u32 {
        self.0
    }
    pub fn set(&mut self, id: u32) {
        self.0 = id;
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            app.manage(Arc::new(threadpool::ThreadPool::new(
                std::thread::available_parallelism()
                    .map(|x| x.get())
                    .unwrap_or(2),
            )));
            app.manage(Mutex::new(SortingNetworkVerifyId::default()));
            app.listen("frontend", move |event| {
                println!("frontend event: {:?}", event);
            });
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            //commands::greet,
            commands::sorting_network_verify,
            //commands::trigger_backend_event,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
