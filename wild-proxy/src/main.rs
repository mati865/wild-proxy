use anyhow::Result;

fn main() -> Result<()> {
    // libwild does that right now but probably should not
    // tracing_subscriber::fmt::init();
    libwild_proxy::fallback::fallback()
}
