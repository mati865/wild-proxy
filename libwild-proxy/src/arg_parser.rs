use anyhow::{Context, Result, bail};
use std::collections::HashMap;

// Conceptually based on Wild's implementation but written from scratch to fit this use case.

#[derive(Default)]
struct ArgParser<'a> {
    args: Args<'a>,
    unprefixed_args: Vec<&'a str>,
    short_args: HashMap<&'a str, Arg<'a>>,
    long_args: HashMap<&'a str, Arg<'a>>,
    short_flags: HashMap<&'a str, Flag<'a>>,
    long_flags: HashMap<&'a str, Flag<'a>>,
}

enum Value<'a, 'b> {
    Single(&'b mut Option<&'a str>),
    Multi(&'b mut Vec<&'a str>),
}

#[derive(Copy, Clone)]
struct Arg<'a> {
    args_field: for<'b> fn(&'b mut Args<'a>) -> Value<'a, 'b>,
    separator: Option<char>,
}

#[derive(Copy, Clone)]
struct Flag<'a> {
    args_field: for<'b> fn(&'b mut Args<'a>) -> &'b mut bool,
    supports_negation: bool,
}

struct FlagBuilder<'a, 'p> {
    parser: &'p mut ArgParser<'a>,
    long_name: Option<&'a str>,
    short_name: Option<&'a str>,
    supports_negation: bool,
    args_field: Option<for<'b> fn(&'b mut Args<'a>) -> &'b mut bool>,
}

impl<'a, 'p> FlagBuilder<'a, 'p> {
    #[must_use]
    fn short(mut self, name: &'a str) -> Self {
        self.short_name = Some(name);
        self
    }

    #[must_use]
    fn long(mut self, name: &'a str) -> Self {
        self.long_name = Some(name);
        self
    }

    #[must_use]
    fn with_negation(mut self, supports_negation: bool) -> Self {
        self.supports_negation = supports_negation;
        self
    }

    #[must_use]
    fn bind(mut self, args_field: for<'b> fn(&'b mut Args<'a>) -> &'b mut bool) -> Self {
        self.args_field = Some(args_field);
        self
    }

    fn build(self) -> Result<()> {
        let args_field = self
            .args_field
            .context("A field must be bound to the flag using bind()")?;

        if self.long_name.is_none() && self.short_name.is_none() {
            bail!("Flag name is missing");
        }

        let flag = Flag {
            args_field,
            supports_negation: self.supports_negation,
        };

        if let Some(long_name) = self.long_name {
            self.parser.long_flags.insert(long_name, flag);
        }
        if let Some(short_name) = self.short_name {
            self.parser.short_flags.insert(short_name, flag);
        }

        Ok(())
    }
}

struct ArgBuilder<'a, 'p> {
    parser: &'p mut ArgParser<'a>,
    long_name: Option<&'a str>,
    short_name: Option<&'a str>,
    separator: Option<char>,
    args_field: Option<for<'b> fn(&'b mut Args<'a>) -> Value<'a, 'b>>,
}

impl<'a, 'p> ArgBuilder<'a, 'p> {
    #[must_use]
    fn short(mut self, name: &'a str) -> Self {
        self.short_name = Some(name);
        self
    }

    #[must_use]
    fn long(mut self, name: &'a str) -> Self {
        self.long_name = Some(name);
        self
    }

    #[must_use]
    fn with_separator(mut self, separator: char) -> Self {
        self.separator = Some(separator);
        self
    }

    #[must_use]
    fn bind(mut self, args_field: for<'b> fn(&'b mut Args<'a>) -> Value<'a, 'b>) -> Self {
        self.args_field = Some(args_field);
        self
    }

    fn build(self) -> Result<()> {
        let args_field = self
            .args_field
            .context("A field must be bound to the argument using bind()")?;

        if self.long_name.is_none() && self.short_name.is_none() {
            bail!("Argument name is missing");
        }

        let arg = Arg {
            args_field,
            separator: self.separator,
        };

        if let Some(long_name) = self.long_name {
            self.parser.long_args.insert(long_name, arg);
        }
        if let Some(short_name) = self.short_name {
            self.parser.short_args.insert(short_name, arg);
        }

        Ok(())
    }
}

impl<'a> ArgParser<'a> {
    fn declare_flag(&mut self) -> FlagBuilder<'a, '_> {
        FlagBuilder {
            parser: self,
            long_name: None,
            short_name: None,
            supports_negation: false,
            args_field: None,
        }
    }

    fn declare_arg(&mut self) -> ArgBuilder<'a, '_> {
        ArgBuilder {
            parser: self,
            long_name: None,
            short_name: None,
            separator: None,
            args_field: None,
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
                *(flag.args_field)(&mut self.args) = value;
                return true;
            }
        }

        false
    }

    fn parse_arg(&mut self, arg: &'a str, args_iter: &mut impl Iterator<Item = &'a str>) -> bool {
        let (stripped, is_long) = if let Some(s) = arg.strip_prefix("--") {
            (s, true)
        } else if let Some(s) = arg.strip_prefix("-") {
            (s, false)
        } else {
            return false;
        };

        let arg_map = if is_long {
            &self.long_args
        } else {
            &self.short_args
        };

        let arg_value_pair = if let Some((key, val)) = stripped.split_once('=') {
            arg_map.get(key).map(|arg| (arg, val))
        } else {
            let arg = arg_map.get(stripped);
            if let Some(arg) = arg {
                Some((arg, args_iter.next().unwrap()))
            } else if !is_long {
                arg_map
                    .keys()
                    .find(|&&key| stripped.starts_with(key))
                    .map(|&key| (&arg_map[key], stripped.strip_prefix(key).unwrap()))
            } else {
                todo!()
            }
        };

        if let Some((arg, value)) = arg_value_pair {
            match (arg.args_field)(&mut self.args) {
                Value::Single(single_value) => {
                    single_value.replace(value);
                }
                Value::Multi(multi_value) => {
                    if let Some(separator) = arg.separator {
                        multi_value.extend(value.split(separator).filter(|s| !s.is_empty()))
                    } else {
                        multi_value.push(value);
                    }
                }
            }
            return true;
        }

        false
    }

    fn parse(&mut self, args: &[&'a str]) {
        let mut args_iter = args.into_iter().copied();
        while let Some(arg) = args_iter.next() {
            if !self.parse_flag(arg) && !self.parse_arg(arg, &mut args_iter) {
                // Neither a flag nor an argument, so it's unprefixed.
                // TODO: Handle it
                self.unprefixed_args.push(arg);
            }
        }
    }
}

// TODO: move
#[derive(Debug)]
struct Args<'a> {
    pthread: bool,
    sysroot: Option<&'a str>,
    scripts: Vec<&'a str>,
    pie: bool,
    shared: bool,
    static_exe: bool,
    static_pie: bool,
    linker_args: Vec<&'a str>,
}

impl Default for Args<'_> {
    fn default() -> Self {
        Self {
            pthread: false,
            sysroot: None,
            scripts: Vec::new(),
            pie: true,
            shared: false,
            static_exe: false,
            static_pie: false,
            linker_args: vec![],
        }
    }
}

fn setup_parser<'a>() -> Result<ArgParser<'a>> {
    let mut parser = ArgParser::default();

    parser
        .declare_flag()
        .short("pie")
        .with_negation(true)
        .bind(|args| &mut args.pie)
        .build()?;

    parser
        .declare_flag()
        .long("shared")
        .short("shared")
        .bind(|args| &mut args.shared)
        .build()?;

    parser
        .declare_flag()
        .long("static")
        .short("static")
        .bind(|args| &mut args.static_exe)
        .build()?;

    parser
        .declare_flag()
        .short("static-pie")
        .bind(|args| &mut args.static_pie)
        .build()?;

    parser
        .declare_flag()
        .long("pthread")
        .short("pthread")
        .with_negation(true)
        .bind(|args| &mut args.pthread)
        .build()?;

    parser
        .declare_arg()
        .long("sysroot")
        .bind(|args| Value::Single(&mut args.sysroot))
        .build()?;

    parser
        .declare_arg()
        .short("T")
        .bind(|args| Value::Multi(&mut args.scripts))
        .build()?;

    parser
        .declare_arg()
        .short("Wl")
        .with_separator(',')
        .bind(|args| Value::Multi(&mut args.linker_args))
        .build()?;

    Ok(parser)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pthread_parsing() {
        let mut parser = setup_parser().unwrap();
        assert!(!parser.args.pthread);
        parser.parse(&["--pthread"]);
        assert!(parser.args.pthread);
        parser.parse(&["-no-pthread"]);
        assert!(!parser.args.pthread);
    }

    #[test]
    fn sysroot_parsing() {
        let mut parser = setup_parser().unwrap();
        assert!(parser.args.sysroot.is_none());
        parser.parse(&["--sysroot=/foo"]);
        assert_eq!(parser.args.sysroot, Some("/foo"));
        parser.parse(&["--sysroot", "/bar"]);
        assert_eq!(parser.args.sysroot, Some("/bar"));
    }

    #[test]
    fn scripts_parsing() {
        let mut parser = setup_parser().unwrap();
        assert!(parser.args.scripts.is_empty());
        parser.parse(&["-T", "/foo", "-T/bar"]);
        assert_eq!(parser.args.scripts, vec!["/foo", "/bar"]);
    }

    #[test]
    fn wl_parsing() {
        let mut parser = setup_parser().unwrap();
        parser.parse(&["-Wl,-z,text", "-Wl=-z,now"]);
        assert_eq!(parser.args.linker_args, vec!["-z", "text", "-z", "now"]);
    }
}
