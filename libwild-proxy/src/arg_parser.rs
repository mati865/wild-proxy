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

#[derive(Default, PartialEq)]
enum Prefix {
    #[default]
    Long,
    LongAndShort,
    Short,
}

enum Value<'a, 'b> {
    Single(&'b mut Option<&'a str>),
    Multi(&'b mut Vec<&'a str>),
}

struct Arg<'a> {
    args_field: for<'b> fn(&'b mut Args<'a>) -> Value<'a, 'b>,
    separator: Option<char>,
}

struct Flag<'a> {
    args_field: for<'b> fn(&'b mut Args<'a>) -> &'b mut bool,
    supports_negation: bool,
}

impl<'a> ArgParser<'a> {
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
        args_field: for<'b> fn(&'b mut Args<'a>) -> Value<'a, 'b>,
        prefix: Prefix,
        separator: Option<char>,
    ) {
        if prefix == Prefix::Long || prefix == Prefix::LongAndShort {
            self.long_args.insert(
                name,
                Arg {
                    args_field,
                    separator,
                },
            );
        }
        if prefix == Prefix::Short || prefix == Prefix::LongAndShort {
            self.short_args.insert(
                name,
                Arg {
                    args_field,
                    separator,
                },
            );
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

fn setup_parser<'a>() -> ArgParser<'a> {
    let mut parser = ArgParser::default();
    parser.declare_flag("pie", |args| &mut args.pie, Prefix::Short, true);
    parser.declare_flag(
        "shared",
        |args| &mut args.shared,
        Prefix::LongAndShort,
        false,
    );
    parser.declare_flag(
        "static",
        |args| &mut args.static_exe,
        Prefix::LongAndShort,
        false,
    );
    parser.declare_flag(
        "static-pie",
        |args| &mut args.static_pie,
        Prefix::Short,
        false,
    );

    parser.declare_flag(
        "pthread",
        |args| &mut args.pthread,
        Prefix::LongAndShort,
        true,
    );

    parser.declare_arg(
        "sysroot",
        |args| Value::Single(&mut args.sysroot),
        Prefix::Long,
        None,
    );

    parser.declare_arg(
        "T",
        |args| Value::Multi(&mut args.scripts),
        Prefix::Short,
        None,
    );

    parser.declare_arg(
        "Wl",
        |args| Value::Multi(&mut args.linker_args),
        Prefix::Short,
        Some(','),
    );

    parser
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pthread_parsing() {
        let mut parser = setup_parser();
        assert!(!parser.args.pthread);
        parser.parse_flag("--pthread");
        assert!(parser.args.pthread);
        parser.parse_flag("-no-pthread");
        assert!(!parser.args.pthread);
    }

    #[test]
    fn sysroot_parsing() {
        let mut parser = setup_parser();
        assert!(parser.args.sysroot.is_none());
        parser.parse(&["--sysroot=/foo"]);
        assert_eq!(parser.args.sysroot, Some("/foo"));
        parser.parse(&["--sysroot", "/bar"]);
        assert_eq!(parser.args.sysroot, Some("/bar"));
    }

    #[test]
    fn scripts_parsing() {
        let mut parser = setup_parser();
        assert!(parser.args.scripts.is_empty());
        parser.parse(&["-T", "/foo", "-T/bar"]);
        assert_eq!(parser.args.scripts, vec!["/foo", "/bar"]);
    }

    #[test]
    fn wl_parsing() {
        let mut parser = setup_parser();
        parser.parse(&["-Wl,-z,text", "-Wl=-z,now"]);
        assert_eq!(parser.args.linker_args, vec!["-z", "text", "-z", "now"]);
    }
}
