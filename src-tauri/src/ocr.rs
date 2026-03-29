#[cfg(any(target_os = "macos", target_os = "windows"))]
use crate::insertion::remember_active_window;
use log::{debug, error};
#[cfg(target_os = "windows")]
use std::path::Path;
use tauri::path::BaseDirectory;
use tauri::Manager;

#[tauri::command(async)]
#[specta::specta]
pub fn cut_image(left: u32, top: u32, width: u32, height: u32) {
    use image::GenericImage;
    let app_handle = match crate::APP_HANDLE.get() {
        Some(handle) => handle,
        None => {
            error!("cut_image: APP_HANDLE not initialized");
            return;
        }
    };
    let image_dir = match app_handle
        .path()
        .resolve("ocr_images", BaseDirectory::AppCache)
    {
        Ok(dir) => dir,
        Err(e) => {
            error!("cut_image: failed to resolve ocr_images directory: {:?}", e);
            return;
        }
    };
    let image_file_path = image_dir.join("fullscreen.png");
    if !image_file_path.exists() {
        return;
    }
    let mut img = match image::open(&image_file_path) {
        Ok(v) => v,
        Err(e) => {
            error!("cut_image: failed to open image: {}", e);
            return;
        }
    };
    let img2 = img.sub_image(left, top, width, height);
    let new_image_file_path = image_dir.join("cut.png");
    match img2.to_image().save(&new_image_file_path) {
        Ok(_) => {}
        Err(e) => {
            error!("cut_image: failed to save cut image: {:?}", e);
            return;
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn screenshot(x: i32, y: i32) {
    use screenshots::{Compression, Screen};
    use std::fs;

    let screens = match Screen::all() {
        Ok(screens) => screens,
        Err(e) => {
            error!("screenshot: failed to get screens: {:?}", e);
            return;
        }
    };
    for screen in screens {
        let info = screen.display_info;
        if info.x == x && info.y == y {
            let app_handle = match crate::APP_HANDLE.get() {
                Some(handle) => handle,
                None => {
                    error!("screenshot: APP_HANDLE not initialized");
                    return;
                }
            };
            let image_dir = match app_handle
                .path()
                .resolve("ocr_images", BaseDirectory::AppCache)
            {
                Ok(dir) => dir,
                Err(e) => {
                    error!("screenshot: failed to resolve ocr_images directory: {:?}", e);
                    return;
                }
            };
            if !image_dir.exists() {
                if let Err(e) = std::fs::create_dir_all(&image_dir) {
                    error!("screenshot: failed to create ocr_images directory: {:?}", e);
                    return;
                }
            }
            let image_file_path = image_dir.join("fullscreen.png");
            let image = match screen.capture() {
                Ok(img) => img,
                Err(e) => {
                    error!("screenshot: failed to capture screen: {:?}", e);
                    return;
                }
            };
            let buffer = match image.to_png(Compression::Fast) {
                Ok(buf) => buf,
                Err(e) => {
                    error!("screenshot: failed to convert image to PNG: {:?}", e);
                    return;
                }
            };
            debug!("screenshot: image_file_path: {:?}", image_file_path);
            if let Err(e) = fs::write(&image_file_path, buffer) {
                error!("screenshot: failed to write file: {:?}", e);
                return;
            }
            break;
        }
    }
}

#[cfg(target_os = "linux")]
pub fn do_ocr() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn do_ocr() -> Result<(), Box<dyn std::error::Error>> {
    use crate::windows::show_screenshot_window;
    show_screenshot_window();
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn do_ocr_with_cut_file_path(image_file_path: &Path) {
    use windows::core::HSTRING;
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::{FileAccessMode, StorageFile};

    let path = image_file_path.to_string_lossy().replace("\\\\?\\", "");
    debug!("OCR image file path: {:?}", path);

    let file = match StorageFile::GetFileFromPathAsync(&HSTRING::from(path)) {
        Ok(op) => match op.get() {
            Ok(f) => f,
            Err(e) => {
                error!("OCR: failed to get storage file: {:?}", e);
                return;
            }
        },
        Err(e) => {
            error!("OCR: GetFileFromPathAsync failed: {:?}", e);
            return;
        }
    };

    let stream = match file.OpenAsync(FileAccessMode::Read) {
        Ok(op) => match op.get() {
            Ok(s) => s,
            Err(e) => {
                error!("OCR: failed to open file stream: {:?}", e);
                return;
            }
        },
        Err(e) => {
            error!("OCR: OpenAsync failed: {:?}", e);
            return;
        }
    };

    let decoder_id = match BitmapDecoder::PngDecoderId() {
        Ok(id) => id,
        Err(e) => {
            error!("OCR: failed to get PNG decoder ID: {:?}", e);
            return;
        }
    };

    let bitmap = match BitmapDecoder::CreateWithIdAsync(decoder_id, &stream) {
        Ok(op) => match op.get() {
            Ok(d) => d,
            Err(e) => {
                error!("OCR: failed to create bitmap decoder: {:?}", e);
                return;
            }
        },
        Err(e) => {
            error!("OCR: CreateWithIdAsync failed: {:?}", e);
            return;
        }
    };

    let bitmap = match bitmap.GetSoftwareBitmapAsync() {
        Ok(op) => match op.get() {
            Ok(b) => b,
            Err(e) => {
                error!("OCR: failed to get software bitmap: {:?}", e);
                return;
            }
        },
        Err(e) => {
            error!("OCR: GetSoftwareBitmapAsync failed: {:?}", e);
            return;
        }
    };

    let engine = OcrEngine::TryCreateFromUserProfileLanguages();

    match engine {
        Ok(engine) => {
            let result = match engine.RecognizeAsync(&bitmap) {
                Ok(op) => match op.get() {
                    Ok(r) => r,
                    Err(e) => {
                        error!("OCR: RecognizeAsync get failed: {:?}", e);
                        return;
                    }
                },
                Err(e) => {
                    error!("OCR: RecognizeAsync failed: {:?}", e);
                    return;
                }
            };

            let mut content = String::new();
            if let Ok(lines) = result.Lines() {
                for line in lines {
                    if let Ok(text) = line.Text() {
                        content.push_str(&text.to_string_lossy().trim());
                        content.push('\n');
                    }
                }
            }

            debug!("OCR content: {:?}", content);
            crate::utils::send_text(content);
            remember_active_window();
            crate::windows::show_translator_window(false, true, true);
        }
        Err(e) => {
            error!("OCR engine creation failed: {:?}", e);
            if e.to_string().contains("0x00000000") {
                error!("Language package not installed! See: https://learn.microsoft.com/zh-cn/windows/powertoys/text-extractor#supported-languages");
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub fn do_ocr() -> Result<(), Box<dyn std::error::Error>> {
    use crate::{APP_HANDLE, CPU_VENDOR};

    let mut rel_path = "resources/bin/ocr_intel".to_string();
    if *CPU_VENDOR.lock() == "Apple" {
        rel_path = "resources/bin/ocr_apple".to_string();
    }

    let app = APP_HANDLE.get().ok_or("APP_HANDLE not initialized")?;

    let bin_path = app
        .path()
        .resolve(&rel_path, BaseDirectory::Resource)
        .map_err(|e| {
            format!(
                "Failed to resolve ocr binary resource '{}': {:?}",
                rel_path, e
            )
        })?;

    if !bin_path.exists() {
        return Err(format!(
            "OCR binary not found at {:?}. Please ensure the binary is bundled correctly.",
            bin_path
        )
        .into());
    }

    let output = std::process::Command::new(&bin_path)
        .args(["-l", "zh"])
        .output()
        .map_err(|e| format!("Failed to execute ocr binary at {:?}: {:?}", bin_path, e))?;

    // check exit code
    if output.status.success() {
        // get output content
        let content = String::from_utf8_lossy(&output.stdout);
        crate::utils::send_text(content.to_string());
        remember_active_window();
        crate::windows::show_translator_window(false, true, true);
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "OCR binary failed with exit code {:?}: {}",
            output.status.code(),
            stderr
        )
        .into())
    }
}

#[tauri::command(async)]
#[specta::specta]
pub fn start_ocr() {
    ocr();
}

pub fn ocr() {
    if let Err(e) = do_ocr() {
        error!("OCR failed: {:?}", e);
    }
}

#[tauri::command(async)]
#[specta::specta]
pub fn finish_ocr() {
    do_finish_ocr();
}

#[cfg(target_os = "windows")]
fn do_finish_ocr() {
    let app_handle = match crate::APP_HANDLE.get() {
        Some(handle) => handle,
        None => {
            error!("finish_ocr: APP_HANDLE not initialized");
            return;
        }
    };
    let image_dir = match app_handle
        .path()
        .resolve("ocr_images", BaseDirectory::AppCache)
    {
        Ok(dir) => dir,
        Err(e) => {
            error!("finish_ocr: failed to resolve ocr_images directory: {:?}", e);
            return;
        }
    };
    let image_file_path = image_dir.join("cut.png");
    do_ocr_with_cut_file_path(&image_file_path);
}

#[cfg(target_os = "linux")]
fn do_finish_ocr() {}

#[cfg(target_os = "macos")]
fn do_finish_ocr() {}
