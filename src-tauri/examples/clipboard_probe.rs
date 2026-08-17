//! Diagnostic probe: mirrors `commands::get_clipboard_files` so the Wayland side
//! can be exercised without launching the whole app.
//!
//! Run with: cargo run --example clipboard_probe

use std::io::Read;

fn main() {
    use wl_clipboard_rs::paste::{self, ClipboardType, MimeType, Seat};

    let mime_types = match paste::get_mime_types_ordered(ClipboardType::Regular, Seat::Unspecified)
    {
        Ok(types) => types,
        Err(error) => {
            println!("get_mime_types_ordered FAILED: {error}");
            return;
        }
    };

    println!("offered mime types ({}):", mime_types.len());
    for mime in &mime_types {
        println!("  {mime}");
    }

    for mime in mime_types.iter().filter(|mime| mime.starts_with("image/")) {
        match paste::get_contents(
            ClipboardType::Regular,
            Seat::Unspecified,
            MimeType::Specific(mime),
        ) {
            Ok((mut reader, actual)) => {
                let mut data = Vec::new();
                let read = reader.read_to_end(&mut data);
                println!(
                    "image branch: {mime} -> negotiated {actual}, {:?} bytes, read={:?}",
                    data.len(),
                    read.map(|_| ())
                );
                if !data.is_empty() {
                    println!("  => would return pasted-image with {} bytes", data.len());
                    return;
                }
            }
            Err(error) => println!("image branch: {mime} -> get_contents FAILED: {error}"),
        }
    }

    let Some(uri_mime) = mime_types.iter().find(|mime| mime.as_str() == "text/uri-list") else {
        println!("no text/uri-list on clipboard => would return EMPTY list");
        return;
    };

    match paste::get_contents(
        ClipboardType::Regular,
        Seat::Unspecified,
        MimeType::Specific(uri_mime),
    ) {
        Ok((mut reader, _)) => {
            let mut uri_data = String::new();
            match reader.read_to_string(&mut uri_data) {
                Ok(_) => println!("text/uri-list payload:\n{uri_data:?}"),
                Err(error) => println!("uri-list read FAILED (not valid UTF-8?): {error}"),
            }
        }
        Err(error) => println!("uri-list get_contents FAILED: {error}"),
    }
}
