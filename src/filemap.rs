use std::{
    collections::HashMap,
    fs::{self},
    path::Path,
    sync::Arc,
};

use hyper::body::Bytes;

pub enum FileData {
    Bytes(Bytes),
}

pub struct FileMap {
    pub data: FileData,
    pub mime_type: &'static str,
}

impl FileMap {
    fn get_data(path: &str, file: &str) -> Result<FileData, std::io::Error> {
        let path = Path::new(path).join(file);
        let data = fs::read(path)?;

        Ok(FileData::Bytes(data.into()))
    }

    fn get_mime_type(path: &str) -> &'static str {
        let ext = Path::new(path)
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("");

        match ext {
            _ if ext.eq_ignore_ascii_case("html") => "text/html",
            _ if ext.eq_ignore_ascii_case("js") => "application/javascript",
            _ if ext.eq_ignore_ascii_case("css") => "text/css",
            _ if ext.eq_ignore_ascii_case("ico") => "image/x-icon",
            _ => "application/octet-stream", // Default MIME type.
        }
    }

    pub fn static_files() -> Arc<HashMap<String, FileMap>> {
        let path = "web";
        let files = [
            "index.html",
            "favicon.ico",
            "style.css",
            "js/pubsub.js",
            "js/main.js",
            "js/socket.js",
            "js/ui.js",
            "js/alert.js",
        ];

        let mut filemap = HashMap::new();

        for &file in &files {
            let route = format!("/{}", file);
            let data = Self::get_data(path, file).unwrap();
            let mime_type = Self::get_mime_type(file);

            filemap.insert(route, FileMap { data, mime_type });
        }

        Arc::new(filemap)
    }
}
