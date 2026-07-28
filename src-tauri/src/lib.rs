pub mod err;
pub mod image_ops;
pub mod paths;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            image_ops::img_compress_target,
            image_ops::img_compress,
            image_ops::img_convert,
            image_ops::img_resize,
            image_ops::img_strip_exif,
            image_ops::stat_files,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
