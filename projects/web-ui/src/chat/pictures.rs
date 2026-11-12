//! Pictures the user adds to a message: picked, pasted or dropped. Each is read into
//! memory with a preview URL; on send each goes to the core, which keeps it among the
//! chat's sources and gives back the tag the message carries.

use leptos::{prelude::*, task::spawn_local};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use dusk_protocol::client::MAX_PICTURE_UPLOAD;

#[derive(Clone)]
pub struct Picture {
    pub name: String,
    pub bytes: Vec<u8>,
    /// A `blob:` URL for the thumbnail; revoked when the picture leaves.
    pub preview: String,
}

impl Picture {
    pub fn forget(&self) {
        let _ = web_sys::Url::revoke_object_url(&self.preview);
    }
}

/// Reads the files that are pictures into `pictures`; a file too large or of another
/// kind is refused with a toast.
pub fn add_files(files: Vec<web_sys::File>, pictures: RwSignal<Vec<Picture>>) {
    let app = crate::state::state();
    for file in files {
        if !matches!(file.type_().as_str(), "image/png" | "image/jpeg") {
            app.fail(format!("{} is not a PNG or JPEG picture", file.name()));
            continue;
        }
        if file.size() > MAX_PICTURE_UPLOAD as f64 {
            app.fail(format!(
                "{} is over {} MiB",
                file.name(),
                MAX_PICTURE_UPLOAD / (1024 * 1024)
            ));
            continue;
        }
        spawn_local(async move {
            let Ok(buffer) = JsFuture::from(file.array_buffer()).await else {
                return app.fail(format!("cannot read {}", file.name()));
            };
            let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
            let preview = web_sys::Url::create_object_url_with_blob(&file).unwrap_or_default();
            pictures.update(|list| {
                list.push(Picture {
                    name: file.name(),
                    bytes,
                    preview,
                })
            });
        });
    }
}

/// The files of a list, such as an input's or a drop's.
pub fn files_of(list: Option<web_sys::FileList>) -> Vec<web_sys::File> {
    let Some(list) = list else {
        return Vec::new();
    };
    (0..list.length())
        .filter_map(|index| list.get(index))
        .collect()
}

/// The pictures a paste carries; text pastes as text.
pub fn pasted(event: &web_sys::ClipboardEvent) -> Vec<web_sys::File> {
    files_of(event.clipboard_data().and_then(|data| data.files()))
}

/// The pictures dropped on the box.
pub fn dropped(event: &web_sys::DragEvent) -> Vec<web_sys::File> {
    files_of(event.data_transfer().and_then(|data| data.files()))
}

/// The thumbnails waiting to go, each with a button that takes it back.
#[component]
pub fn Strip(pictures: RwSignal<Vec<Picture>>) -> impl IntoView {
    view! {
        <Show when=move || pictures.with(|list| !list.is_empty())>
            <div class="pictures">
                {move || pictures.get().into_iter().enumerate().map(|(index, picture)| view! {
                    <div class="picture" title=picture.name.clone()>
                        <img src=picture.preview.clone() alt=picture.name.clone() />
                        <button class="icon" title="Take it back" on:click=move |_| {
                            pictures.update(|list| {
                                if index < list.len() {
                                    list.remove(index).forget();
                                }
                            })
                        }>"×"</button>
                    </div>
                }).collect_view()}
            </div>
        </Show>
    }
}

/// The hidden file input behind the attach button.
pub fn pick(input: &web_sys::HtmlInputElement, pictures: RwSignal<Vec<Picture>>) {
    add_files(files_of(input.files()), pictures);
    input.set_value("");
}

pub fn input_of(event: &leptos::ev::Event) -> Option<web_sys::HtmlInputElement> {
    event.target()?.dyn_into().ok()
}
