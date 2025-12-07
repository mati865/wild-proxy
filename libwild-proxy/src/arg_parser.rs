use anyhow::Result;

pub type LinkerArgs = Vec<String>;
pub type CompilerArgs = Vec<String>;

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
#[derive(Debug)]
#[non_exhaustive]
pub struct DriverArgs {
    pub(crate) output: String,
    pub(crate) objects_and_libs: Vec<String>,
    pub(crate) output_kind: OutputKind,
    pub(crate) _profiler: bool,
}

impl Default for DriverArgs {
    fn default() -> Self {
        Self {
            output: "a.out".to_string(),
            objects_and_libs: vec![],
            output_kind: Default::default(),
            _profiler: false,
        }
    }
}

// TODO: optimise
pub(crate) fn parse(args: &[String]) -> Result<Mode> {
    // Exit early if not linking at all.
    if args.iter().map(AsRef::as_ref).any(is_compile_only_arg) {
        return Ok(Mode::CompileOnly);
    }

    let mut args = args.into_iter();

    let mut compiler_args = Vec::with_capacity(args.len());
    let mut linker_args = Vec::with_capacity(args.len());
    let mut driver_args = DriverArgs::default();
    let mut are_sources_present = false;

    let mut pie = true;
    let mut shared = false;
    let mut static_exe = false;
    let mut static_pie = false;

    while let Some(arg) = args.next() {
        // Linker args accepted by the driver
        {
            let found = ["-T", "--sysroot"].iter().any(|&linker_arg| {
                parse_and_append_arg(linker_arg, &arg, &mut args, &mut linker_args)
            });
            if found {
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
        }

        // Args that the compiler and linker driver need to handle.
        {
            if let Some(output) = parse_arg("-o", &arg, &mut args) {
                driver_args.output = output.to_string();
                compiler_args.push(arg.to_string());
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
            // TODO: if arg doesn't start with `-`, store it separately to add between start and end
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

        {
            if arg == "-pthread" {
                driver_args.objects_and_libs.push("-lpthread".to_string());
            }
        }

        {
            match arg.as_str() {
                "-pie" => {
                    pie = true;
                    continue;
                }
                "-no-pie" => {
                    pie = false;
                    continue;
                }
                "-static" => {
                    static_exe = true;
                    continue;
                }
                "-static-pie" => {
                    static_pie = true;
                    continue;
                }
                "-shared" => {
                    shared = true;
                    continue;
                }
                _ => (),
            }
        }

        compiler_args.push(arg.to_string());
    }

    driver_args.output_kind = OutputKind::new(static_pie, static_exe, shared, pie);

    let mode = if are_sources_present {
        Mode::CompileAndLink((compiler_args, linker_args, driver_args))
    } else {
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
