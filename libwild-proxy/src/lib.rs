use crate::{arch::target_arch, args::Mode};
use anyhow::{Context, Result, bail};
use std::{
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

pub fn process(args: &[&str], zero_position_arg: &str, binary_name: &str) -> Result<()> {
    if args.is_empty() {
        bail!("no input files")
    }

    let zero_position_path = Path::new(zero_position_arg);
    // let args = args
    //     .into_iter()
    //     .copied()
    //     .filter(|s| !s.starts_with("-fuse-ld="))
    //     .collect::<Vec<_>>();
    if args.iter().any(|arg| *arg == "--pipe") {
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

    let parsed_args = args::Args::parse_args(&args, target)?;

    if parsed_args.help {
        bail!("Help is not supported yet");
    }

    match parsed_args.mode {
        Mode::CompileOnly => {
            let compiler_path = find_next_executable(&zero_position_path)?;
            let mut compiler_command = Command::new(&compiler_path);
            let err = compiler_command.args(&*args).exec();
            bail!(
                "Failed to exec compiler {}: {}",
                compiler_path.display(),
                err
            );
        }
        Mode::LinkOnly => {
            link::link(&parsed_args, cpp_mode)?;
        }
        Mode::CompileAndLink => {
            dbg!(&parsed_args);
            return fallback::fallback();
        }
        Mode::None => {
            bail!("Could not determine what to do with arguments: {args:?}");
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

#[cfg(test)]
mod tests {
    use crate::{args::Args, link::build_link_args};
    use pretty_assertions::assert_eq;

    #[test]
    fn rustc_link() {
        let args = vec![
            "-m64",
            "/tmp/rustc9n9gBH/symbols.o",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/rustc_main-d019688b2c42e12c.rustc_main.e3546e6658d3c99d-cgu.0.rcgu.o",
            "-Wl,--as-needed",
            "-Wl,-Bdynamic",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_driver-00204bc794c5a4d0.so",
            "-Wl,-Bstatic",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libcompiler_builtins-a3a5501b3d3823e7.rlib",
            "-Wl,-Bdynamic",
            "-ldl",
            "-lLLVM-21-rust-1.94.0-nightly",
            "-lstdc++",
            "-ldl",
            "-lgcc_s",
            "-lutil",
            "-lrt",
            "-lpthread",
            "-lm",
            "-ldl",
            "-lc",
            "-L",
            "/tmp/rustc9n9gBH/raw-dylibs",
            "-B/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/bin/gcc-ld",
            "-B/build/rust/build/x86_64-unknown-linux-gnu/stage0/lib/rustlib/x86_64-unknown-linux-gnu/bin/gcc-ld",
            "-fuse-ld=lld",
            "-Wl,--eh-frame-hdr",
            "-Wl,-z,noexecstack",
            "-L",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/build/psm-9ec73701addafb56/out",
            "-L",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/build/blake3-96dd8f07c6c50b58/out",
            "-L",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/build/rustc_llvm-10f1fcf7999e0d0e/out",
            "-L",
            "/build/rust/build/x86_64-unknown-linux-gnu/ci-llvm/lib",
            "-L",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib",
            "-o",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/rustc_main-d019688b2c42e12c",
            "-Wl,--gc-sections",
            "-pie",
            "-Wl,-z,relro,-z,now",
            "-Wl,-O1",
            "-nodefaultlibs",
            "-Wl,-z,origin",
            "-Wl,-rpath,/../lib",
        ];
        let parsed = Args::parse_args(&args, None).unwrap();
        let link_args = build_link_args(&parsed, false).unwrap();
        assert_eq!(
            link_args,
            vec![
                "--hash-style=gnu",
                "--build-id",
                "--eh-frame-hdr",
                "-m",
                "elf_x86_64",
                "-pie",
                "--dynamic-linker",
                "/lib64/ld-linux-x86-64.so.2",
                "-o",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/rustc_main-d019688b2c42e12c",
                "//lib64/Scrt1.o",
                "//lib64/crti.o",
                "/usr/lib64/gcc/x86_64-pc-linux-gnu/15.2.1/crtbeginS.o",
                "-L/tmp/rustc9n9gBH/raw-dylibs",
                "-L/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/build/psm-9ec73701addafb56/out",
                "-L/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/build/blake3-96dd8f07c6c50b58/out",
                "-L/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/build/rustc_llvm-10f1fcf7999e0d0e/out",
                "-L/build/rust/build/x86_64-unknown-linux-gnu/ci-llvm/lib",
                "-L/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib",
                "-L/usr/lib64/gcc/x86_64-pc-linux-gnu/15.2.1",
                "-L//lib64",
                "-L//usr/lib64",
                "-L//lib",
                "-L//usr/lib",
                "/tmp/rustc9n9gBH/symbols.o",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/rustc_main-d019688b2c42e12c.rustc_main.e3546e6658d3c99d-cgu.0.rcgu.o",
                "--as-needed",
                "-Bdynamic",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_driver-00204bc794c5a4d0.so",
                "-Bstatic",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libcompiler_builtins-a3a5501b3d3823e7.rlib",
                "-Bdynamic",
                "-ldl",
                "-lLLVM-21-rust-1.94.0-nightly",
                "-lstdc++",
                "-ldl",
                "-lgcc_s",
                "-lutil",
                "-lrt",
                "-lpthread",
                "-lm",
                "-ldl",
                "-lc",
                "--eh-frame-hdr",
                "-z",
                "noexecstack",
                "--gc-sections",
                "-z",
                "relro",
                "-z",
                "now",
                "-O1",
                "-z",
                "origin",
                "-rpath",
                "/../lib",
                "/usr/lib64/gcc/x86_64-pc-linux-gnu/15.2.1/crtendS.o",
                "//lib64/crtn.o"
            ]
        )
    }
}
