use crate::{arch::target_arch, args::Mode};
use anyhow::{Context, Result, bail};
use std::{
    io::Read,
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

pub fn process(original_args: &[&str], zero_position_arg: &str, binary_name: &str) -> Result<()> {
    let zero_position_path = Path::new(zero_position_arg);

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
        // TODO: This is out of place and I should think how to do it nicely.
        if executable_name == "wild" || executable_name == "ld.wild" {
            let wild_args = match libwild::Args::parse(|| original_args.iter()) {
                Ok(args) => args,
                Err(e) => {
                    bail!("Wild args parse error: {}", e.to_string());
                }
            };
            unsafe { libwild::run_in_subprocess(wild_args) }
        }
        cpp_mode = executable_name.ends_with("++");
        let target_str = executable_name.rsplit_once("-");
        target = target_str
            .map(|(triple, _)| target_arch(triple))
            .transpose()?;
    } else {
        cpp_mode = false;
        target = None;
    };

    // TODO: Add test
    let mut response_files_contents = Vec::new();
    for arg in original_args {
        if arg.starts_with("@") {
            let filename = &arg[1..];
            let mut file = std::fs::File::open(filename)
                .with_context(|| format!("Could not open response file {filename}"))?;
            let mut contents = String::new();
            file.read_to_string(&mut contents)
                .with_context(|| format!("Could not read response file {filename}"))?;
            let split_args = shell_words::split(contents.as_str())?;
            response_files_contents.push(split_args);
        }
    }
    let mut response_files_contents_iter = response_files_contents.iter();
    let mut args = Vec::with_capacity(
        original_args.len()
            + response_files_contents
                .iter()
                .map(Vec::len)
                .reduce(|acc, e| acc + e)
                .unwrap_or(0),
    );
    for arg in original_args {
        if arg.starts_with("@") {
            let new_args = response_files_contents_iter
                .next()
                .unwrap()
                .iter()
                .map(|arg| arg.as_str());
            args.extend(new_args);
        } else {
            args.push(arg);
        }
    }

    let parsed_args = args::Args::parse_args(&args, target, cpp_mode)?;

    if parsed_args.help {
        bail!("Help is not supported yet");
    }

    let interposed_compiler_path = find_next_executable(&zero_position_path)?;

    if parsed_args.hash_hash_hash || parsed_args.verbose || parsed_args.version {
        println!("{binary_name} version {}", env!("CARGO_PKG_VERSION"));
        let interposed_compiler = interposed_compiler_path
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();
        // Zlib's `configure` script looks for "gcc" or "clang" in the output.
        println!(
            "Compatible with other compilers CLI, currently interposing: {interposed_compiler}"
        );
        // In `Compile*` modes will spawn the compiler anyway, avoid doing it twice.
        if parsed_args.mode != Mode::CompileOnly && parsed_args.mode != Mode::CompileAndLink {
            Command::new(&interposed_compiler_path)
                .args(args)
                .arg("-c")
                .status()?;
        }
    } else if parsed_args.mode == Mode::None {
        bail!("no input files")
    }

    if let Some(fuse_ld) = &parsed_args.fuse_ld {
        eprintln!("warn: ignoring -fuse-ld={}", fuse_ld);
    }

    match parsed_args.mode {
        Mode::CompileOnly => {
            let mut compiler_command = Command::new(&interposed_compiler_path);
            let err = compiler_command.args(original_args).exec();
            bail!(
                "Failed to exec compiler {}: {}",
                interposed_compiler_path.display(),
                err
            );
        }
        Mode::LinkOnly => {
            if !parsed_args.version {
                link::link(&parsed_args)?;
            }
        }
        Mode::CompileAndLink => {
            return fallback::fallback();
        }
        Mode::Print => {
            if let Some(name) = parsed_args.print_prog_name {
                println!("{name}")
            } else {
                bail!("Only `--print-prog-name` is supported yet")
            }
        }
        Mode::None => {
            // Do nothing
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

    // TODO: Mock GCC dir
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
        let parsed = Args::parse_args(&args, None, false).unwrap();
        let link_args = build_link_args(&parsed).unwrap();
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
                "/lib64/Scrt1.o",
                "/lib64/crti.o",
                "/usr/lib64/gcc/x86_64-pc-linux-gnu/15.2.1/crtbeginS.o",
                "-L/tmp/rustc9n9gBH/raw-dylibs",
                "-L/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/build/psm-9ec73701addafb56/out",
                "-L/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/build/blake3-96dd8f07c6c50b58/out",
                "-L/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/build/rustc_llvm-10f1fcf7999e0d0e/out",
                "-L/build/rust/build/x86_64-unknown-linux-gnu/ci-llvm/lib",
                "-L/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib",
                "-L/usr/lib64/gcc/x86_64-pc-linux-gnu/15.2.1",
                "-L/lib64",
                "-L/usr/lib64",
                "-L/lib",
                "-L/usr/lib",
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
                "/lib64/crtn.o"
            ]
        )
    }

    #[test]
    fn librustc_driver_link() {
        let args = vec![
            "-Wl,--version-script=/tmp/rustcKk4Xlj/list",
            "-Wl,--no-undefined-version",
            "-m64",
            "/tmp/rustcKk4Xlj/symbols.o",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/rustc_driver-23d51a7ae6381501.rustc_driver.31a38ef3d8373027-cgu.0.rcgu.o",
            "/tmp/rustcKk4Xlj/rmeta.o",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/rustc_driver-23d51a7ae6381501.8tfkk5unuq490u6lq0gbd6v4f.rcgu.o",
            "-Wl,--as-needed",
            "-Wl,-Bstatic",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_driver_impl-bd6892f158872bb6.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libctrlc-d38327187ce07e0f.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libnix-64e0e3a9d1b0fb75.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_log-fd5732f26166c7fe.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtracing_tree-24892338644f0083.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtracing_log-149389d03d66ff20.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtracing_subscriber-1e07328309495aec.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libsharded_slab-d62d485df448ccff.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/liblazy_static-d5ea42230b171193.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libmatchers-36b0220694507dd4.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libnu_ansi_term-762f8b00e03c212a.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libthread_local-583f63a74e0b150a.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libjiff-91ff6f4390ae9016.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libshlex-99d3a341c9a86137.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_public-0f4554cd16bed968.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_public_bridge-72ce244932e44af6.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_interface-d8683ea701a8941c.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_codegen_llvm-c6d357baf2d6d382.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/liblibloading-508bc7b07167e85e.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_llvm-da5b440c7123b117.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_sanitizers-f5de2ba89027eebb.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_hir_typeck-ecce6f9a84d85e42.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_hir_analysis-cfad7a1533f4131c.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_monomorphize-c1580aeeb69cdb5f.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_mir_transform-55501824e5603848.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_mir_build-427d04da3a95e28c.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_pattern_analysis-128da25e2283fc1d.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_borrowck-c4b7282fe5427137.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_traits-025c5024dfb8a43c.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_const_eval-996a8ff580db355e.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_mir_dataflow-245764647131cca4.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_builtin_macros-6e3a732d3821cd40.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_resolve-e20e7d7c250811fc.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libpulldown_cmark-06374c9f5517d00f.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libunicase-56fec33e7d012112.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libpulldown_cmark_escape-b47006a888fa87ae.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_passes-7a2a58795be0375f.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_ast_lowering-97e8e8cf20f0806e.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_privacy-66e8b0a8f821461f.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_ty_utils-1a19758b38889e98.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_lint-d5baaba9f9ef3596.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libunicode_security-a0b1ddc091ba25d8.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libunicode_script-eff2a1d2bacad010.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_query_impl-b3e40b45a6c45c19.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_codegen_ssa-d9fff5c84497631e.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libwasm_encoder-474de55290d8d020.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libleb128-c0ff2e479cd49024.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libthorin-418d4b81a24ff317.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libgimli-239fef8f2e61be60.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libfallible_iterator-bb1cffb49b35862e.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libobject-aa38e621d939f62a.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libruzstd-0845499878594a3d.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtwox_hash-7a7641d3e02f2a06.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librand-bd46b4e082a763f6.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librand_chacha-b04ab6e22f51e3cf.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librand_core-d52df22c4d1e962d.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libgetrandom-6421c1509abca187.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libstatic_assertions-de6b6dffccb50005.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libflate2-d38efc1122a288e7.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libminiz_oxide-6e047e15977f5ad7.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libsimd_adler32-35feb0f6f77a7e75.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libbstr-e8e7727377b48c66.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_trait_selection-ac2cc0becda9034c.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_next_trait_solver-07a91be87b1962ab.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_parse_format-8cfcc7565f5c1dbe.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_transmute-4f3102ca9fc12c75.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_infer-200a9c83777301e9.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_symbol_mangling-91f934cae5c1229e.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_demangle-2aaa6b3b7c07692e.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libpunycode-c9621206b7d2ec90.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libpathdiff-e0c43cb181445ff1.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libregex-6866054342cd6db6.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libfind_msvc_tools-6826e0da5d6b9644.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libar_archive_writer-f48cc112ee6afb56.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_metadata-e2e7d3e380ea2581.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/liblibloading-74e12fb1faa2b597.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_incremental-48f9047151b993ed.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_expand-afb5ab7491ba40f4.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_ast_passes-17894f552798cb0b.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_proc_macro-dbb99f277c1cbb52.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_attr_parsing-a67ec3fe0a04ad84.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_parse-f90f19e6c0f97d2e.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libunicode_normalization-4e410ac1004a1776.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtinyvec-765c0201d2a38046.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtinyvec_macros-b5ce11dd7f9de8a4.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_middle-aeea4739c8bc49b9.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_apfloat-7e5d19dcb77b5f34.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libgsgdt-c162380bde251ede.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libpolonius_engine-fc9739920eff7841.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libdatafrog-7d91c110c8f87484.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_query_system-25e262e73ac192ba.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_type_ir-008cc4121b281123.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_session-1255b55c4c7774a1.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libgetopts-a2562a16ef7406e0.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_feature-e05101130450eb0c.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_hir_pretty-e2175b614aac9650.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_errors-c13a1da85a497b3d.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtermize-c9f1481965c0c8e0.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_error_codes-5663bb6db5e1e925.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libannotate_snippets-887de6400b2674c7.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libanstream-b310d562f99c823f.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libanstyle_query-829fea5d9999e51e.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libis_terminal_polyfill-c08748f86e33ebd6.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libanstyle-0572e7a3163b0305.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libcolorchoice-ba4a7cf58d3d1cf6.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libanstyle_parse-c4b2fa4175aeaab1.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libutf8parse-71ed97eb02e1c9e7.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_hir-2a1a42fd3019c390.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libodht-dcb0a8017fb070c3.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_lint_defs-8d7484a6ef493a31.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_hir_id-982c55964d3e2cf1.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_target-e70e6ff34c7356dd.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libserde_path_to_error-361fceb2d0243016.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libschemars-82d3f008dfe11f47.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libdyn_clone-a6e43562e70c7064.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libserde-c2054341d1c06b8e.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libref_cast-5df3ffc280115468.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libobject-d694ea94a992f5be.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libcrc32fast-a1dd814449c5f62a.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libwasmparser-d195adac3f6af65d.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_fs_util-5609f6ada267bf3c.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libserde_json-db06470bdba04427.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libryu-606419a09a27540b.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libserde_core-ab11963c982bae61.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_abi-53ea049bb63e1eb6.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librand_xoshiro-d8a81a649342ccc4.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_error_messages-9b3dfbfab159b64a.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_baked_icu_data-6880d557e391b2df.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libicu_list-d4ddcd99c9df1ab4.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libregex_automata-ab85046163f05cbe.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libaho_corasick-3e558d5aa4e8bf40.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libregex_syntax-aaed4e0c494ce1a8.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libicu_locale-8a042c5791987e22.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libicu_locale_data-c79827aa62e7a5d5.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libicu_collections-d2136bd6bc3eb411.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libpotential_utf-c988d67831482dd0.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libicu_provider-4fc726341a88201d.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libzerotrie-b653e7ae5de0522b.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libicu_locale_core-5806e7e4d35ec235.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libwriteable-47d0d6397da34840.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/liblitemap-2893ffc44a900ee5.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_ast_pretty-a24d9c3e20ed2a18.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_lexer-0bba7350eea182d2.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libunicode_properties-52d94ef3376bfe9b.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libunicode_ident-382baee0f82ccd7a.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libitertools-bf4ea11b7fa77ec9.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libfluent_bundle-01720a030b338200.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libfluent_langneg-2686e77c1347c4e6.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libintl_pluralrules-c2c1f58c2bea131a.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libself_cell-52ff5b35974e15a7.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libintl_memoizer-e69da131c3d626ce.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtype_map-1c49d242d1bb2967.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libunic_langid-d67b512477fc89f9.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libunic_langid_macros-c57467a36683e7fc.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libunic_langid_impl-88df0b04fcad01de.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtinystr-efb5bc2f2d3d3e26.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libzerovec-3578fb191427bc09.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libyoke-7fe4bd95ba097861.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libzerofrom-88dcf12717bc781f.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libfluent_syntax-08a91f80ebc5b609.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libthiserror-6424d4f379e13100.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librand-3cbb469048c3a8ed.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librand_chacha-b95b3aa16f83ba2a.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libppv_lite86-abaa8559accb6582.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libzerocopy-2213c2a44490dc66.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librand_core-2fde3a8bc406a3e1.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_ast-c7268eb43232510d.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libmemchr-4064a3627ffa0769.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_ast_ir-e20b64b5ed81f191.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_literal_escaper-f37336268ee3b23d.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_span-f0d64fa97b8ec436.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libblake3-737f22238b497850.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libconstant_time_eq-7ce8095bafb146f9.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libarrayref-2bf7cda89717af8b.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libitoa-8b1ea998ba7de172.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libscoped_tls-de3ff4d767e313db.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libsha2-ce3fbe578b5b8c17.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libsha1-d5db9f8d11171ebb.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libcpufeatures-5a705b7f57c3a6ca.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libmd5-f641beaa78060f3b.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libdigest-1b5bb16cc1fa46f9.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libblock_buffer-a38fafcbc97bc32c.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libcrypto_common-cdc13f5a770e6553.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libgeneric_array-219812d45f7e8097.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtypenum-0c2eb2a54e020ad1.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libunicode_width-b4c98f7caea9f739.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_data_structures-af713b14ddaa3808.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libelsa-5df0c31f6ce86dea.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libstable_deref_trait-d39f234914cd9793.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_thread_pool-01a8640757ae17bd.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libcrossbeam_deque-202bca6d3ba3f709.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libcrossbeam_epoch-98b35e49bd5941ab.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libcrossbeam_utils-32379da0bd506234.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libstacker-a60ba100f7fa6c37.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libpsm-3f2cd231453a7349.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_arena-fbacb9c70b2996f1.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtempfile-54453b5decdff963.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libgetrandom-a646e9540d4818c1.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libfastrand-f63fa601d14da23d.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustix-c4d19040af32c0aa.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libbitflags-9e014b1123fbc2c7.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/liblinux_raw_sys-b71521572ec37c5f.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libarrayvec-c00b139ff037d5df.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libhashbrown-2d39c302656676b3.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libfoldhash-237879aa0e32cc5d.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libmeasureme-caeff02e367c0490.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_hash-07d284751732f7c3.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libperf_event_open_sys-cc77c0b282dbcf66.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libmemmap2-604f19046fb5854b.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_graphviz-2b055e06d47c2a7e.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libparking_lot-933549b50bdfea4c.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libparking_lot_core-9839867e5f17c088.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libcfg_if-c7013c86b925a2a9.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/liblock_api-c54af63b1cfbcc29.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libscopeguard-18cc73ded319d0e6.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libjobserver-413dbf714a8c973b.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/liblibc-781860311ba44a33.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtracing-f03260af29603705.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libpin_project_lite-91c00a15940256d2.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtracing_core-6754b5f066185191.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libonce_cell-c78a89c820d66559.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_hash-86d7f1fd37b5580a.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libeither-02faf9d2804e46da.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_index-c1ce9d328b2cab68.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_serialize-f4da5ed81396bbfc.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libindexmap-dc6f3ac63cb042ca.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libequivalent-b1078dc243ee9f8e.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libhashbrown-bfc14bb6461db5d4.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libthin_vec-a4e92cd33ce37e03.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_hashes-07ffcbc2ecfb840d.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_stable_hash-92f93d16442a6e44.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libsmallvec-4672270ee3dee627.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libena-896771ba289f4809.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/liblog-fdc909d0970d9b00.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libstd-09206de7e76388a0.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libpanic_unwind-d03e9b0258ff45d9.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libobject-c36a7a2939b20651.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libmemchr-138624c2ac9307c3.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libaddr2line-6c83a44ed31579dd.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libgimli-b20050716a26ef37.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libcfg_if-71ad5c29b4100ecc.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/librustc_demangle-aec14a4c1e2a3b19.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libstd_detect-1b48c577f545403b.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libhashbrown-5f8e35c0a13e7b56.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/librustc_std_workspace_alloc-5d27ff3687f5623d.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libminiz_oxide-be28337da21ec663.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libadler2-2f9457cbeac8bbec.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libunwind-48c5606188a45c7e.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/liblibc-708f1ae5be4e9f65.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/librustc_std_workspace_core-1604ce0146f4fdb2.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/liballoc-fc00bf8d8ef0afa8.rlib",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libcore-522e416bea6349b5.rlib",
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
            "/tmp/rustcKk4Xlj/raw-dylibs",
            "-B/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/bin/gcc-ld",
            "-B/build/rust/build/x86_64-unknown-linux-gnu/stage0/lib/rustlib/x86_64-unknown-linux-gnu/bin/gcc-ld",
            "-fuse-ld=lld",
            "-Wl,--eh-frame-hdr",
            "-Wl,-z,noexecstack",
            "-L",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/build/psm-a13502f6a7202f84/out",
            "-L",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/build/blake3-5854528706313af4/out",
            "-L",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/build/rustc_llvm-f56a5d785b59b940/out",
            "-L",
            "/build/rust/build/x86_64-unknown-linux-gnu/ci-llvm/lib",
            "-L",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib",
            "-o",
            "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_driver-23d51a7ae6381501.so",
            "-shared",
            "-Wl,-soname=librustc_driver-23d51a7ae6381501.so",
            "-Wl,-z,relro,-z,now",
            "-Wl,-O1",
            "-nodefaultlibs",
            "-Wl,-z,origin",
            "-Wl,-rpath,/../lib",
        ];
        let parsed = Args::parse_args(&args, None, false).unwrap();
        let link_args = build_link_args(&parsed).unwrap();
        assert_eq!(
            link_args,
            vec![
                "--hash-style=gnu",
                "--build-id",
                "--eh-frame-hdr",
                "-m",
                "elf_x86_64",
                "-shared",
                "-o",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_driver-23d51a7ae6381501.so",
                "/lib64/crti.o",
                "/usr/lib64/gcc/x86_64-pc-linux-gnu/15.2.1/crtbeginS.o",
                "-L/tmp/rustcKk4Xlj/raw-dylibs",
                "-L/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/build/psm-a13502f6a7202f84/out",
                "-L/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/build/blake3-5854528706313af4/out",
                "-L/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/build/rustc_llvm-f56a5d785b59b940/out",
                "-L/build/rust/build/x86_64-unknown-linux-gnu/ci-llvm/lib",
                "-L/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib",
                "-L/usr/lib64/gcc/x86_64-pc-linux-gnu/15.2.1",
                "-L/lib64",
                "-L/usr/lib64",
                "-L/lib",
                "-L/usr/lib",
                "--version-script=/tmp/rustcKk4Xlj/list",
                "--no-undefined-version",
                "/tmp/rustcKk4Xlj/symbols.o",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/rustc_driver-23d51a7ae6381501.rustc_driver.31a38ef3d8373027-cgu.0.rcgu.o",
                "/tmp/rustcKk4Xlj/rmeta.o",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/rustc_driver-23d51a7ae6381501.8tfkk5unuq490u6lq0gbd6v4f.rcgu.o",
                "--as-needed",
                "-Bstatic",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_driver_impl-bd6892f158872bb6.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libctrlc-d38327187ce07e0f.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libnix-64e0e3a9d1b0fb75.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_log-fd5732f26166c7fe.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtracing_tree-24892338644f0083.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtracing_log-149389d03d66ff20.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtracing_subscriber-1e07328309495aec.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libsharded_slab-d62d485df448ccff.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/liblazy_static-d5ea42230b171193.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libmatchers-36b0220694507dd4.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libnu_ansi_term-762f8b00e03c212a.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libthread_local-583f63a74e0b150a.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libjiff-91ff6f4390ae9016.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libshlex-99d3a341c9a86137.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_public-0f4554cd16bed968.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_public_bridge-72ce244932e44af6.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_interface-d8683ea701a8941c.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_codegen_llvm-c6d357baf2d6d382.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/liblibloading-508bc7b07167e85e.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_llvm-da5b440c7123b117.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_sanitizers-f5de2ba89027eebb.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_hir_typeck-ecce6f9a84d85e42.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_hir_analysis-cfad7a1533f4131c.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_monomorphize-c1580aeeb69cdb5f.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_mir_transform-55501824e5603848.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_mir_build-427d04da3a95e28c.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_pattern_analysis-128da25e2283fc1d.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_borrowck-c4b7282fe5427137.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_traits-025c5024dfb8a43c.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_const_eval-996a8ff580db355e.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_mir_dataflow-245764647131cca4.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_builtin_macros-6e3a732d3821cd40.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_resolve-e20e7d7c250811fc.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libpulldown_cmark-06374c9f5517d00f.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libunicase-56fec33e7d012112.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libpulldown_cmark_escape-b47006a888fa87ae.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_passes-7a2a58795be0375f.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_ast_lowering-97e8e8cf20f0806e.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_privacy-66e8b0a8f821461f.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_ty_utils-1a19758b38889e98.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_lint-d5baaba9f9ef3596.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libunicode_security-a0b1ddc091ba25d8.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libunicode_script-eff2a1d2bacad010.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_query_impl-b3e40b45a6c45c19.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_codegen_ssa-d9fff5c84497631e.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libwasm_encoder-474de55290d8d020.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libleb128-c0ff2e479cd49024.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libthorin-418d4b81a24ff317.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libgimli-239fef8f2e61be60.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libfallible_iterator-bb1cffb49b35862e.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libobject-aa38e621d939f62a.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libruzstd-0845499878594a3d.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtwox_hash-7a7641d3e02f2a06.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librand-bd46b4e082a763f6.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librand_chacha-b04ab6e22f51e3cf.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librand_core-d52df22c4d1e962d.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libgetrandom-6421c1509abca187.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libstatic_assertions-de6b6dffccb50005.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libflate2-d38efc1122a288e7.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libminiz_oxide-6e047e15977f5ad7.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libsimd_adler32-35feb0f6f77a7e75.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libbstr-e8e7727377b48c66.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_trait_selection-ac2cc0becda9034c.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_next_trait_solver-07a91be87b1962ab.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_parse_format-8cfcc7565f5c1dbe.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_transmute-4f3102ca9fc12c75.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_infer-200a9c83777301e9.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_symbol_mangling-91f934cae5c1229e.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_demangle-2aaa6b3b7c07692e.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libpunycode-c9621206b7d2ec90.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libpathdiff-e0c43cb181445ff1.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libregex-6866054342cd6db6.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libfind_msvc_tools-6826e0da5d6b9644.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libar_archive_writer-f48cc112ee6afb56.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_metadata-e2e7d3e380ea2581.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/liblibloading-74e12fb1faa2b597.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_incremental-48f9047151b993ed.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_expand-afb5ab7491ba40f4.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_ast_passes-17894f552798cb0b.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_proc_macro-dbb99f277c1cbb52.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_attr_parsing-a67ec3fe0a04ad84.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_parse-f90f19e6c0f97d2e.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libunicode_normalization-4e410ac1004a1776.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtinyvec-765c0201d2a38046.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtinyvec_macros-b5ce11dd7f9de8a4.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_middle-aeea4739c8bc49b9.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_apfloat-7e5d19dcb77b5f34.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libgsgdt-c162380bde251ede.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libpolonius_engine-fc9739920eff7841.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libdatafrog-7d91c110c8f87484.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_query_system-25e262e73ac192ba.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_type_ir-008cc4121b281123.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_session-1255b55c4c7774a1.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libgetopts-a2562a16ef7406e0.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_feature-e05101130450eb0c.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_hir_pretty-e2175b614aac9650.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_errors-c13a1da85a497b3d.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtermize-c9f1481965c0c8e0.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_error_codes-5663bb6db5e1e925.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libannotate_snippets-887de6400b2674c7.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libanstream-b310d562f99c823f.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libanstyle_query-829fea5d9999e51e.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libis_terminal_polyfill-c08748f86e33ebd6.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libanstyle-0572e7a3163b0305.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libcolorchoice-ba4a7cf58d3d1cf6.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libanstyle_parse-c4b2fa4175aeaab1.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libutf8parse-71ed97eb02e1c9e7.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_hir-2a1a42fd3019c390.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libodht-dcb0a8017fb070c3.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_lint_defs-8d7484a6ef493a31.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_hir_id-982c55964d3e2cf1.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_target-e70e6ff34c7356dd.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libserde_path_to_error-361fceb2d0243016.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libschemars-82d3f008dfe11f47.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libdyn_clone-a6e43562e70c7064.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libserde-c2054341d1c06b8e.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libref_cast-5df3ffc280115468.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libobject-d694ea94a992f5be.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libcrc32fast-a1dd814449c5f62a.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libwasmparser-d195adac3f6af65d.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_fs_util-5609f6ada267bf3c.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libserde_json-db06470bdba04427.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libryu-606419a09a27540b.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libserde_core-ab11963c982bae61.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_abi-53ea049bb63e1eb6.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librand_xoshiro-d8a81a649342ccc4.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_error_messages-9b3dfbfab159b64a.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_baked_icu_data-6880d557e391b2df.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libicu_list-d4ddcd99c9df1ab4.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libregex_automata-ab85046163f05cbe.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libaho_corasick-3e558d5aa4e8bf40.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libregex_syntax-aaed4e0c494ce1a8.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libicu_locale-8a042c5791987e22.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libicu_locale_data-c79827aa62e7a5d5.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libicu_collections-d2136bd6bc3eb411.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libpotential_utf-c988d67831482dd0.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libicu_provider-4fc726341a88201d.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libzerotrie-b653e7ae5de0522b.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libicu_locale_core-5806e7e4d35ec235.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libwriteable-47d0d6397da34840.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/liblitemap-2893ffc44a900ee5.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_ast_pretty-a24d9c3e20ed2a18.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_lexer-0bba7350eea182d2.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libunicode_properties-52d94ef3376bfe9b.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libunicode_ident-382baee0f82ccd7a.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libitertools-bf4ea11b7fa77ec9.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libfluent_bundle-01720a030b338200.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libfluent_langneg-2686e77c1347c4e6.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libintl_pluralrules-c2c1f58c2bea131a.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libself_cell-52ff5b35974e15a7.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libintl_memoizer-e69da131c3d626ce.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtype_map-1c49d242d1bb2967.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libunic_langid-d67b512477fc89f9.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libunic_langid_macros-c57467a36683e7fc.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libunic_langid_impl-88df0b04fcad01de.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtinystr-efb5bc2f2d3d3e26.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libzerovec-3578fb191427bc09.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libyoke-7fe4bd95ba097861.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libzerofrom-88dcf12717bc781f.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libfluent_syntax-08a91f80ebc5b609.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libthiserror-6424d4f379e13100.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librand-3cbb469048c3a8ed.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librand_chacha-b95b3aa16f83ba2a.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libppv_lite86-abaa8559accb6582.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libzerocopy-2213c2a44490dc66.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librand_core-2fde3a8bc406a3e1.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_ast-c7268eb43232510d.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libmemchr-4064a3627ffa0769.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_ast_ir-e20b64b5ed81f191.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_literal_escaper-f37336268ee3b23d.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_span-f0d64fa97b8ec436.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libblake3-737f22238b497850.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libconstant_time_eq-7ce8095bafb146f9.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libarrayref-2bf7cda89717af8b.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libitoa-8b1ea998ba7de172.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libscoped_tls-de3ff4d767e313db.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libsha2-ce3fbe578b5b8c17.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libsha1-d5db9f8d11171ebb.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libcpufeatures-5a705b7f57c3a6ca.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libmd5-f641beaa78060f3b.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libdigest-1b5bb16cc1fa46f9.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libblock_buffer-a38fafcbc97bc32c.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libcrypto_common-cdc13f5a770e6553.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libgeneric_array-219812d45f7e8097.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtypenum-0c2eb2a54e020ad1.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libunicode_width-b4c98f7caea9f739.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_data_structures-af713b14ddaa3808.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libelsa-5df0c31f6ce86dea.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libstable_deref_trait-d39f234914cd9793.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_thread_pool-01a8640757ae17bd.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libcrossbeam_deque-202bca6d3ba3f709.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libcrossbeam_epoch-98b35e49bd5941ab.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libcrossbeam_utils-32379da0bd506234.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libstacker-a60ba100f7fa6c37.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libpsm-3f2cd231453a7349.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_arena-fbacb9c70b2996f1.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtempfile-54453b5decdff963.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libgetrandom-a646e9540d4818c1.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libfastrand-f63fa601d14da23d.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustix-c4d19040af32c0aa.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libbitflags-9e014b1123fbc2c7.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/liblinux_raw_sys-b71521572ec37c5f.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libarrayvec-c00b139ff037d5df.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libhashbrown-2d39c302656676b3.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libfoldhash-237879aa0e32cc5d.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libmeasureme-caeff02e367c0490.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_hash-07d284751732f7c3.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libperf_event_open_sys-cc77c0b282dbcf66.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libmemmap2-604f19046fb5854b.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_graphviz-2b055e06d47c2a7e.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libparking_lot-933549b50bdfea4c.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libparking_lot_core-9839867e5f17c088.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libcfg_if-c7013c86b925a2a9.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/liblock_api-c54af63b1cfbcc29.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libscopeguard-18cc73ded319d0e6.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libjobserver-413dbf714a8c973b.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/liblibc-781860311ba44a33.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtracing-f03260af29603705.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libpin_project_lite-91c00a15940256d2.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libtracing_core-6754b5f066185191.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libonce_cell-c78a89c820d66559.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_hash-86d7f1fd37b5580a.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libeither-02faf9d2804e46da.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_index-c1ce9d328b2cab68.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_serialize-f4da5ed81396bbfc.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libindexmap-dc6f3ac63cb042ca.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libequivalent-b1078dc243ee9f8e.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libhashbrown-bfc14bb6461db5d4.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libthin_vec-a4e92cd33ce37e03.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_hashes-07ffcbc2ecfb840d.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/librustc_stable_hash-92f93d16442a6e44.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libsmallvec-4672270ee3dee627.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/libena-896771ba289f4809.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage1-rustc/x86_64-unknown-linux-gnu/release/deps/liblog-fdc909d0970d9b00.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libstd-09206de7e76388a0.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libpanic_unwind-d03e9b0258ff45d9.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libobject-c36a7a2939b20651.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libmemchr-138624c2ac9307c3.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libaddr2line-6c83a44ed31579dd.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libgimli-b20050716a26ef37.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libcfg_if-71ad5c29b4100ecc.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/librustc_demangle-aec14a4c1e2a3b19.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libstd_detect-1b48c577f545403b.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libhashbrown-5f8e35c0a13e7b56.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/librustc_std_workspace_alloc-5d27ff3687f5623d.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libminiz_oxide-be28337da21ec663.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libadler2-2f9457cbeac8bbec.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libunwind-48c5606188a45c7e.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/liblibc-708f1ae5be4e9f65.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/librustc_std_workspace_core-1604ce0146f4fdb2.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/liballoc-fc00bf8d8ef0afa8.rlib",
                "/build/rust/build/x86_64-unknown-linux-gnu/stage0-sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib/libcore-522e416bea6349b5.rlib",
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
                "-soname=librustc_driver-23d51a7ae6381501.so",
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
                "/lib64/crtn.o",
            ]
        )
    }
}
