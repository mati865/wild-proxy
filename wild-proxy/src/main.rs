use anyhow::{Context, Result};

fn main() -> Result<()> {
    // libwild does that right now but probably should not
    // tracing_subscriber::fmt::init();
    if std::env::var("WILD_PROXY_FALLBACK").is_ok_and(|val| val == "1") {
        return libwild_proxy::fallback::fallback();
    }
    let mut args = std::env::args();
    let binary_name = env!("CARGO_BIN_NAME");
    let zero_position_arg = args
        .next()
        .context("Could not obtain binary name from args")?;
    // TODO: Avoid allocs
    let args = args.collect::<Vec<String>>();
    let args = args.iter().map(|s| s.as_str()).collect::<Vec<&str>>();
    libwild_proxy::process(&args, &zero_position_arg, binary_name)
}
