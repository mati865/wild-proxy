use crate::arch::{Arch, target_arch};
use anyhow::{Context, Result};

pub type LinkerArgs = Vec<String>;
pub type CompilerArgs = Vec<String>;

#[derive(Debug, PartialEq)]
pub enum Mode {
    CompileOnly,
    LinkOnly(LinkerArgs, DriverArgs),
    CompileAndLink((CompilerArgs, LinkerArgs, DriverArgs)),
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
    fn new(static_pie: bool, static_exe: bool, shared: bool, pie: bool) -> Self {
        if static_pie {
            OutputKind::StaticPie
        } else if static_exe {
            OutputKind::Static
        } else if shared {
            OutputKind::SharedObject
        } else if pie {
            OutputKind::DynamicPie
        } else {
            OutputKind::Dynamic
        }
    }
}

/// Arguments for both
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub struct DriverArgs {
    pub(crate) output: String,
    pub(crate) objects_and_libs: Vec<String>,
    pub(crate) output_kind: OutputKind,
    pub(crate) default_libs: bool,
    pub(crate) profile: bool,
    pub(crate) coverage: bool,
    pub(crate) arch: Arch,
    pub(crate) sysroot: Option<String>,
    pub(crate) pthread: bool,
}

impl Default for DriverArgs {
    fn default() -> Self {
        Self {
            output: "a.out".to_string(),
            objects_and_libs: vec![],
            output_kind: Default::default(),
            default_libs: true,
            profile: false,
            coverage: false,
            arch: Arch::default(),
            sysroot: None,
            pthread: false,
        }
    }
}

// TODO: optimise
pub(crate) fn parse(args: &[String], target: Option<Arch>) -> Result<Mode> {
    // Exit early if not linking at all.
    if args.iter().map(AsRef::as_ref).any(is_compile_only_arg) {
        return Ok(Mode::CompileOnly);
    }

    let mut args = args.into_iter();

    let mut compiler_args = Vec::with_capacity(args.len());
    let mut linker_args = Vec::with_capacity(args.len());
    let mut driver_args = DriverArgs::default();
    let mut unknown_args = Vec::default();

    if let Some(target) = target {
        driver_args.arch = target;
    }

    let mut are_sources_present = false;
    let mut pie = true;
    let mut shared = false;
    let mut static_exe = false;
    let mut static_pie = false;

    while let Some(arg) = args.next() {
        // Driver args
        {
            if parse_and_append_arg("-T", &arg, &mut args, &mut linker_args) {
                continue;
            }
            if let Some(args) = arg.strip_prefix("-Wl,") {
                linker_args.extend(args.split(',').map(|x| x.into()));
                continue;
            }
            if let Some(arg) = parse_arg("-Xlinker", &arg, &mut args) {
                linker_args.push(arg);
                continue;
            }

            // TODO: This is used by Rust, handle others as well.
            match arg.as_str() {
                "-nodefaultlibs" => {
                    driver_args.default_libs = false;
                    continue;
                }
                _ => {}
            }

            match arg.as_str() {
                "-pie" | "--pie" => {
                    pie = true;
                    continue;
                }
                "-no-pie" | "--no-pie" => {
                    pie = false;
                    continue;
                }
                "-static" | "--static" => {
                    static_exe = true;
                    continue;
                }
                "-static-pie" | "--static-pie" => {
                    static_pie = true;
                    continue;
                }
                "-shared" | "--shared" => {
                    shared = true;
                    continue;
                }
                _ => (),
            }
        }

        // Args that the compiler and linker driver need to handle.
        {
            if let Some(output) = parse_arg("-o", &arg, &mut args) {
                driver_args.output = output.to_string();
                compiler_args.push(arg.to_string());
                continue;
            } else if arg == "-pthread" || arg == "--pthread" {
                driver_args.pthread = true;
                compiler_args.push(arg.to_string());
                continue;
            } else if arg == "-no-pthread" || arg == "--no-pthread" {
                driver_args.pthread = false;
                compiler_args.push(arg.to_string());
                continue;
            } else if arg == "-pg" || arg == "--profile" {
                driver_args.profile = true;
                compiler_args.push(arg.to_string());
                continue;
            } else if arg == "-coverage" || arg == "--coverage" {
                driver_args.coverage = true;
                compiler_args.push(arg.to_string());
                continue;
            } else if let Some(target) = arg.strip_prefix("--target=") {
                driver_args.arch = target_arch(target)?;
                compiler_args.push(arg.to_string());
                continue;
            } else if arg == "-target" {
                let target = args
                    .next()
                    .with_context(|| format!("Unexpected end of arguments after: {arg}"))?;
                driver_args.arch = target_arch(target)?;
                compiler_args.extend([arg.to_string(), target.to_string()]);
                continue;
            }

            if let Some(path) = parse_arg("--sysroot", &arg, &mut args) {
                let sysroot_arg = format!("--sysroot={}", path);
                compiler_args.push(sysroot_arg.clone());
                linker_args.push(sysroot_arg);
                driver_args.sysroot = Some(path);
                continue;
            }
        }

        // Compilers seem to care only about supported extension or `-x` argument when determining
        // whether a file is source code.
        {
            if parse_and_append_arg("-x", &arg, &mut args, &mut compiler_args) {
                are_sources_present = true;
                continue;
            } else if arg.ends_with(".c")
                || arg.ends_with(".cc")
                || arg.ends_with(".cpp")
                || arg.ends_with(".s")
                || arg.ends_with(".S")
            {
                compiler_args.push(arg.to_string());
                are_sources_present = true;
                continue;
            }
        }

        {
            let found = ["-l", "-L"].iter().any(|&linker_arg| {
                parse_and_append_arg(
                    linker_arg,
                    &arg,
                    &mut args,
                    &mut driver_args.objects_and_libs,
                )
            });
            if found {
                continue;
            }
            if !arg.starts_with('-') {
                driver_args.objects_and_libs.push(arg.to_string());
                continue;
            }
        }

        if arg == "-pedantic"
            || arg == "--pedantic"
            || arg.starts_with("-f")
            || arg.starts_with("-m")
            || arg.starts_with("-W")
            || arg.starts_with("-O")
            || arg.starts_with("-D")
        {
            compiler_args.push(arg.to_string());
        } else {
            unknown_args.push(arg);
        }
    }

    driver_args.output_kind = OutputKind::new(static_pie, static_exe, shared, pie);

    let mode = if are_sources_present {
        Mode::CompileAndLink((compiler_args, linker_args, driver_args))
    } else if driver_args.objects_and_libs.is_empty() {
        Mode::CompileOnly
    } else {
        if !unknown_args.is_empty() {
            eprintln!("Unhandled arguments: {:?}", &unknown_args);
        }
        Mode::LinkOnly(linker_args, driver_args)
    };
    Ok(mode)
}

pub(crate) fn is_compile_only_arg(arg: &str) -> bool {
    // TODO: handle some of them
    ["--help", "--version", "-###", "-c", "-S"].contains(&arg)
        || arg.starts_with("-dump")
        || arg.starts_with("-print")
}

// TODO: find a better way
// true means the argument matched
fn parse_and_append_arg<'a>(
    desired_arg: &str,
    actual_arg: &str,
    additional_arg: &mut impl Iterator<Item = &'a String>,
    vec: &mut Vec<String>,
) -> bool {
    match actual_arg.strip_prefix(desired_arg) {
        Some("") => {
            let rest = additional_arg
                .next()
                .unwrap_or_else(|| panic!("missing arg to {desired_arg}"));
            vec.extend_from_slice(&[desired_arg.to_string(), rest.to_string()]);
        }
        Some(_) => {
            vec.push(actual_arg.to_string());
        }
        None => return false,
    }
    true
}

// TODO: find a better way
fn parse_arg<'a>(
    desired_arg: &str,
    actual_arg: &str,
    additional_arg: &mut impl Iterator<Item = &'a String>,
) -> Option<String> {
    match actual_arg.strip_prefix(desired_arg) {
        Some("") => Some(
            additional_arg
                .next()
                .unwrap_or_else(|| panic!("missing arg to {desired_arg}"))
                .to_string(),
        ),
        Some(rest) => Some(rest.trim_start_matches("=").to_string()),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_args() {
        let args = vec![];
        assert!(matches!(parse(&args, None), Ok(Mode::CompileOnly)));
    }

    #[test]
    fn compile_only_mode() {
        let args = vec!["foo.c".to_string(), "-c".to_string()];
        assert_eq!(Mode::CompileOnly, parse(&args, None).unwrap());
    }

    #[test]
    fn compile_and_link_mode_with_sources() {
        let args = vec!["foo.c".to_string()];
        assert!(matches!(parse(&args, None), Ok(Mode::CompileAndLink(_))));
    }

    #[test]
    fn compile_and_link_mode_with_sources_and_objects() {
        let args = vec!["foo.c".to_string(), "bar.o".to_string()];
        assert!(matches!(parse(&args, None), Ok(Mode::CompileAndLink(_))));
    }

    #[test]
    fn link_mode() {
        let args = vec!["foo.o".to_string()];
        assert!(matches!(parse(&args, None), Ok(Mode::LinkOnly(_, _))));
    }

    #[test]
    fn default_output_kind() {
        let args = vec!["foo.o".to_string()];
        let Mode::LinkOnly(_, driver_args) = parse(&args, None).unwrap() else {
            panic!()
        };
        assert_eq!(driver_args.output_kind, OutputKind::DynamicPie);
    }

    #[test]
    fn linker_args() {
        let args = [
            "-L",
            "foo",
            "-lfoo",
            "bar.o",
            "-Lbaz",
            "-lbaz",
            "-T",
            "/foo",
            "-T/bar",
            "-Wl,-v,-z,now",
            "-Xlinker",
            "--build-id",
        ]
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
        let Mode::LinkOnly(linker_args, driver_args) = parse(&args, None).unwrap() else {
            panic!("Wrong driver mode")
        };
        assert_eq!(
            linker_args,
            ["-T", "/foo", "-T/bar", "-v", "-z", "now", "--build-id"]
        );
        assert_eq!(
            driver_args.objects_and_libs,
            ["-L", "foo", "-lfoo", "bar.o", "-Lbaz", "-lbaz"]
        );
    }

    #[test]
    fn pthread_arg() {
        let mut args = vec!["foo.o".to_string()];
        assert!(matches!(
            parse(&args, None),
            Ok(Mode::LinkOnly(_, DriverArgs { pthread: false, .. }))
        ));
        args.push("-no-pthread".to_string());
        assert!(matches!(
            parse(&args, None),
            Ok(Mode::LinkOnly(_, DriverArgs { pthread: false, .. }))
        ));
        args.push("-pthread".to_string());
        assert!(matches!(
            parse(&args, None),
            Ok(Mode::LinkOnly(_, DriverArgs { pthread: true, .. }))
        ));
        args.push("--no-pthread".to_string());
        assert!(matches!(
            parse(&args, None),
            Ok(Mode::LinkOnly(_, DriverArgs { pthread: false, .. }))
        ));
        args.push("--pthread".to_string());
        assert!(matches!(
            parse(&args, None),
            Ok(Mode::LinkOnly(_, DriverArgs { pthread: true, .. }))
        ));
    }

    #[test]
    fn target_arg() {
        let mut args = vec![
            "foo.o".to_string(),
            "-target".to_string(),
            "x86_64-linux-gnu".to_string(),
        ];
        assert!(matches!(
            parse(&args, None),
            Ok(Mode::LinkOnly(
                _,
                DriverArgs {
                    arch: Arch::X86_64,
                    ..
                }
            ))
        ));
        args.push("--target=aarch64-pc-linux-gnu".to_string());
        assert!(matches!(
            parse(&args, None),
            Ok(Mode::LinkOnly(
                _,
                DriverArgs {
                    arch: Arch::Aarch64,
                    ..
                }
            ))
        ));
    }

    #[test]
    fn coverage_arg() {
        let mut args = vec!["foo.o".to_string()];
        assert!(matches!(
            parse(&args, None),
            Ok(Mode::LinkOnly(
                _,
                DriverArgs {
                    coverage: false,
                    ..
                }
            ))
        ));
        args.push("--coverage".to_string());
        assert!(matches!(
            parse(&args, None),
            Ok(Mode::LinkOnly(_, DriverArgs { coverage: true, .. }))
        ));
    }

    #[test]
    fn sysroot_arg() {
        let mut args = vec!["foo.o".to_string(), "--sysroot=/foo".to_string()];
        assert!(matches!(
            parse(&args, None),
            Ok(Mode::LinkOnly(
                _,
                DriverArgs {
                    sysroot: Some(ref s),
                    ..
                }
            )) if s == "/foo"
        ));
        args.extend(["--sysroot".to_string(), "/bar".to_string()]);
        assert!(matches!(
            parse(&args, None),
            Ok(Mode::LinkOnly(
                _,
                DriverArgs {
                    sysroot: Some(ref s),
                    ..
                }
            )) if dbg!(s) == "/bar"
        ))
    }

    #[test]
    fn nodefaultlibs_arg() {
        let args = vec!["foo.o".to_string(), "--nodefaultlibs".to_string()];
        assert!(matches!(
            parse(&args, None),
            Ok(Mode::LinkOnly(
                _,
                DriverArgs {
                    default_libs: true,
                    ..
                }
            ))
        ));
    }

    #[test]
    fn profile_arg() {
        let args = vec![
            "foo.o".to_string(),
            "--profile".to_string(),
            "-profile".to_string(),
        ];
        assert!(matches!(
            parse(&args, None),
            Ok(Mode::LinkOnly(_, DriverArgs { profile: true, .. }))
        ));
    }

    #[test]
    fn pg_arg() {
        let args = vec!["foo.o".to_string(), "-pg".to_string()];
        assert!(matches!(
            parse(&args, None),
            Ok(Mode::LinkOnly(_, DriverArgs { profile: true, .. }))
        ));
    }

    #[test]
    fn unknown_arg() {
        let args = vec!["foo.o".to_string(), "--unknown".to_string()];
        assert!(matches!(parse(&args, None), Ok(Mode::LinkOnly(_, _))));
    }
}
