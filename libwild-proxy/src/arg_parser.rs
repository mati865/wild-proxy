use anyhow::{Result, bail};
use std::collections::HashMap;

// Conceptually based on Wild's implementation but written from scratch to fit this use case.

struct ArgParser<'a> {
    args: &'a mut Args<'a>,
    unprefixed_args: Vec<&'a str>,
    short_args: HashMap<&'a str, Arg<'a>>,
    long_args: HashMap<&'a str, Arg<'a>>,
    short_flags: HashMap<&'a str, Flag<'a>>,
    long_flags: HashMap<&'a str, Flag<'a>>,
}

#[derive(Default, PartialEq)]
enum Prefix {
    #[default]
    Long,
    LongAndShort,
    None,
    Short,
}

#[derive(Clone, Debug, PartialEq)]
enum Value<'a> {
    Single(Option<&'a str>),
    Multiple(Vec<&'a str>),
}

impl<'a> From<Option<&'a str>> for Value<'a> {
    fn from(value: Option<&'a str>) -> Self {
        Self::Single(value)
    }
}

struct Arg<'a> {
    name: &'a str,
    // value: Value<'a>,
    args_field: for<'b> fn(&'b mut Args<'a>) -> &'b mut Value<'a>,
}

struct Flag<'a> {
    args_field: for<'b> fn(&'b mut Args<'a>) -> &'b mut bool,
    supports_negation: bool,
}

impl<'a> ArgParser<'a> {
    fn new(args: &'a mut Args<'a>) -> Self {
        Self {
            args,
            unprefixed_args: Vec::new(),
            short_args: HashMap::new(),
            long_args: HashMap::new(),
            short_flags: HashMap::new(),
            long_flags: HashMap::new(),
        }
    }

    fn declare_flag(
        &mut self,
        name: &'a str,
        args_field: for<'b> fn(&'b mut Args<'a>) -> &'b mut bool,
        prefix: Prefix,
        supports_negation: bool,
    ) {
        if prefix == Prefix::Long || prefix == Prefix::LongAndShort {
            self.long_flags.insert(
                name,
                Flag {
                    args_field,
                    supports_negation,
                },
            );
        }
        if prefix == Prefix::Short || prefix == Prefix::LongAndShort {
            self.short_flags.insert(
                name,
                Flag {
                    args_field,
                    supports_negation,
                },
            );
        }
    }

    fn declare_arg(
        &mut self,
        name: &'a str,
        args_field: for<'b> fn(&'b mut Args<'a>) -> &'b mut Value<'a>,
        prefix: Prefix,
    ) {
        if prefix == Prefix::Long || prefix == Prefix::LongAndShort {
            self.long_args.insert(name, Arg { name, args_field });
        }
        if prefix == Prefix::Short || prefix == Prefix::LongAndShort {
            self.short_args.insert(name, Arg { name, args_field });
        }
    }

    fn parse_flag(&mut self, arg: &str) -> bool {
        let (stripped, is_long) = if let Some(s) = arg.strip_prefix("--") {
            (s, true)
        } else if let Some(s) = arg.strip_prefix("-") {
            (s, false)
        } else {
            return false;
        };

        let (flag_name, value) = if let Some(negated) = stripped.strip_prefix("no-") {
            (negated, false)
        } else {
            (stripped, true)
        };

        let flag_map = if is_long {
            &self.long_flags
        } else {
            &self.short_flags
        };

        if let Some(flag) = flag_map.get(flag_name) {
            if value || flag.supports_negation {
                *(flag.args_field)(self.args) = value;
                return true;
            }
        }

        false
    }

    fn parse_arg(&mut self, arg: &str) -> Result<()> {
        let (stripped, is_long) = if let Some(s) = arg.strip_prefix("--") {
            (s, true)
        } else if let Some(s) = arg.strip_prefix("-") {
            (s, false)
        } else {
            bail!("Invalid argument: {}", arg);
        };

        let arg_map = if is_long {
            &self.long_args
        } else {
            &self.short_args
        };

        if let Some(arg) = arg_map.get(stripped) {
            let value = (arg.args_field)(self.args);
            match value {
                Value::Single(v) => {
                    v.replace(arg.name);
                }
                Value::Multiple(v) => {}
            }
        }

        Ok(())
    }
}

// TODO: move
#[derive(Debug)]
struct Args<'a> {
    pthread: bool,
    sysroot: Option<&'a str>,
}

impl Default for Args<'_> {
    fn default() -> Self {
        Self {
            pthread: false,
            sysroot: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_parsing() {
        let mut args = Args::default();
        let mut parser = ArgParser::new(&mut args);
        parser.declare_flag(
            "pthread",
            |args| &mut args.pthread,
            Prefix::LongAndShort,
            true,
        );
        assert!(!parser.args.pthread);
        parser.parse_flag("--pthread");
        assert!(parser.args.pthread);
        parser.parse_flag("-no-pthread");
        assert!(!parser.args.pthread);
    }

    #[test]
    fn arg_parsing() {
        let mut args = Args::default();
        let mut parser = ArgParser::new(&mut args);
        parser.declare_arg(
            "sysroot",
            |args| &mut args.sysroot.into(),
            Prefix::LongAndShort,
        );
        assert!(parser.args.sysroot.is_none());
        parser.parse_flag("--sysroot=/path/to/sysroot");
        assert_eq!(parser.args.sysroot, Some("/path/to/sysroot"));
    }
}
