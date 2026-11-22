use crate::args::{Args, OutputKind};
use anyhow::{Result, anyhow, bail};
use std::{
    collections::BTreeMap,
    fs::read_dir,
    path::{Path, PathBuf},
};

pub(crate) struct SystemLibraryPaths {
    pub(crate) crt1: Option<PathBuf>,
    pub(crate) crti: PathBuf,
    pub(crate) crtn: PathBuf,
    pub(crate) library_paths: Vec<String>,
}

fn potential_library_paths(args: &Args, gcc_libs_path: &Path) -> Vec<String> {
    if let Some(path) = &args.sysroot
        && path != "/"
    {
        let trimmed = path.trim_end_matches("/");
        vec![
            format!("{}/../../../../lib64", gcc_libs_path.display()),
            format!("{trimmed}/../lib64"),
            format!("{trimmed}/lib"),
        ]
    } else {
        vec![
            format!("{}/../../../../lib64", gcc_libs_path.display()),
            // This needs to be relative not to break Wild's WILD_SAVE_DIR
            "/lib/../lib64".to_string(),
            "/usr/lib64".to_string(),
            "/lib".to_string(),
            "/usr/lib".to_string(),
        ]
    }
}

fn system_library_paths(args: &Args, gcc_libs_path: &Path) -> Result<SystemLibraryPaths> {
    let potential_paths = potential_library_paths(args, gcc_libs_path);
    let Some(found_path) = potential_paths
        .iter()
        .map(Path::new)
        .find(|p| p.join("crti.o").exists())
    else {
        bail!("todo");
    };
    Ok(system_library_paths_at(args, gcc_libs_path, found_path))
}

fn system_library_paths_at(
    args: &Args,
    gcc_libs_path: &Path,
    found_path: &Path,
) -> SystemLibraryPaths {
    // 535 │ glibc /usr/lib/Mcrt1.o
    // 536 │ glibc /usr/lib/Scrt1.o
    // 539 │ glibc /usr/lib/crt1.o
    // 540 │ glibc /usr/lib/crti.o
    // 541 │ glibc /usr/lib/crtn.o
    // 799 │ glibc /usr/lib/gcrt1.o
    // 804 │ glibc /usr/lib/grcrt1.o
    // 866 │ glibc /usr/lib/rcrt1.o
    // TODO: Handle others
    let crt1_name = if args.profile {
        if args.output_kind == OutputKind::StaticPie {
            Some("grcrt1.o")
        } else {
            Some("gcrt1.o")
        }
    } else {
        match args.output_kind {
            OutputKind::DynamicPie => Some("Scrt1.o"),
            OutputKind::StaticPie => Some("rcrt1.o"),
            OutputKind::Dynamic | OutputKind::Static => Some("crt1.o"),
            OutputKind::SharedObject => None,
        }
    };
    let crti_name = "crti.o";
    let crtn_name = "crtn.o";

    // TODO: avoid duplication?
    let crt1 = crt1_name.map(|name| found_path.join(name));
    let crti = found_path.join(crti_name);
    let crtn = found_path.join(crtn_name);

    SystemLibraryPaths {
        crt1,
        crti,
        crtn,
        library_paths: potential_library_paths(args, gcc_libs_path),
    }
}

pub(crate) struct GccObjects {
    pub(crate) begin_object: PathBuf,
    pub(crate) end_object: PathBuf,
    pub(crate) lib_dir: PathBuf,
}

fn gcc_objects(args: &Args) -> Result<GccObjects> {
    // TODO: Figure out the logic
    // TODO: handle other distros like RHEL and *SUSE
    let potential_paths = ["/usr/lib64", "/usr/lib"];
    let Some(found_path) = potential_paths
        .iter()
        .map(Path::new)
        .flat_map(|p| {
            let arch = args.arch;
            [
                p.join("gcc").join(format!("{arch}-pc-linux-gnu")),
                p.join("gcc").join(format!("{arch}-linux-gnu")),
            ]
        })
        .find(|p| p.exists())
    else {
        bail!("todo")
    };

    let mut gcc_versions = read_dir(&found_path)?
        .filter_map(|dir| dir.map(|dir| (dir.file_name(), dir.path())).ok())
        .filter_map(|(file_name, path)| {
            let mut version_parts = file_name.to_str()?.splitn(3, '.');
            let major: u8 = version_parts.next().unwrap().parse().unwrap();
            let minor: u8 = version_parts
                .next()
                .map(|str| str.parse().expect("GCC version components are numbers"))
                .unwrap_or(0);
            let patch: u8 = version_parts
                .next()
                .map(|str| str.parse().expect("GCC version components are numbers"))
                .unwrap_or(0);
            Some(((major, minor, patch), path))
        })
        .collect::<BTreeMap<_, _>>();
    assert!(!gcc_versions.is_empty(), "Could not find GCC directory");
    let lib_dir = gcc_versions.pop_last().unwrap().1;

    Ok(gcc_objects_at(args, lib_dir))
}

fn gcc_objects_at(args: &Args, lib_dir: PathBuf) -> GccObjects {
    // 952 │ gcc /usr/lib/gcc/x86_64-pc-linux-gnu/15.2.1/crtbegin.o
    // 953 │ gcc /usr/lib/gcc/x86_64-pc-linux-gnu/15.2.1/crtbeginS.o
    // 954 │ gcc /usr/lib/gcc/x86_64-pc-linux-gnu/15.2.1/crtbeginT.o
    // 955 │ gcc /usr/lib/gcc/x86_64-pc-linux-gnu/15.2.1/crtend.o
    // 956 │ gcc /usr/lib/gcc/x86_64-pc-linux-gnu/15.2.1/crtendS.o
    // 957 │ gcc /usr/lib/gcc/x86_64-pc-linux-gnu/15.2.1/crtfastmath.o
    // 958 │ gcc /usr/lib/gcc/x86_64-pc-linux-gnu/15.2.1/crtprec32.o
    // 959 │ gcc /usr/lib/gcc/x86_64-pc-linux-gnu/15.2.1/crtprec64.o
    // 960 │ gcc /usr/lib/gcc/x86_64-pc-linux-gnu/15.2.1/crtprec80.o
    // TODO: Handle others
    let begin_object_name = match args.output_kind {
        OutputKind::DynamicPie | OutputKind::StaticPie | OutputKind::SharedObject => "crtbeginS.o",
        OutputKind::Dynamic => "crtbegin.o",
        OutputKind::Static => "crtbeginT.o",
    };
    let end_object_name = match args.output_kind {
        OutputKind::DynamicPie | OutputKind::StaticPie | OutputKind::SharedObject => "crtendS.o",
        _ => "crtend.o",
    };

    GccObjects {
        begin_object: lib_dir.join(begin_object_name),
        end_object: lib_dir.join(end_object_name),
        lib_dir,
    }
}

pub(crate) struct HostPaths {
    pub(crate) gcc_objects: GccObjects,
    pub(crate) system_library_paths: SystemLibraryPaths,
}

impl HostPaths {
    fn discover(args: &Args) -> Result<HostPaths> {
        let gcc_objects = gcc_objects(args)?;
        let system_library_paths = system_library_paths(args, &gcc_objects.lib_dir)?;
        Ok(HostPaths {
            gcc_objects,
            system_library_paths,
        })
    }

    #[cfg(test)]
    pub(crate) fn mock(args: &Args) -> HostPaths {
        let lib_dir = PathBuf::from("/usr/lib64/gcc/x86_64-pc-linux-gnu/16.1.1");
        let crt_dir = lib_dir.join("../../../../lib64");
        HostPaths {
            gcc_objects: gcc_objects_at(args, lib_dir.clone()),
            system_library_paths: system_library_paths_at(args, &lib_dir, &crt_dir),
        }
    }
}

pub(crate) fn link(args: &Args) -> Result<()> {
    let linker_args = build_link_args(args, &HostPaths::discover(args)?);

    if args.hash_hash_hash || args.verbose {
        eprintln!(" wild {}", linker_args.join(" "));
    }
    if !args.hash_hash_hash {
        let mut wild_args = libwild::Args::new(|| linker_args.iter())
            .map_err(|e| anyhow!("Wild args parse error: {}", e.to_string()))?;
        wild_args
            .parse(|| linker_args.iter())
            .map_err(|e| anyhow::anyhow!("Wild args parse error: {}", e.to_string()))?;
        // TODO: Do forking
        // unsafe { libwild::run_in_subprocess(wild_args) }
        if let Err(e) = libwild::run(wild_args) {
            bail!("Wild run error: {}", e.to_string());
        };
    }

    Ok(())
}

pub(crate) fn build_link_args(args: &Args, host_paths: &HostPaths) -> Vec<String> {
    let gcc_objects = &host_paths.gcc_objects;
    let system_library_paths = &host_paths.system_library_paths;
    // Based on Clang
    let builtin_args1 = [
        "--hash-style=gnu",
        "--build-id",
        "--eh-frame-hdr",
        "-m",
        args.arch.emulation(),
    ];
    let output_kind_args: &[&str] = match args.output_kind {
        OutputKind::DynamicPie => &["-pie", "--dynamic-linker", args.arch.dynamic_linker()],
        OutputKind::StaticPie => &["-static", "-pie"],
        OutputKind::Dynamic => &["--dynamic-linker", args.arch.dynamic_linker()],
        OutputKind::Static => &["-static"],
        OutputKind::SharedObject => &["-shared"],
    };

    let mut final_linker_args = vec!["wild".to_string()];
    if let Some(sysroot) = &args.sysroot {
        final_linker_args.push(format!("--sysroot={sysroot}"));
    };
    final_linker_args.extend(builtin_args1.iter().map(ToString::to_string));
    if args.rdynamic {
        // TODO: add test
        final_linker_args.push("--export-dynamic".to_string());
    }
    final_linker_args.extend(output_kind_args.iter().map(ToString::to_string));
    final_linker_args.extend(["-o".to_string(), args.output.to_string()]);

    // TODO: Add `--sysroot` when not running natively, Clang doesn't do it.

    if let Some(crt1) = &system_library_paths.crt1 {
        final_linker_args.push(crt1.display().to_string());
    }
    if !args.nostdlib {
        final_linker_args.push(system_library_paths.crti.display().to_string());
        final_linker_args.push(gcc_objects.begin_object.display().to_string());
    }
    final_linker_args.extend(
        args.additional_search_paths
            .iter()
            .map(|path| format!("-L{}", path)),
    );
    final_linker_args.push(format!("-L{}", gcc_objects.lib_dir.display()));
    for path in &system_library_paths.library_paths {
        final_linker_args.push(format!("-L{}", path));
    }
    final_linker_args.extend(args.raw_linker_args.clone().into_iter());
    if !args.nodefaultlibs && !args.nostdlib && !args.nostdlibxx {
        if args.cpp_mode {
            final_linker_args.extend(["-lstdc++".to_string(), "-lm".to_string()]);
        }
    }
    if args.openmp {
        final_linker_args.push("-lgomp".to_string());
    }
    if args.coverage {
        final_linker_args.push("-lgcov".to_string());
    }
    if !args.nodefaultlibs && !args.nostdlib {
        let system_libs = if args.output_kind == OutputKind::Static
            || args.output_kind == OutputKind::StaticPie
        {
            let mut system_libs = vec!["-lgcc", "-lgcc_eh"];
            if !args.nolibc {
                system_libs.push("-lc")
            }
            system_libs
        } else {
            let mut system_libs = vec!["-lgcc", "--as-needed", "-lgcc_s"];
            if !args.nolibc {
                system_libs.extend(&["--no-as-needed", "-lc"])
            }
            system_libs
        };
        final_linker_args.extend(system_libs.iter().map(ToString::to_string));
    }
    if args.pthread {
        final_linker_args.push("-lpthread".to_string());
    }
    if !args.nostartfiles && !args.nostdlib {
        final_linker_args.push(gcc_objects.end_object.display().to_string());
        final_linker_args.push(system_library_paths.crtn.display().to_string());
    }
    final_linker_args.extend(
        args.scripts
            .iter()
            .flat_map(|script| ["-T".to_string(), script.to_string()]),
    );
    final_linker_args
}

// World:
// Gnome: dev-lang/spidermonkey net-misc/networkmanager net-libs/webkit-gtk
