/// The wall clock in Unix milliseconds; zero if the clock reads before 1970.
///
/// Not built for wasm, where `SystemTime` panics: the browser reads its own clock.
#[cfg(not(target_arch = "wasm32"))]
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}
