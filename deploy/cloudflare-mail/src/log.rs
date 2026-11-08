//! Worker logs. Only fixed text and counts are accepted, so tokens, addresses
//! and message content can never be written to a log.

#[cfg(target_arch = "wasm32")]
pub fn error(message: &'static str) {
    worker::console_error!("{message}");
}

#[cfg(target_arch = "wasm32")]
pub fn count(message: &'static str, count: usize) {
    worker::console_log!("{message} {count}");
}

#[cfg(not(target_arch = "wasm32"))]
thread_local! {
    pub static LINES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(not(target_arch = "wasm32"))]
pub fn error(message: &'static str) {
    LINES.with(|lines| lines.borrow_mut().push(message.to_string()));
}

#[cfg(not(target_arch = "wasm32"))]
pub fn count(message: &'static str, count: usize) {
    LINES.with(|lines| lines.borrow_mut().push(format!("{message} {count}")));
}
