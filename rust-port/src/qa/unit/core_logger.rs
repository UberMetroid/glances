use crate::core::logger;

#[test]
fn init_and_basic_calls_do_not_panic() {
    logger::init(false);
    logger::debug("hidden at info level");
    logger::info("hello");
    logger::warning("warn");
    logger::error("err");
    logger::critical("crit");
}

#[test]
fn debug_enabled_at_debug_level() {
    logger::init(true);
    logger::debug("should appear");
}
