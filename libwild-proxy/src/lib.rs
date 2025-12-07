use crate::arg_parser::Mode;
use anyhow::{Context, bail};
use std::{
    env::Args,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::Command,
};

mod arg_parser;
pub mod fallback;
mod link;
mod outputs_cleanup;

pub fn process(mut args: Args) -> anyhow::Result<()> {
    let zero_position_arg = args
        .next()
        .context("Could not obtain binary name from args")?;
    let args = args
        .filter(|s| !s.starts_with("-fuse-ld="))
        .collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--pipe") {
        bail!("--pipe is not supported yet");
    }

    // TODO: Extension if running on Windows
    let cpp_mode = zero_position_arg.ends_with("++");
    let parsed = arg_parser::parse(&args)?;

    match parsed {
        Mode::CompileOnly => {
            let compiler_path = find_next_executable(&zero_position_arg)?;
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

pub(crate) fn find_next_executable(zero_position_arg: &str) -> anyhow::Result<PathBuf> {
    let mut wanted_exe = Path::new(zero_position_arg)
        .file_stem()
        .context("args[0] has no file stem")?;
    let real_exe = std::env::current_exe().context("Could not get current exe path")?;
    let binary_name = real_exe
        .file_stem()
        .context("Current exe has no file stem")?;
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
