use std::{fs::OpenOptions, io::Write};

fn main() -> Result<(), std::io::Error> {
    let content = "sdfasdfasdf".to_string();
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open("./wtf.toml")
        .unwrap()
        .write_all(&content.into_bytes())
}
