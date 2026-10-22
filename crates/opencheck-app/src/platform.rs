//! The few things that differ between the desktop app and the browser
//! (WebAssembly): background work, file dialogs and saving files.
//!
//! On the desktop, work runs on threads and files go through the system's
//! dialogs. In the browser there are no threads or file paths: work runs at
//! once, "saving" downloads the file, and picked files arrive later as bytes
//! (see [`take_picked`]).

use std::path::PathBuf;

/// Whether this build runs in a browser.
pub const IS_WEB: bool = cfg!(target_arch = "wasm32");

/// A file type for dialogs, e.g. `("IFC model", &["ifc"])`.
pub type Filter<'a> = (&'a str, &'a [&'a str]);

/// Runs work in the background on the desktop. In the browser it runs on the
/// page itself, which waits while it does; it starts a moment later so the
/// "Loading…" message is drawn first.
pub fn spawn(work: impl FnOnce() + Send + 'static) {
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(work);
    #[cfg(target_arch = "wasm32")]
    web::after_next_frame(work);
}

/// Whether files picked or dropped in the browser are still being read.
pub fn reading_files() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web::reading()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

/// Asks for one file to open. None when cancelled, and always in the
/// browser, where files are picked with [`pick_in_browser`].
pub fn pick_file(title: &str, filter: Filter) -> Option<PathBuf> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        rfd::FileDialog::new()
            .set_title(title)
            .add_filter(filter.0, filter.1)
            .pick_file()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (title, filter);
        None
    }
}

/// Asks for several files to open (desktop only, as [`pick_file`]).
pub fn pick_files(title: &str, filter: Filter) -> Option<Vec<PathBuf>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        rfd::FileDialog::new()
            .set_title(title)
            .add_filter(filter.0, filter.1)
            .pick_files()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (title, filter);
        None
    }
}

/// Asks where to save a file. In the browser the answer is just the file
/// name: [`write`] then downloads it.
pub fn save_file(title: &str, filter: Filter, name: &str) -> Option<PathBuf> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        rfd::FileDialog::new()
            .set_title(title)
            .add_filter(filter.0, filter.1)
            .set_file_name(name)
            .save_file()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (title, filter);
        Some(PathBuf::from(name))
    }
}

/// Writes a file chosen with [`save_file`]; in the browser it is downloaded.
pub fn write(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::write(path, bytes)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "download".to_owned());
        web::download(&name, bytes).map_err(std::io::Error::other)
    }
}

/// What a file picked in the browser is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    AddModels,
    OpenProject,
    ImportRuleset,
    ImportIds,
}

/// A file picked in the browser: its name and content.
pub struct PickedFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Opens the browser's file picker; the files arrive with [`take_picked`].
/// Does nothing on the desktop, where the system dialogs are used.
pub fn pick_in_browser(kind: Pick, filter: Filter, multiple: bool, ctx: &eframe::egui::Context) {
    #[cfg(target_arch = "wasm32")]
    web::pick(kind, filter, multiple, ctx.clone());
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (kind, filter, multiple, ctx);
}

/// Files picked in the browser since the last call.
pub fn take_picked() -> Vec<(Pick, Vec<PickedFile>)> {
    #[cfg(target_arch = "wasm32")]
    {
        web::take()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        Vec::new()
    }
}

/// Reads files dropped on the page; they arrive with [`take_picked`] as
/// models to add. Does nothing on the desktop, where dropped files have paths.
pub fn read_dropped(
    files: Vec<std::sync::Arc<dyn eframe::egui::DroppedFile + Send + Sync>>,
    ctx: &eframe::egui::Context,
) {
    #[cfg(target_arch = "wasm32")]
    web::read_dropped(files, ctx.clone());
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (files, ctx);
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;

    use eframe::wasm_bindgen::JsCast;

    use super::{Filter, Pick, PickedFile};

    thread_local! {
        static PICKED: RefCell<Vec<(Pick, Vec<PickedFile>)>> = const { RefCell::new(Vec::new()) };
        /// The last download's address, released at the next download so a
        /// large file has time to start.
        static LAST_DOWNLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
        /// Picks and drops whose files are still being read.
        static READING: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    pub fn reading() -> bool {
        READING.with(|r| r.get() > 0)
    }

    /// Runs `work` once the browser has had time to draw a frame (egui draws
    /// on the next animation frame, well within the delay).
    pub fn after_next_frame(work: impl FnOnce() + 'static) {
        let callback = eframe::wasm_bindgen::closure::Closure::once_into_js(work);
        let scheduled = web_sys::window().map(|window| {
            window
                .set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), 60)
        });
        if !matches!(scheduled, Some(Ok(_))) {
            log::warn!("could not schedule background work");
        }
    }

    /// Counts a read in progress until the returned guard is dropped.
    fn start_reading(ctx: &eframe::egui::Context) -> impl Drop {
        struct Reading(eframe::egui::Context);
        impl Drop for Reading {
            fn drop(&mut self) {
                READING.with(|r| r.set(r.get().saturating_sub(1)));
                self.0.request_repaint();
            }
        }
        READING.with(|r| r.set(r.get() + 1));
        ctx.request_repaint();
        Reading(ctx.clone())
    }

    pub fn pick(kind: Pick, filter: Filter, multiple: bool, ctx: eframe::egui::Context) {
        let dialog = rfd::AsyncFileDialog::new().add_filter(filter.0, filter.1);
        wasm_bindgen_futures::spawn_local(async move {
            let handles = if multiple {
                dialog.pick_files().await.unwrap_or_default()
            } else {
                dialog.pick_file().await.into_iter().collect()
            };
            let _reading = (!handles.is_empty()).then(|| start_reading(&ctx));
            let mut files = Vec::new();
            for handle in handles {
                files.push(PickedFile {
                    name: handle.file_name(),
                    bytes: handle.read().await,
                });
            }
            if !files.is_empty() {
                PICKED.with(|p| p.borrow_mut().push((kind, files)));
                ctx.request_repaint();
            }
        });
    }

    pub fn read_dropped(
        files: Vec<std::sync::Arc<dyn eframe::egui::DroppedFile + Send + Sync>>,
        ctx: eframe::egui::Context,
    ) {
        if files.is_empty() {
            return;
        }
        wasm_bindgen_futures::spawn_local(async move {
            let _reading = start_reading(&ctx);
            let mut picked = Vec::new();
            for file in files {
                if let Ok(bytes) = file.bytes_async().await {
                    picked.push(PickedFile {
                        name: file.path().to_string_lossy().into_owned(),
                        bytes,
                    });
                }
            }
            if !picked.is_empty() {
                PICKED.with(|p| p.borrow_mut().push((Pick::AddModels, picked)));
                ctx.request_repaint();
            }
        });
    }

    pub fn take() -> Vec<(Pick, Vec<PickedFile>)> {
        PICKED.with(|p| std::mem::take(&mut *p.borrow_mut()))
    }

    /// Hands bytes to the browser as a download.
    pub fn download(name: &str, bytes: &[u8]) -> Result<(), String> {
        let error = |e: eframe::wasm_bindgen::JsValue| format!("{e:?}");
        let window = web_sys::window().ok_or("no window")?;
        let document = window.document().ok_or("no document")?;
        let array = js_sys::Uint8Array::from(bytes);
        let parts = js_sys::Array::of1(&array);
        let blob = web_sys::Blob::new_with_u8_array_sequence(&parts).map_err(error)?;
        let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(error)?;
        let anchor: web_sys::HtmlAnchorElement = document
            .create_element("a")
            .map_err(error)?
            .dyn_into()
            .map_err(|_| "not an anchor")?;
        anchor.set_href(&url);
        anchor.set_download(name);
        anchor.click();
        if let Some(previous) = LAST_DOWNLOAD.with(|last| last.borrow_mut().replace(url)) {
            web_sys::Url::revoke_object_url(&previous).map_err(error)?;
        }
        Ok(())
    }
}
