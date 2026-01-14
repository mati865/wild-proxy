use crate::args::Args;
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::ops::Not;
// Conceptually based on Wild's implementation but written from scratch to fit this use case.

#[derive(Default)]
pub(crate) struct ArgParser {
    pub(crate) args: Args,
    short_args: HashMap<&'static str, Arg>,
    long_args: HashMap<&'static str, Arg>,
    short_flags: HashMap<&'static str, Flag>,
    long_flags: HashMap<&'static str, Flag>,
    pub(crate) unknown_args: Vec<String>,
}

pub(crate) enum FlagValue<'b> {
    Single(&'b mut bool),
    Multi(&'b mut Vec<String>),
}

pub(crate) enum ArgValue<'b> {
    Single(&'b mut Option<String>),
    Multi(&'b mut Vec<String>),
}

#[derive(Copy, Clone)]
struct Arg {
    args_field: for<'b> fn(&'b mut Args) -> ArgValue<'b>,
    separator: Option<char>,
    raw: bool,
}

#[derive(Copy, Clone)]
struct Flag {
    args_field: for<'b> fn(&'b mut Args) -> FlagValue<'b>,
    supports_negation: bool,
    unstripped: bool,
}

pub(crate) struct FlagBuilder<'p> {
    parser: &'p mut ArgParser,
    long_name: Option<&'static str>,
    short_name: Option<&'static str>,
    supports_negation: bool,
    args_field: Option<for<'b> fn(&'b mut Args) -> FlagValue<'b>>,
    unstripped: bool,
}

impl<'p> FlagBuilder<'p> {
    #[must_use]
    pub(crate) fn short(mut self, name: &'static str) -> Self {
        self.short_name = Some(name);
        self
    }

    #[must_use]
    pub(crate) fn long(mut self, name: &'static str) -> Self {
        self.long_name = Some(name);
        self
    }

    #[must_use]
    pub(crate) fn with_negation(mut self, supports_negation: bool) -> Self {
        self.supports_negation = supports_negation;
        self
    }

    #[must_use]
    pub(crate) fn bind(mut self, args_field: for<'b> fn(&'b mut Args) -> FlagValue<'b>) -> Self {
        self.args_field = Some(args_field);
        self
    }

    pub(crate) fn raw(mut self) -> Self {
        self.unstripped = true;
        self
    }

    pub(crate) fn build(self) -> Result<()> {
        let args_field = self
            .args_field
            .context("A field must be bound to the flag using bind()")?;

        if self.long_name.is_none() && self.short_name.is_none() {
            bail!("Flag name is missing");
        }

        if matches!(args_field(&mut self.parser.args), FlagValue::Single(_)) {
            if self.unstripped {
                bail!("Cannot use raw with single-value flag")
            }
        } else {
            if self.supports_negation {
                bail!("Cannot use negation with multi-value flag")
            }
        }

        let flag = Flag {
            args_field,
            supports_negation: self.supports_negation,
            unstripped: self.unstripped,
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

pub(crate) struct ArgBuilder<'p> {
    parser: &'p mut ArgParser,
    long_name: Option<&'static str>,
    short_name: Option<&'static str>,
    separator: Option<char>,
    args_field: Option<for<'b> fn(&'b mut Args) -> ArgValue<'b>>,
    unstripped: bool,
}

impl<'p> ArgBuilder<'p> {
    #[must_use]
    pub(crate) fn short(mut self, name: &'static str) -> Self {
        self.short_name = Some(name);
        self
    }

    #[must_use]
    pub(crate) fn long(mut self, name: &'static str) -> Self {
        self.long_name = Some(name);
        self
    }

    #[must_use]
    pub(crate) fn with_separator(mut self, separator: char) -> Self {
        self.separator = Some(separator);
        self
    }

    #[must_use]
    pub(crate) fn bind(mut self, args_field: for<'b> fn(&'b mut Args) -> ArgValue<'b>) -> Self {
        self.args_field = Some(args_field);
        self
    }

    pub(crate) fn raw(mut self) -> Self {
        self.unstripped = true;
        self
    }

    pub(crate) fn build(self) -> Result<()> {
        let args_field = self
            .args_field
            .context("A field must be bound to the argument using bind()")?;

        if self.long_name.is_none() && self.short_name.is_none() {
            bail!("Argument name is missing");
        }

        let arg = Arg {
            args_field,
            separator: self.separator,
            raw: self.unstripped,
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

impl ArgParser {
    pub(crate) fn declare_flag(&mut self) -> FlagBuilder<'_> {
        FlagBuilder {
            parser: self,
            long_name: None,
            short_name: None,
            supports_negation: false,
            args_field: None,
            unstripped: false,
        }
    }

    pub(crate) fn declare_arg(&mut self) -> ArgBuilder<'_> {
        ArgBuilder {
            parser: self,
            long_name: None,
            short_name: None,
            separator: None,
            args_field: None,
            unstripped: false,
        }
    }

    fn parse_flag(&mut self, raw_arg: &str) -> bool {
        let (stripped, is_long) = if let Some(s) = raw_arg.strip_prefix("--") {
            (s, true)
        } else if let Some(s) = raw_arg.strip_prefix("-") {
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
            match (flag.args_field)(&mut self.args) {
                FlagValue::Single(single_value) => {
                    if value || flag.supports_negation {
                        *single_value = value;
                        return true;
                    }
                }
                FlagValue::Multi(multi_value) => {
                    if flag.unstripped {
                        multi_value.push(raw_arg.to_string());
                    } else {
                        multi_value.push(flag_name.to_string());
                    }
                    return true;
                }
            }
        }

        false
    }

    fn parse_arg<'a>(
        &mut self,
        raw_arg: &str,
        args_iter: &mut impl Iterator<Item = &'a str>,
    ) -> bool {
        let (stripped, is_long) = if let Some(s) = raw_arg.strip_prefix("--") {
            (s, true)
        } else if let Some(s) = raw_arg.strip_prefix("-") {
            (s, false)
        } else {
            return false;
        };

        let arg_map = if is_long {
            &self.long_args
        } else {
            &self.short_args
        };

        let mut next_arg = None;
        let arg_value_pair = stripped
            .find(&[',', '='])
            .and_then(|pos| {
                let (key, rest) = stripped.split_at(pos);
                let val = &rest[1..];
                arg_map.get(key).map(|arg| (arg, val))
            })
            .or_else(|| {
                let arg = arg_map.get(stripped);
                if let Some(arg) = arg {
                    next_arg = args_iter.next();
                    Some((arg, next_arg.unwrap()))
                } else if !is_long {
                    arg_map
                        .iter()
                        .find_map(|(&key, arg)| stripped.strip_prefix(key).map(|val| (arg, val)))
                } else {
                    None
                }
            });

        if let Some((arg, value)) = arg_value_pair {
            match (arg.args_field)(&mut self.args) {
                ArgValue::Single(single_value) => {
                    if arg.raw {
                        if next_arg.is_some() {
                            panic!("Unstripped argument cannot be created from two arguments");
                        } else {
                            single_value.replace(raw_arg.to_string());
                        }
                    } else {
                        single_value.replace(value.to_string());
                    }
                }
                ArgValue::Multi(multi_value) => {
                    if arg.raw {
                        if let Some(next_arg) = next_arg {
                            multi_value.extend([raw_arg.to_string(), next_arg.to_string()]);
                        } else {
                            multi_value.push(raw_arg.to_string());
                        }
                    } else if let Some(separator) = arg.separator {
                        multi_value.extend(
                            value
                                .split(separator)
                                .filter_map(|s| s.is_empty().not().then(|| s.to_string())),
                        )
                    } else {
                        multi_value.push(value.to_string());
                    }
                }
            }
            return true;
        }

        false
    }

    fn handle_unknown_arg(&mut self, arg: &str) -> bool {
        if arg.starts_with('-') {
            self.unknown_args.push(arg.to_string());
            return true;
        }
        false
    }

    pub(crate) fn parse(&mut self, args: &[&str]) {
        let mut args_iter = args.into_iter().copied();
        while let Some(arg) = args_iter.next() {
            if !self.parse_flag(arg)
                && !self.parse_arg(arg, &mut args_iter)
                && !self.handle_unknown_arg(arg)
            {
                // Neither a flag nor an argument, so it's an object or source file.
                if [".c", ".cc", ".cpp", ".s", ".S"]
                    .iter()
                    .any(|ext| arg.ends_with(ext))
                {
                    self.args.sources.push(arg.to_string());
                } else {
                    self.args.input_objects_found = true;
                    self.args.raw_args.push(arg.to_string());
                }
            }
        }
    }
}
