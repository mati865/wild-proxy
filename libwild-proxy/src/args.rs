pub struct Args {
    pub input: String,
    pub output: String,
}

pub(crate) fn parse<S: AsRef<str>, I: Iterator<Item = S>>(mut input: I) -> Result<Args> {
    let input = input.next().ok_or("Missing input")?.as_ref().to_string();
    let output = input.next().ok_or("Missing output")?.as_ref().to_string();
    Ok(Args { input, output })
}
