//! A `multipart/form-data` request folded into one value, the way the upload routes read busboy's events: a part that names a file
//! is a file, any other a text field, and the last file and the last value of a field win.

use axum::extract::multipart::MultipartRejection;
use axum::extract::Multipart;
use std::collections::HashMap;

pub struct File {
    pub mime: String,
    pub data: Vec<u8>,
}

#[derive(Default)]
pub struct Form {
    pub files: Vec<File>,
    pub fields: HashMap<String, String>,
}

impl Form {
    pub fn file(&self) -> Option<&File> {
        self.files.last()
    }

    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields.get(name).map(String::as_str)
    }
}

/// The form, or the message the 400 carries when the request is not a well-formed multipart body.
pub async fn read(form: Result<Multipart, MultipartRejection>) -> Result<Form, String> {
    let mut multipart = form.map_err(|rejection| rejection.body_text())?;
    let mut out = Form::default();
    while let Some(field) = multipart.next_field().await.map_err(|e| e.body_text())? {
        let name = field.name().unwrap_or_default().to_string();
        if field.file_name().is_some() {
            let mime = field.content_type().unwrap_or_default().to_string();
            let data = field.bytes().await.map_err(|e| e.body_text())?.to_vec();
            out.files.push(File { mime, data });
        } else {
            let text = field.text().await.map_err(|e| e.body_text())?;
            out.fields.insert(name, text);
        }
    }
    Ok(out)
}
