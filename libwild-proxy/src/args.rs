use crate::arch::Arch;
use crate::arg_parser::{ArgParser, ArgValue, FlagValue};
use anyhow::{Result, bail};

// use crate::arch::{Arch, target_arch};
// use anyhow::{Context, Result};
//
// pub type LinkerArgs = Vec<String>;
// pub type CompilerArgs = Vec<String>;
//
#[derive(Debug, PartialEq, Default)]
pub enum Mode {
    #[default]
    None,
    CompileOnly,
    LinkOnly,
    CompileAndLink,
}

#[derive(Debug, Default, PartialEq)]
pub enum OutputKind {
    #[default]
    DynamicPie,
    StaticPie,
    Dynamic,
    Static,
    SharedObject,
}

impl OutputKind {
    // Drivers follow a pretty weird priority-based scheme, in order of option importance:
    // -static-pie
    // -static
    // -shared
    // -(no-)pie
    // The options don't override each other, so for example, `-static -shared` is the same as just
    // using `-static`.
    fn from_args(args: &Args) -> Self {
        if args.static_pie {
            OutputKind::StaticPie
        } else if args.static_exe {
            OutputKind::Static
        } else if args.shared {
            OutputKind::SharedObject
        } else if args.pie {
            OutputKind::DynamicPie
        } else {
            OutputKind::Dynamic
        }
    }
}

// /// Arguments for both
// #[derive(Debug, PartialEq)]
// #[non_exhaustive]
// pub struct DriverArgs {
//     pub(crate) output: String,
//     pub(crate) objects_and_libs: Vec<String>,
//     pub(crate) output_kind: OutputKind,
//     pub(crate) default_libs: bool,
//     pub(crate) profile: bool,
//     pub(crate) coverage: bool,
//     pub(crate) arch: Arch,
//     pub(crate) sysroot: Option<String>,
//     pub(crate) pthread: bool,
// }
//
// impl Default for DriverArgs {
//     fn default() -> Self {
//         Self {
//             output: "a.out".to_string(),
//             objects_and_libs: vec![],
//             output_kind: Default::default(),
//             default_libs: true,
//             profile: false,
//             coverage: false,
//             arch: Arch::default(),
//             sysroot: None,
//             pthread: false,
//         }
//     }
// }
//
// // TODO: optimise
// pub(crate) fn parse(args: &[String], target: Option<Arch>) -> Result<Mode> {
//     // Exit early if not linking at all.
//     if args.iter().map(AsRef::as_ref).any(is_compile_only_arg) {
//         return Ok(Mode::CompileOnly);
//     }
//
//     let mut args = args.into_iter();
//
//     let mut compiler_args = Vec::with_capacity(args.len());
//     let mut linker_args = Vec::with_capacity(args.len());
//     let mut driver_args = DriverArgs::default();
//     let mut unknown_args = Vec::default();
//
//     if let Some(target) = target {
//         driver_args.arch = target;
//     }
//
//     let mut are_sources_present = false;
//     let mut pie = true;
//     let mut shared = false;
//     let mut static_exe = false;
//     let mut static_pie = false;
//
//     while let Some(arg) = args.next() {
//         // Driver args
//         {
//             if parse_and_append_arg("-T", &arg, &mut args, &mut linker_args) {
//                 continue;
//             }
//             if let Some(args) = arg.strip_prefix("-Wl,") {
//                 linker_args.extend(args.split(',').map(|x| x.into()));
//                 continue;
//             }
//             if let Some(arg) = parse_arg("-Xlinker", &arg, &mut args) {
//                 linker_args.push(arg);
//                 continue;
//             }
//
//             // TODO: This is used by Rust, handle others as well.
//             match arg.as_str() {
//                 "-nodefaultlibs" => {
//                     driver_args.default_libs = false;
//                     continue;
//                 }
//                 _ => {}
//             }
//
//             match arg.as_str() {
//                 "-pie" | "--pie" => {
//                     pie = true;
//                     continue;
//                 }
//                 "-no-pie" | "--no-pie" => {
//                     pie = false;
//                     continue;
//                 }
//                 "-static" | "--static" => {
//                     static_exe = true;
//                     continue;
//                 }
//                 "-static-pie" | "--static-pie" => {
//                     static_pie = true;
//                     continue;
//                 }
//                 "-shared" | "--shared" => {
//                     shared = true;
//                     continue;
//                 }
//                 _ => (),
//             }
//         }
//
//         // Args that the compiler and linker driver need to handle.
//         {
//             if let Some(output) = parse_arg("-o", &arg, &mut args) {
//                 driver_args.output = output.to_string();
//                 compiler_args.push(arg.to_string());
//                 continue;
//             } else if arg == "-pthread" || arg == "--pthread" {
//                 driver_args.pthread = true;
//                 compiler_args.push(arg.to_string());
//                 continue;
//             } else if arg == "-no-pthread" || arg == "--no-pthread" {
//                 driver_args.pthread = false;
//                 compiler_args.push(arg.to_string());
//                 continue;
//             } else if arg == "-pg" || arg == "--profile" {
//                 driver_args.profile = true;
//                 compiler_args.push(arg.to_string());
//                 continue;
//             } else if arg == "-coverage" || arg == "--coverage" {
//                 driver_args.coverage = true;
//                 compiler_args.push(arg.to_string());
//                 continue;
//             } else if let Some(target) = arg.strip_prefix("--target=") {
//                 driver_args.arch = target_arch(target)?;
//                 compiler_args.push(arg.to_string());
//                 continue;
//             } else if arg == "-target" {
//                 let target = args
//                     .next()
//                     .with_context(|| format!("Unexpected end of arguments after: {arg}"))?;
//                 driver_args.arch = target_arch(target)?;
//                 compiler_args.extend([arg.to_string(), target.to_string()]);
//                 continue;
//             }
//
//             if let Some(path) = parse_arg("--sysroot", &arg, &mut args) {
//                 let sysroot_arg = format!("--sysroot={}", path);
//                 compiler_args.push(sysroot_arg.clone());
//                 linker_args.push(sysroot_arg);
//                 driver_args.sysroot = Some(path);
//                 continue;
//             }
//         }
//
//         // Compilers seem to care only about supported extension or `-x` argument when determining
//         // whether a file is source code.
//         {
//             if parse_and_append_arg("-x", &arg, &mut args, &mut compiler_args) {
//                 are_sources_present = true;
//                 continue;
//             } else if arg.ends_with(".c")
//                 || arg.ends_with(".cc")
//                 || arg.ends_with(".cpp")
//                 || arg.ends_with(".s")
//                 || arg.ends_with(".S")
//             {
//                 compiler_args.push(arg.to_string());
//                 are_sources_present = true;
//                 continue;
//             }
//         }
//
//         {
//             let found = ["-l", "-L"].iter().any(|&linker_arg| {
//                 parse_and_append_arg(
//                     linker_arg,
//                     &arg,
//                     &mut args,
//                     &mut driver_args.objects_and_libs,
//                 )
//             });
//             if found {
//                 continue;
//             }
//             if !arg.starts_with('-') {
//                 driver_args.objects_and_libs.push(arg.to_string());
//                 continue;
//             }
//         }
//
//         if arg == "-pedantic"
//             || arg == "--pedantic"
//             || arg.starts_with("-f")
//             || arg.starts_with("-m")
//             || arg.starts_with("-W")
//             || arg.starts_with("-O")
//             || arg.starts_with("-D")
//         {
//             compiler_args.push(arg.to_string());
//         } else {
//             unknown_args.push(arg);
//         }
//     }
//
//     driver_args.output_kind = OutputKind::new(static_pie, static_exe, shared, pie);
//
//     let mode = if are_sources_present {
//         Mode::CompileAndLink((compiler_args, linker_args, driver_args))
//     } else if driver_args.objects_and_libs.is_empty() {
//         Mode::CompileOnly
//     } else {
//         if !unknown_args.is_empty() {
//             eprintln!("Unhandled arguments: {:?}", &unknown_args);
//         }
//         Mode::LinkOnly(linker_args, driver_args)
//     };
//     Ok(mode)
// }
//
// pub(crate) fn is_compile_only_arg(arg: &str) -> bool {
//     // TODO: handle some of them
//     ["--help", "--version", "-###", "-c", "-S"].contains(&arg)
//         || arg.starts_with("-dump")
//         || arg.starts_with("-print")
// }
//
// // TODO: find a better way
// // true means the argument matched
// fn parse_and_append_arg<'a>(
//     desired_arg: &str,
//     actual_arg: &str,
//     additional_arg: &mut impl Iterator<Item = &'a String>,
//     vec: &mut Vec<String>,
// ) -> bool {
//     match actual_arg.strip_prefix(desired_arg) {
//         Some("") => {
//             let rest = additional_arg
//                 .next()
//                 .unwrap_or_else(|| panic!("missing arg to {desired_arg}"));
//             vec.extend_from_slice(&[desired_arg.to_string(), rest.to_string()]);
//         }
//         Some(_) => {
//             vec.push(actual_arg.to_string());
//         }
//         None => return false,
//     }
//     true
// }
//
// // TODO: find a better way
// fn parse_arg<'a>(
//     desired_arg: &str,
//     actual_arg: &str,
//     additional_arg: &mut impl Iterator<Item = &'a String>,
// ) -> Option<String> {
//     match actual_arg.strip_prefix(desired_arg) {
//         Some("") => Some(
//             additional_arg
//                 .next()
//                 .unwrap_or_else(|| panic!("missing arg to {desired_arg}"))
//                 .to_string(),
//         ),
//         Some(rest) => Some(rest.trim_start_matches("=").to_string()),
//         None => None,
//     }
// }
//
// #[cfg(test)]
// mod tests {
//     use super::*;
//
//     #[test]
//     fn no_args() {
//         let args = vec![];
//         assert!(matches!(parse(&args, None), Ok(Mode::CompileOnly)));
//     }
//
//     #[test]
//     fn compile_only_mode() {
//         let args = vec!["foo.c".to_string(), "-c".to_string()];
//         assert_eq!(Mode::CompileOnly, parse(&args, None).unwrap());
//     }
//
//     #[test]
//     fn compile_and_link_mode_with_sources() {
//         let args = vec!["foo.c".to_string()];
//         assert!(matches!(parse(&args, None), Ok(Mode::CompileAndLink(_))));
//     }
//
//     #[test]
//     fn compile_and_link_mode_with_sources_and_objects() {
//         let args = vec!["foo.c".to_string(), "bar.o".to_string()];
//         assert!(matches!(parse(&args, None), Ok(Mode::CompileAndLink(_))));
//     }
//
//     #[test]
//     fn link_mode() {
//         let args = vec!["foo.o".to_string()];
//         assert!(matches!(parse(&args, None), Ok(Mode::LinkOnly(_, _))));
//     }
//
//     #[test]
//     fn default_output_kind() {
//         let args = vec!["foo.o".to_string()];
//         let Mode::LinkOnly(_, driver_args) = parse(&args, None).unwrap() else {
//             panic!()
//         };
//         assert_eq!(driver_args.output_kind, OutputKind::DynamicPie);
//     }
//
//     #[test]
//     fn linker_args() {
//         let args = [
//             "-L",
//             "foo",
//             "-lfoo",
//             "bar.o",
//             "-Lbaz",
//             "-lbaz",
//             "-T",
//             "/foo",
//             "-T/bar",
//             "-Wl,-v,-z,now",
//             "-Xlinker",
//             "--build-id",
//         ]
//         .iter()
//         .map(ToString::to_string)
//         .collect::<Vec<_>>();
//         let Mode::LinkOnly(linker_args, driver_args) = parse(&args, None).unwrap() else {
//             panic!("Wrong driver mode")
//         };
//         assert_eq!(
//             linker_args,
//             ["-T", "/foo", "-T/bar", "-v", "-z", "now", "--build-id"]
//         );
//         assert_eq!(
//             driver_args.objects_and_libs,
//             ["-L", "foo", "-lfoo", "bar.o", "-Lbaz", "-lbaz"]
//         );
//     }
//
//     #[test]
//     fn pthread_arg() {
//         let mut args = vec!["foo.o".to_string()];
//         assert!(matches!(
//             parse(&args, None),
//             Ok(Mode::LinkOnly(_, DriverArgs { pthread: false, .. }))
//         ));
//         args.push("-no-pthread".to_string());
//         assert!(matches!(
//             parse(&args, None),
//             Ok(Mode::LinkOnly(_, DriverArgs { pthread: false, .. }))
//         ));
//         args.push("-pthread".to_string());
//         assert!(matches!(
//             parse(&args, None),
//             Ok(Mode::LinkOnly(_, DriverArgs { pthread: true, .. }))
//         ));
//         args.push("--no-pthread".to_string());
//         assert!(matches!(
//             parse(&args, None),
//             Ok(Mode::LinkOnly(_, DriverArgs { pthread: false, .. }))
//         ));
//         args.push("--pthread".to_string());
//         assert!(matches!(
//             parse(&args, None),
//             Ok(Mode::LinkOnly(_, DriverArgs { pthread: true, .. }))
//         ));
//     }
//
//     #[test]
//     fn target_arg() {
//         let mut args = vec![
//             "foo.o".to_string(),
//             "-target".to_string(),
//             "x86_64-linux-gnu".to_string(),
//         ];
//         assert!(matches!(
//             parse(&args, None),
//             Ok(Mode::LinkOnly(
//                 _,
//                 DriverArgs {
//                     arch: Arch::X86_64,
//                     ..
//                 }
//             ))
//         ));
//         args.push("--target=aarch64-pc-linux-gnu".to_string());
//         assert!(matches!(
//             parse(&args, None),
//             Ok(Mode::LinkOnly(
//                 _,
//                 DriverArgs {
//                     arch: Arch::Aarch64,
//                     ..
//                 }
//             ))
//         ));
//     }
//
//     #[test]
//     fn coverage_arg() {
//         let mut args = vec!["foo.o".to_string()];
//         assert!(matches!(
//             parse(&args, None),
//             Ok(Mode::LinkOnly(
//                 _,
//                 DriverArgs {
//                     coverage: false,
//                     ..
//                 }
//             ))
//         ));
//         args.push("--coverage".to_string());
//         assert!(matches!(
//             parse(&args, None),
//             Ok(Mode::LinkOnly(_, DriverArgs { coverage: true, .. }))
//         ));
//     }
//
//     #[test]
//     fn sysroot_arg() {
//         let mut args = vec!["foo.o".to_string(), "--sysroot=/foo".to_string()];
//         assert!(matches!(
//             parse(&args, None),
//             Ok(Mode::LinkOnly(
//                 _,
//                 DriverArgs {
//                     sysroot: Some(ref s),
//                     ..
//                 }
//             )) if s == "/foo"
//         ));
//         args.extend(["--sysroot".to_string(), "/bar".to_string()]);
//         assert!(matches!(
//             parse(&args, None),
//             Ok(Mode::LinkOnly(
//                 _,
//                 DriverArgs {
//                     sysroot: Some(ref s),
//                     ..
//                 }
//             )) if s == "/bar"
//         ))
//     }
//
//     #[test]
//     fn nodefaultlibs_arg() {
//         let args = vec!["foo.o".to_string(), "-nodefaultlibs".to_string()];
//         assert!(matches!(
//             parse(&args, None),
//             Ok(Mode::LinkOnly(
//                 _,
//                 DriverArgs {
//                     default_libs: false,
//                     ..
//                 }
//             ))
//         ));
//     }
//
//     #[test]
//     fn profile_arg() {
//         let args = vec![
//             "foo.o".to_string(),
//             "--profile".to_string(),
//             "-profile".to_string(),
//         ];
//         assert!(matches!(
//             parse(&args, None),
//             Ok(Mode::LinkOnly(_, DriverArgs { profile: true, .. }))
//         ));
//     }
//
//     #[test]
//     fn pg_arg() {
//         let args = vec!["foo.o".to_string(), "-pg".to_string()];
//         assert!(matches!(
//             parse(&args, None),
//             Ok(Mode::LinkOnly(_, DriverArgs { profile: true, .. }))
//         ));
//     }
//
//     #[test]
//     fn unknown_arg() {
//         let args = vec!["foo.o".to_string(), "--unknown".to_string()];
//         assert!(matches!(parse(&args, None), Ok(Mode::LinkOnly(_, _))));
//     }
// }
#[derive(Debug)]
pub(crate) struct Args {
    pub(crate) pthread: bool,
    pub(crate) sysroot: Option<String>,
    pub(crate) scripts: Vec<String>,
    pie: bool,
    shared: bool,
    static_exe: bool,
    static_pie: bool,
    pub(crate) input_objects_found: bool,
    pub(crate) raw_args: Vec<String>,
    pub(crate) additional_search_paths: Vec<String>,
    compiler_args: Vec<String>,
    pub(crate) nodefaultlibs: bool,
    pub(crate) nostartfiles: bool,
    pub(crate) nostdlib: bool,
    pub(crate) coverage: bool,
    pub(crate) profile: bool,
    target: Option<String>,
    language: Option<String>,
    dont_assemble: bool,
    dont_link: bool,
    pub(crate) output: String,
    pub(crate) output_kind: OutputKind,
    pub(crate) mode: Mode,
    pub(crate) arch: Arch,
    pub(crate) sources: Vec<String>,
    pub(crate) help: bool,
    pub(crate) hash_hash_hash: bool,
    pub(crate) verbose: bool,
    pub(crate) version: bool,
    pub(crate) fuse_ld: Option<String>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            pthread: false,
            sysroot: None,
            scripts: Vec::new(),
            pie: true,
            shared: false,
            static_exe: false,
            static_pie: false,
            input_objects_found: false,
            additional_search_paths: Vec::new(),
            raw_args: vec![],
            compiler_args: vec![],
            nodefaultlibs: false,
            nostartfiles: false,
            nostdlib: false,
            coverage: false,
            profile: false,
            target: None,
            language: None,
            dont_assemble: false,
            dont_link: false,
            output: "a.out".to_string(),
            output_kind: Default::default(),
            mode: Default::default(),
            arch: Default::default(),
            sources: vec![],
            help: false,
            hash_hash_hash: false,
            verbose: false,
            version: false,
            fuse_ld: None,
        }
    }
}

impl Args {
    pub(crate) fn parse_args(args: &[&str], target: Option<Arch>) -> Result<Self> {
        let mut parser = setup_parser()?;
        parser.parse(args);
        let mut args = parser.args;

        // Transformations to make the resulting struct more suitable.
        // TODO: Find a better way to do this.
        {
            args.output_kind = OutputKind::from_args(&args);

            if let Some(target) = &args.target {
                args.arch = crate::arch::target_arch(target)?;
            } else if let Some(target) = target {
                args.arch = target;
            }

            if args.dont_assemble || args.dont_link || args.language.is_some() {
                args.mode = Mode::CompileOnly;
            } else if !args.sources.is_empty() {
                args.mode = Mode::CompileAndLink
            } else if args.input_objects_found {
                args.mode = Mode::LinkOnly
            } else if args.version {
                // If no specific mode can be determined, and we are asked to print the version,
                // this is probably a build system that tries to determine the compiler kind.
                args.mode = Mode::CompileOnly
            }
        }

        if std::env::var_os("WILD_PROXY_DENY_UNKNOWN_ARGS").is_some() {
            if !parser.unknown_args.is_empty() {
                bail!("Unhandled args: \"{}\"", parser.unknown_args.join("\" \""));
            }
        }

        Ok(args)
    }
}

fn setup_parser() -> Result<ArgParser> {
    let mut parser = ArgParser::default();

    parser
        .declare_flag()
        .short("pie")
        .with_negation(true)
        .bind(|args| FlagValue::Single(&mut args.pie))
        .build()?;

    parser
        .declare_flag()
        .long("shared")
        .short("shared")
        .bind(|args| FlagValue::Single(&mut args.shared))
        .build()?;

    parser
        .declare_flag()
        .long("static")
        .short("static")
        .bind(|args| FlagValue::Single(&mut args.static_exe))
        .build()?;

    parser
        .declare_flag()
        .short("static-pie")
        .bind(|args| FlagValue::Single(&mut args.static_pie))
        .build()?;

    parser
        .declare_flag()
        .long("pthread")
        .short("pthread")
        .with_negation(true)
        .bind(|args| FlagValue::Single(&mut args.pthread))
        .build()?;

    parser
        .declare_flag()
        .short("nodefaultlibs")
        .bind(|args| FlagValue::Single(&mut args.nodefaultlibs))
        .build()?;

    parser
        .declare_flag()
        .long("nostartfiles")
        .bind(|args| FlagValue::Single(&mut args.nostartfiles))
        .build()?;

    parser
        .declare_flag()
        .long("nostdlib")
        .bind(|args| FlagValue::Single(&mut args.nostdlib))
        .build()?;

    parser
        .declare_flag()
        .long("coverage")
        .short("coverage")
        .bind(|args| FlagValue::Single(&mut args.coverage))
        .build()?;

    parser
        .declare_flag()
        .long("profile")
        .short("pg")
        .bind(|args| FlagValue::Single(&mut args.profile))
        .build()?;

    parser
        .declare_flag()
        .short("c")
        .bind(|arg| FlagValue::Single(&mut arg.dont_link))
        .build()?;

    parser
        .declare_flag()
        .short("S")
        .bind(|arg| FlagValue::Single(&mut arg.dont_assemble))
        .build()?;

    parser
        .declare_flag()
        .long("help")
        .bind(|args| FlagValue::Single(&mut args.help))
        .build()?;

    parser
        .declare_flag()
        .short("###")
        .bind(|args| FlagValue::Single(&mut args.hash_hash_hash))
        .build()?;

    parser
        .declare_flag()
        .short("v")
        .long("verbose")
        .bind(|args| FlagValue::Single(&mut args.verbose))
        .build()?;

    parser
        .declare_flag()
        .long("version")
        .bind(|args| FlagValue::Single(&mut args.version))
        .build()?;

    parser
        .declare_flag()
        .short("E")
        .bind(|args| FlagValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_flag()
        .short("M")
        .bind(|args| FlagValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_flag()
        .long("pipe")
        .short("pipe")
        .bind(|args| FlagValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short("o")
        .prefix("o")
        .bind(|args| ArgValue::Single(&mut args.output))
        .build()?;

    parser
        .declare_arg()
        .short("x")
        .prefix("x")
        .bind(|args| ArgValue::SingleOptional(&mut args.language))
        .build()?;

    parser
        .declare_arg()
        .short("B")
        .prefix("B")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .prefix("m")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .prefix("D")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .prefix("I")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .prefix("W")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .prefix("M")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short("MF")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short("MJ")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short("MQ")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short("MT")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short_or_prefix("O")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short("std")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .prefix("f")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short("pedantic")
        .long("pedantic")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short("idirafter")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short("imacros")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short("imultilib")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short("include")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short("iprefix")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short("iquote")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short("isysroot")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short("isystem")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short("iwithprefix")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short("iwithprefixbefore")
        .bind(|args| ArgValue::Multi(&mut args.compiler_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short_or_prefix("l")
        .bind(|args| ArgValue::Multi(&mut args.raw_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short_or_prefix("L")
        .bind(|args| ArgValue::Multi(&mut args.additional_search_paths))
        .build()?;

    parser
        .declare_arg()
        .short("target")
        .long("target")
        .bind(|args| ArgValue::SingleOptional(&mut args.target))
        .build()?;

    parser
        .declare_arg()
        .long("sysroot")
        .bind(|args| ArgValue::SingleOptional(&mut args.sysroot))
        .build()?;

    parser
        .declare_arg()
        .short_or_prefix("T")
        .bind(|args| ArgValue::Multi(&mut args.scripts))
        .build()?;

    parser
        .declare_arg()
        .short_or_prefix("Wl")
        .with_separator(',')
        .bind(|args| ArgValue::Multi(&mut args.raw_args))
        .build()?;

    parser
        .declare_arg()
        .short("Xlinker")
        .bind(|args| ArgValue::Multi(&mut args.raw_args))
        .build()?;

    parser
        .declare_arg()
        .short_or_prefix("z")
        .bind(|args| ArgValue::Multi(&mut args.raw_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short_or_prefix("u")
        .bind(|args| ArgValue::Multi(&mut args.raw_args))
        .raw()
        .build()?;

    parser
        .declare_arg()
        .short("fuse-ld")
        .bind(|args| ArgValue::SingleOptional(&mut args.fuse_ld))
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
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn sysroot_parsing() {
        let mut parser = setup_parser().unwrap();
        assert!(parser.args.sysroot.is_none());
        parser.parse(&["--sysroot=/foo"]);
        assert_eq!(parser.args.sysroot.as_deref(), Some("/foo"));
        parser.parse(&["--sysroot", "/bar"]);
        assert_eq!(parser.args.sysroot.as_deref(), Some("/bar"));
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn scripts_parsing() {
        let mut parser = setup_parser().unwrap();
        assert!(parser.args.scripts.is_empty());
        parser.parse(&["-T", "/foo", "-T/bar"]);
        assert_eq!(parser.args.scripts, ["/foo", "/bar"]);
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn wl_parsing() {
        let mut parser = setup_parser().unwrap();
        parser.parse(&[
            "-Wl,-z,text",
            "-Wl=-z,now",
            "-Wl,-soname=librustc_driver-23d51a7ae6381501.so",
        ]);
        let mut expected = vec![
            "-z",
            "text",
            "-z",
            "now",
            "-soname=librustc_driver-23d51a7ae6381501.so",
        ];
        assert_eq!(parser.args.raw_args, expected);
        expected.push("-z,relro");
        parser.parse(&["-Xlinker", "-z,relro"]);
        assert_eq!(parser.args.raw_args, expected);
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn compiler_args_parsing() {
        let mut parser = setup_parser().unwrap();
        let args = [
            "-B/foo",
            "-B",
            "/bar",
            "-Bstatic",
            "-m64",
            "-DNDEBUG",
            "-DFOO=BAR",
            "-O3",
            "-I/foo/bar",
            "-Werror",
            "-std=c23",
            "-fPIC",
            "-pedantic",
            "--pedantic",
            "-E",
        ];
        parser.parse(&args);
        assert_eq!(parser.args.compiler_args, args);
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn additional_libs_parsing() {
        let mut parser = setup_parser().unwrap();
        parser.parse(&["-lfoo", "-lbar"]);
        assert_eq!(parser.args.raw_args, ["-lfoo", "-lbar"]);
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn additional_search_paths_parsing() {
        let mut parser = setup_parser().unwrap();
        parser.parse(&["-L/foo", "-L", "/bar/baz"]);
        assert_eq!(
            parser.args.additional_search_paths,
            vec!["/foo", "/bar/baz"]
        );
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn target_parsing() {
        let mut parser = setup_parser().unwrap();
        parser.parse(&["--target=x86_64-linux-gnu"]);
        assert_eq!(parser.args.target.as_deref(), Some("x86_64-linux-gnu"));
        parser.parse(&["-target", "aarch64-linux-gnu"]);
        assert_eq!(parser.args.target.as_deref(), Some("aarch64-linux-gnu"));
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn coverage_parsing() {
        let mut parser = setup_parser().unwrap();
        assert!(!parser.args.coverage);
        parser.parse(&["--coverage"]);
        assert!(parser.args.coverage);
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn profile_parsing() {
        let mut parser = setup_parser().unwrap();
        assert!(!parser.args.profile);
        parser.parse(&["--profile"]);
        assert!(parser.args.profile);
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn pg_parsing() {
        let mut parser = setup_parser().unwrap();
        assert!(!parser.args.profile);
        parser.parse(&["-pg"]);
        assert!(parser.args.profile);
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn unknown_arg_parsing() {
        let mut parser = setup_parser().unwrap();
        let args = &[
            "-an-argument-that-does-not-exist",
            "--an-argument-that-does-not-exist",
        ];
        parser.parse(args);
        // TODO: Parser should return an error here
        assert_eq!(parser.unknown_args, args);
    }

    #[test]
    fn c_args_parsing() {
        let mut parser = setup_parser().unwrap();
        parser.parse(&["-x", "c"]);
        assert_eq!(parser.args.language.as_deref(), Some("c"));
        parser.parse(&["-xc++"]);
        assert_eq!(parser.args.language.as_deref(), Some("c++"));
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn unprefixed_args_parsing() {
        let mut parser = setup_parser().unwrap();
        let args = &["foo.c", "bar.o", "baz.a", "qux"];
        parser.parse(args);
        assert_eq!(parser.args.sources, args[..1]);
        assert_eq!(parser.args.raw_args, args[1..]);
        assert!(parser.args.input_objects_found);
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn out_parsing() {
        let mut parser = setup_parser().unwrap();
        assert_eq!(parser.args.output, "a.out");
        parser.parse(&["-o", "foo"]);
        assert_eq!(parser.args.output, "foo");
        parser.parse(&["-o/tmp/bar"]);
        assert_eq!(parser.args.output, "/tmp/bar");
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn dont_assemble_parsing() {
        let mut parser = setup_parser().unwrap();
        assert!(!parser.args.dont_assemble);
        parser.parse(&["-S"]);
        assert!(parser.args.dont_assemble);
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn dont_link_parsing() {
        let mut parser = setup_parser().unwrap();
        assert!(!parser.args.dont_link);
        parser.parse(&["-c"]);
        assert!(parser.args.dont_link);
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn z_parsing() {
        let mut parser = setup_parser().unwrap();
        parser.parse(&["-z", "now"]);
        assert_eq!(parser.args.raw_args, ["-z", "now"]);
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn u_parsing() {
        let mut parser = setup_parser().unwrap();
        parser.parse(&["-u", "foo", "-ubar"]);
        assert_eq!(parser.args.raw_args, ["-u", "foo", "-ubar"]);
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn fuse_ld_parsing() {
        let mut parser = setup_parser().unwrap();
        parser.parse(&["-fuse-ld=lld"]);
        assert_eq!(parser.args.fuse_ld.as_deref(), Some("lld"));
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn help_parsing() {
        let mut parser = setup_parser().unwrap();
        assert!(!parser.args.help);
        parser.parse(&["--help"]);
        assert!(parser.args.help);
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn hash_hash_hash_parsing() {
        let mut parser = setup_parser().unwrap();
        assert!(!parser.args.hash_hash_hash);
        parser.parse(&["-###"]);
        assert!(parser.args.hash_hash_hash);
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn v_parsing() {
        let mut parser = setup_parser().unwrap();
        assert!(!parser.args.verbose);
        parser.parse(&["-v"]);
        assert!(parser.args.verbose);
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn verbose_parsing() {
        let mut parser = setup_parser().unwrap();
        assert!(!parser.args.verbose);
        parser.parse(&["--verbose"]);
        assert!(parser.args.verbose);
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn version_parsing() {
        let mut parser = setup_parser().unwrap();
        assert!(!parser.args.version);
        parser.parse(&["--version"]);
        assert!(parser.args.version);
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn compiler_i_args_parsing() {
        let mut parser = setup_parser().unwrap();
        let args = [
            "-idirafter",
            "/foo",
            "-idirafter=/bar",
            "-imacros",
            "foo.h",
            "-imacros=bar.h",
            "-imultilib",
            "lib",
            "-imultilib=lib64",
            "-include",
            "foo.h",
            "-include=bar.h",
            "-iprefix",
            "/usr",
            "-iprefix=/opt",
            "-iquote",
            "/foo",
            "-iquote=/bar",
            "-isysroot",
            "/sysroot",
            "-isysroot=/newsysroot",
            "-isystem",
            "/usr/include",
            "-isystem=/opt/include",
            "-iwithprefix",
            "include",
            "-iwithprefix=local",
            "-iwithprefixbefore",
            "include",
            "-iwithprefixbefore=local",
        ];
        parser.parse(&args);
        assert_eq!(parser.args.compiler_args, args);
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn compiler_m_args_parsing() {
        let mut parser = setup_parser().unwrap();
        parser.parse(&[
            "-M",
            "should-not-be-added",
            "-MD",
            "-MF",
            "/tmp/foo.o.d",
            "-MG",
            "-MM",
            "-MMD",
            "-MP",
            "-MQ",
            "foo.o",
            "-MT",
            "bar.o",
            "-MV",
        ]);
        assert_eq!(
            parser.args.compiler_args,
            [
                "-M",
                "-MD",
                "-MF",
                "/tmp/foo.o.d",
                "-MG",
                "-MM",
                "-MMD",
                "-MP",
                "-MQ",
                "foo.o",
                "-MT",
                "bar.o",
                "-MV",
            ]
        );
        assert!(parser.unknown_args.is_empty());
    }

    #[test]
    fn pipe_parsing() {
        let mut parser = setup_parser().unwrap();
        let args = ["-pipe", "--pipe"];
        parser.parse(&args);
        assert_eq!(parser.args.compiler_args, args);
        assert!(parser.unknown_args.is_empty());
    }
}
