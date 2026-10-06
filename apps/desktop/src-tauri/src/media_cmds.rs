//! The properties strip and images in notes (FR-EDT-006/007/016/017). Errors are `<code>|<detail>|`.

use chrono::Local;
use pla_core::media::{self, FrontMatter, MediaError};
use serde::Serialize;
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::{AppHandle, Emitter, State};

use crate::commands::AppState;
use crate::fileops_cmds::code;

fn media_code(e: MediaError) -> String {
    match e {
        MediaError::NotImage => "not_image||".into(),
        MediaError::TooBig => "too_big|20|".into(),
        MediaError::File(e) => code(e),
        MediaError::Io(e) => format!("io||{e}"),
    }
}

/// FR-EDT-006/007: the open note's tags and aliases, from the editor's current text.
#[tauri::command]
pub fn note_meta(text: String) -> FrontMatter {
    media::read_frontmatter(&text)
}

/// FR-EDT-016: the bytes of the image an embed target shows (an ArrayBuffer in the page).
#[tauri::command]
pub fn image_bytes(state: State<AppState>, target: String) -> Result<Response, String> {
    let guard = state.session.lock().expect("session lock");
    let session = guard.as_ref().ok_or("no_vault||")?;
    let rel = media::resolve_image(&session.vault, &target).ok_or_else(|| format!("missing|{target}|"))?;
    let bytes = media::read_image(&session.vault, &rel).map_err(media_code)?;
    Ok(Response::new(bytes))
}

#[derive(Serialize)]
pub struct SavedImage {
    path: String,
    /// What goes inside `![[…]]`.
    embed: String,
}

/// FR-EDT-017: a pasted or dropped image, sent as the raw request body; a dropped file's name comes
/// in the `x-name` header (URI-encoded), a pasted image has none.
#[tauri::command]
pub fn save_image(app: AppHandle, state: State<AppState>, request: Request<'_>) -> Result<SavedImage, String> {
    let InvokeBody::Raw(bytes) = request.body() else { return Err("not_image||".into()) };
    let name = request
        .headers()
        .get("x-name")
        .and_then(|v| v.to_str().ok())
        .map(percent_decode)
        .filter(|n| !n.trim().is_empty());
    let saved = {
        let guard = state.session.lock().expect("session lock");
        let session = guard.as_ref().ok_or("no_vault||")?;
        let path = media::save_attachment(&session.vault, bytes, name.as_deref(), Local::now().fixed_offset()).map_err(media_code)?;
        let embed = media::embed_target(&session.vault, &path);
        SavedImage { path, embed }
    };
    let _ = app.emit("tree-changed", ());
    Ok(saved)
}

/// `encodeURIComponent` undone (a header carries only ASCII).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(b) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_names_survive_the_header() {
        assert_eq!(percent_decode("Ekran%20g%C3%B6r%C3%BCnt%C3%BCs%C3%BC.png"), "Ekran görüntüsü.png");
        assert_eq!(percent_decode("a%2"), "a%2", "a broken escape stays as it is");
        assert_eq!(media_code(MediaError::TooBig), "too_big|20|");
    }
}
