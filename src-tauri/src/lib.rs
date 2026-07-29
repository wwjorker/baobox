pub mod batch;
pub mod dedupe;
pub mod err;
pub mod image_edit;
pub mod image_ops;
pub mod ocr;
pub mod paths;
pub mod pdf_font;
pub mod pdf_ops;
pub mod pdf_render;
pub mod redact;
pub mod rename;
pub mod screen_ocr;
pub mod textfile;
pub mod watermark;

/// 截图取字的全局热键。
///
/// 这个功能一半的价值在于「正在别的软件里看东西，随手一按就取字」。
/// 必须先切回百宝箱的话，那一半就没了。
const SCREEN_OCR_HOTKEY: &str = "Ctrl+Shift+S";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    // 只认按下，不然一次按键会触发两遍
                    if event.state() != tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        return;
                    }
                    use tauri::Emitter;
                    let _ = app.emit("baobox://hotkey-screen-ocr", ());
                })
                .build(),
        )
        .setup(|app| {
            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            // 热键可能被别的软件占用。抢不到就静默让步——
            // 弹个报错打断启动，比少一个快捷方式糟糕得多。
            if let Err(e) = app.global_shortcut().register(SCREEN_OCR_HOTKEY) {
                eprintln!("全局热键 {SCREEN_OCR_HOTKEY} 注册失败（可能已被占用）: {e}");
            }
            Ok(())
        })
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
            screen_ocr::restore_and_focus,
            redact::img_redact,
            redact::image_preview,
            watermark::img_watermark,
            dedupe::dir_exists,
            pdf_ops::pdf_to_text,
            image_ops::thumbs,
            image_ops::expand_inputs,
            paths::set_output_dir,
            batch::cancel_batch,
            image_edit::img_grid,
            image_edit::img_stitch,
            image_edit::img_trim,
            image_edit::img_frame,
            image_edit::img_adjust,
            textfile::text_fix_encoding,
            textfile::file_hash,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
