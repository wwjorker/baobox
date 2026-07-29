pub mod batch;
pub mod err;
pub mod image_ops;
pub mod ocr;
pub mod paths;
pub mod pdf_ops;

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
            ocr::ocr_image,
            ocr::ocr_batch,
            ocr::ocr_languages,
            pdf_ops::pdf_merge,
            pdf_ops::pdf_split,
            pdf_ops::pdf_rotate,
            pdf_ops::pdf_from_image,
            pdf_ops::pdf_to_text,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
