pub mod batch;
pub mod dedupe;
pub mod err;
pub mod image_ops;
pub mod ocr;
pub mod paths;
pub mod pdf_font;
pub mod pdf_ops;
pub mod pdf_render;
pub mod redact;
pub mod rename;
pub mod screen_ocr;
pub mod watermark;

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
            pdf_ops::pdf_compress,
            pdf_ops::pdf_decrypt,
            pdf_ops::pdf_stamp,
            pdf_render::pdf_to_image,
            dedupe::find_duplicates,
            dedupe::delete_to_trash,
            dedupe::cancel_scan,
            rename::rename_preview,
            rename::rename_apply,
            rename::rename_undo,
            screen_ocr::capture_screen,
            screen_ocr::ocr_region,
            screen_ocr::cursor_pos,
            redact::img_redact,
            redact::image_preview,
            watermark::img_watermark,
            dedupe::dir_exists,
            pdf_ops::pdf_to_text,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
