use anyhow::Result;

fn main() -> Result<()> {
    // libwild does that right now but probably should not
    // tracing_subscriber::fmt::init();
    if std::env::var("WILD_PROXY_FALLBACK").is_ok_and(|val| val == "1") {
        return libwild_proxy::fallback::fallback();
    }
    let args = std::env::args();
    libwild_proxy::process(args)
}
