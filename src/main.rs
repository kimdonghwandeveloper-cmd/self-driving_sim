mod app;

fn main() -> bevy::app::AppExit {
    let smoke_test = std::env::args().any(|argument| argument == "--smoke-test");
    app::run(smoke_test)
}
