use crate::{arch::target_arch, args::Mode};
use anyhow::{Context, Result, bail};
use std::{
    env::Args,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::Command,
};

mod arch;
mod arg_parser;
mod args;
pub mod fallback;
mod link;
mod outputs_cleanup;

pub fn process(mut args: Args, binary_name: &str) -> Result<()> {
    let zero_position_arg = &args
        .next()
        .context("Could not obtain binary name from args")?;
    let zero_position_path = Path::new(zero_position_arg);
    let args = args
        .filter(|s| !s.starts_with("-fuse-ld="))
        .collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--pipe") {
        bail!("--pipe is not supported yet");
    }

    let executable_name: String = zero_position_path
        .file_stem()
        .map(|stem| stem.to_str().unwrap().to_string())
        .or_else(|| {
            std::env::current_exe().ok().and_then(|path| {
                path.file_stem()
                    .map(|stem| stem.to_str().unwrap().to_string())
            })
        })
        .with_context(|| "Could not determine binary name")?;
    let cpp_mode;
    let target;
    if executable_name != binary_name {
        cpp_mode = executable_name.ends_with("++");
        let target_str = executable_name.rsplit_once("-");
        target = target_str
            .map(|(triple, _)| target_arch(triple))
            .transpose()?;
    } else {
        cpp_mode = false;
        target = None;
    };
    let parsed = args::parse(&args, target)?;

    match parsed {
        Mode::CompileOnly => {
            let compiler_path = find_next_executable(&zero_position_path)?;
            let mut compiler_command = Command::new(&compiler_path);
            let err = compiler_command.args(&args).exec();
            bail!(
                "Failed to exec compiler {}: {}",
                compiler_path.display(),
                err
            );
        }
        Mode::LinkOnly(linker_args, driver_args) => {
            link::link(linker_args, driver_args, cpp_mode)?;
        }
        Mode::CompileAndLink((compiler_args, linker_args, driver_args)) => {
            dbg!(&compiler_args, &linker_args, &driver_args);
            return fallback::fallback();
        }
    }

    Ok(())
}

pub(crate) fn find_next_executable(zero_position_arg: &Path) -> Result<PathBuf> {
    let mut wanted_exe = zero_position_arg
        .file_stem()
        .context("args[0] has no file stem")?;
    let real_exe = std::env::current_exe().context("Could not get current exe path")?;
    let binary_name = real_exe
        .file_stem()
        .context("Current exe has no file stem")?;
    // TODO: Maybe just look for gcc or clang string?
    if wanted_exe == binary_name {
        wanted_exe = "cc".as_ref();
    }
    let paths = std::env::var_os("PATH").context("Could not get PATH env variable")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(wanted_exe);
        if let Ok(meta) = std::fs::symlink_metadata(&candidate) {
            let mode = meta.permissions().mode();
            // Owner, group or others executable and not this wrapper?
            if mode & 0o111 != 0
                && (!meta.is_symlink()
                    || candidate
                        .read_link()
                        .is_ok_and(|path| path.file_stem() != Some(binary_name)))
            {
                return Ok(candidate);
            }
        }
    }
    bail!(
        "Could not find {} other than this wrapper in PATH",
        wanted_exe.display()
    );
}
