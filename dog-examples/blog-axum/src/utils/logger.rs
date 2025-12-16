pub fn info(msg: &str) {
    eprintln!("[relay] {msg}");
}

pub fn warn(msg: &str) {
    eprintln!("[relay][warn] {msg}");
}

pub fn error(msg: &str) {
    eprintln!("[relay][error] {msg}");
}
