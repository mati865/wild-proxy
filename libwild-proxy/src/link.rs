use crate::arg_parser::{DriverArgs, LinkerArgs, OutputKind};
use anyhow::bail;
use std::{
    collections::BTreeMap,
    fs::read_dir,
    path::{Path, PathBuf},
};

struct SystemLibraryPaths {
    crt1: Option<PathBuf>,
    crti: PathBuf,
    crtn: PathBuf,
    library_paths: Vec<&'static str>,
}

fn system_library_paths(args: &DriverArgs) -> anyhow::Result<SystemLibraryPaths> {
    // 535 │ glibc /usr/lib/Mcrt1.o
    // 536 │ glibc /usr/lib/Scrt1.o
    // 539 │ glibc /usr/lib/crt1.o
    // 540 │ glibc /usr/lib/crti.o
    // 541 │ glibc /usr/lib/crtn.o
    // 799 │ glibc /usr/lib/gcrt1.o
    // 804 │ glibc /usr/lib/grcrt1.o
    // 866 │ glibc /usr/lib/rcrt1.o
    // TODO: Handle others
    let crt1_name = match args.output_kind {
        OutputKind::DynamicPie => Some("Scrt1.o"),
        OutputKind::StaticPie => Some("rcrt1.o"),
        OutputKind::Dynamic | OutputKind::Static => Some("crt1.o"),
        OutputKind::SharedObject => None,
    };
    let crti_name = "crti.o";
    let crtn_name = "crtn.o";

    // TODO: figure out how to handle it
    let potential_paths = ["/usr/lib64", "/lib64", "/lib", "/usr/lib"];
    let Some(found_path) = potential_paths
        .iter()
        .map(Path::new)
        .find(|p| p.join(crti_name).exists())
    else {
        bail!("todo");
    };

    // TODO: avoid duplication?
    let crt1 = crt1_name.map(|name| found_path.join(name));
    let crti = found_path.join(crti_name);
    let crtn = found_path.join(crtn_name);

    Ok(SystemLibraryPaths {
        crt1,
        crti,
        crtn,
        library_paths: potential_paths.to_vec(),
    })
}

struct GccObjects {
    begin_object: PathBuf,
    end_object: PathBuf,
    lib_dir: PathBuf,
}

fn gcc_objects(args: &DriverArgs) -> anyhow::Result<GccObjects> {
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
    let end_object_name = "crtend.o";

    // TODO: Figure out the logic
    let potential_paths = ["/usr/lib64", "/usr/lib"];
    let Some(found_path) = potential_paths
        .iter()
        .map(Path::new)
        .map(|p| p.join("gcc").join("x86_64-pc-linux-gnu"))
        .find(|p| p.exists())
    else {
        bail!("todo")
    };

    // TODO: handle others
    // TODO: semver might be an overkill
    let mut gcc_versions = read_dir(&found_path)?
        .filter_map(|dir| dir.map(|dir| (dir.file_name(), dir.path())).ok())
        .filter_map(|(file_name, path)| {
            semver::Version::parse(&file_name.to_string_lossy())
                .ok()
                .map(|version| (version, path))
        })
        .collect::<BTreeMap<_, _>>();

    let lib_dir = gcc_versions.pop_last().unwrap().1;
    let begin_object = lib_dir.join(begin_object_name);
    let end_object = lib_dir.join(end_object_name);

    Ok(GccObjects {
        begin_object,
        end_object,
        lib_dir,
    })
}

pub(crate) fn link(
    linker_args: LinkerArgs,
    driver_args: DriverArgs,
    cpp_mode: bool,
) -> anyhow::Result<()> {
    let system_library_paths = system_library_paths(&driver_args)?;
    let gcc_objects = gcc_objects(&driver_args)?;
    // Based on Clang
    let builtin_args1 = [
        "--hash-style=gnu",
        "--build-id",
        "--eh-frame-hdr",
        "-m",
        "elf_x86_64",
    ];
    let static_system_libs = ["-lgcc", "-lgcc_eh", "-lc"];
    let shared_system_libs = ["-lgcc", "--as-needed", "-lgcc_s", "--no-as-needed", "-lc"];
    let output_kind_args: &[&str] = match driver_args.output_kind {
        OutputKind::DynamicPie => &["-pie", "--dynamic-linker", "/lib64/ld-linux-x86-64.so.2"],
        OutputKind::StaticPie => &["-static", "-pie"],
        OutputKind::Dynamic => &["--dynamic-linker", "/lib64/ld-linux-x86-64.so.2"],
        OutputKind::Static => &["-static"],
        OutputKind::SharedObject => &["-shared"],
    };

    let mut final_linker_args = Vec::from_iter(builtin_args1.iter().map(ToString::to_string));
    final_linker_args.extend(output_kind_args.iter().map(ToString::to_string));
    final_linker_args.extend(["-o".to_string(), driver_args.output]);
    if let Some(crt1) = system_library_paths.crt1 {
        final_linker_args.push(crt1.display().to_string());
    }
    final_linker_args.push(system_library_paths.crti.display().to_string());
    final_linker_args.push(gcc_objects.begin_object.display().to_string());
    final_linker_args.push(format!("-L{}", gcc_objects.lib_dir.display()));
    for path in system_library_paths.library_paths {
        final_linker_args.push(format!("-L{}", path));
    }
    final_linker_args.extend(driver_args.objects_and_libs);
    final_linker_args.extend(linker_args.iter().map(ToString::to_string));
    if driver_args.default_libs {
        if cpp_mode {
            final_linker_args.extend(["-lstdc++".to_string(), "-lm".to_string()]);
        }
        if driver_args.output_kind == OutputKind::Static
            || driver_args.output_kind == OutputKind::StaticPie
        {
            final_linker_args.extend(static_system_libs.iter().map(ToString::to_string));
        } else {
            final_linker_args.extend(shared_system_libs.iter().map(ToString::to_string));
        }
    }
    final_linker_args.push(gcc_objects.end_object.display().to_string());
    final_linker_args.push(system_library_paths.crtn.display().to_string());

    let wild_args = libwild::Args::parse(|| final_linker_args.iter()).expect("todo");
    unsafe { libwild::run_in_subprocess(wild_args) }
}
