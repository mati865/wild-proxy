use std::{fs::remove_file, path::PathBuf, process::Command, str::Lines};

use anyhow::{Context, Result, bail};

// TODOs:
// - Implement the TODOs
// - Better error handling
// - Move fallback to a separate module
// - Implement proper solution and use fallback as a fallback

/// Fallback and ask the OG linker if we cannot figure it out ourselves
pub fn fallback() -> Result<()> {
    let mut files_to_delete = Vec::with_capacity(2);
    let mut exe_with_args = std::env::args();
    let zero_position_arg = exe_with_args
        .next()
        .context("Could not obtain binary name from args")?;
    let args = exe_with_args.collect::<Vec<_>>();
    let binary = parse_linker_name(&zero_position_arg)?;

    if args.contains(&"--help".to_string()) {
        Command::new(binary).arg("--help").status()?;
        return Ok(());
    } else if args.contains(&"--version".to_string()) {
        Command::new(binary).arg("--version").status()?;
        return Ok(());
    } else if args.contains(&"-###".to_string()) {
        Command::new(binary).arg("-###").status()?;
        return Ok(());
    }

    let raw_dump = String::from_utf8(
        Command::new(binary)
            .args(&args)
            .arg("-###")
            .output()
            .with_context(|| format!("Failed to run {binary}"))?
            .stderr,
    )?;

    let commands = obtain_whole_command(raw_dump.lines())?;
    for command in commands.build_and_assemble.into_iter() {
        let args = shellwords::split(command)?;
        Command::new(args.first().unwrap())
            .args(&args[1..])
            .status()?;

        let mut args_iter = args.iter().skip(1);
        args_iter.find(|arg| *arg == "-o").unwrap();
        files_to_delete.push(PathBuf::from(args_iter.next().unwrap()));
    }

    // Delete this file only after linker is done, or leave if we are not linking (like when `-c` is passed)
    let final_compiler_output = files_to_delete.pop();

    if let Some(command) = commands.link {
        let wild_args =
            libwild::Args::parse(command.split(' ').skip(1).map(|arg| arg.trim_matches('"')))?;
        // Need to cleanup temp files
        // unsafe { libwild::run_in_subprocess(&wild_args) }
        libwild::run(&wild_args)?;
        if let Some(file) = final_compiler_output {
            remove_file(&file).with_context(|| format!("Failed to delete {}", file.display()))?;
        }
    }

    for file in files_to_delete {
        remove_file(&file).with_context(|| format!("Failed to delete {}", file.display()))?;
    }

    Ok(())
}

fn parse_linker_name(zero_position_arg: &str) -> Result<&str> {
    const NEEDLE: &str = "wild-";
    let needle_position = zero_position_arg.rfind(NEEDLE).context("todo rfind");
    let trimmed = needle_position.map(|pos| &zero_position_arg[pos + NEEDLE.len()..])?;
    let binary = match trimmed {
        valid_command @ ("cc" | "c++" | "gcc" | "g++" | "clang" | "clang++") => valid_command,
        _ => bail!(
            "Argument at zero position must follow pattern: `wild-<command>` where command is one of: cc,c++,gcc,g++,clang,clang++\ngot: {zero_position_arg}"
        ),
    };

    Ok(binary)
}

#[derive(Debug, PartialEq, Eq)]
struct Commands<'a> {
    build_and_assemble: Vec<&'a str>,
    link: Option<&'a str>,
}

fn obtain_whole_command(mut dumped_lines: Lines) -> Result<Commands> {
    let first_real_line = loop {
        if let Some(line) = dumped_lines.next() {
            if !line.trim().is_empty() {
                break line;
            }
        } else {
            bail!("No more lines");
        }
    };
    let commands = if first_real_line.starts_with("clang") {
        parse_clang(dumped_lines)
    } else {
        parse_gcc(dumped_lines)
    };

    commands
}

fn parse_clang(dumped_lines: Lines) -> Result<Commands> {
    let commands = dumped_lines
        .filter_map(|line| {
            (line.starts_with(' ') && !line.ends_with("(in-process)"))
                .then(|| line.trim())
                .filter(|trimmed| !trimmed.is_empty())
        })
        .collect::<Vec<_>>();

    let commands = Commands {
        build_and_assemble: vec![commands[0]],
        link: commands.get(1).copied(),
    };

    Ok(commands)
}

fn parse_gcc(dumped_lines: Lines) -> Result<Commands> {
    let commands = dumped_lines
        .filter_map(|line| {
            line.starts_with(' ')
                .then(|| line.trim())
                .filter(|trimmed| !trimmed.is_empty())
        })
        .collect::<Vec<_>>();

    let commands = Commands {
        build_and_assemble: commands[..=1].to_vec(),
        link: commands.get(2).copied(),
    };

    Ok(commands)
}

#[cfg(test)]
mod test {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_clang() {
        let input = r#"
clang version 19.1.7
Target: x86_64-pc-linux-gnu
Thread model: posix
InstalledDir: /usr/bin
    "/usr/bin/clang++" "-cc1" "-triple" "x86_64-pc-linux-gnu" "-emit-obj" "-dumpdir" "a-" "-disable-free" "-clear-ast-before-backend" "-disable-llvm-verifier" "-discard-value-names" "-main-file-name" "hello.cpp" "-mrelocation-model" "pic" "-pic-level" "2" "-pic-is-pie" "-mframe-pointer=all" "-fmath-errno" "-ffp-contract=on" "-fno-rounding-math" "-mconstructor-aliases" "-funwind-tables=2" "-target-cpu" "x86-64" "-tune-cpu" "generic" "-debugger-tuning=gdb" "-fdebug-compilation-dir=/tmp" "-fcoverage-compilation-dir=/tmp" "-resource-dir" "/usr/lib/clang/19" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../include/c++/14.2.1" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../include/c++/14.2.1/x86_64-pc-linux-gnu" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../include/c++/14.2.1/backward" "-internal-isystem" "/usr/lib/clang/19/include" "-internal-isystem" "/usr/local/include" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../x86_64-pc-linux-gnu/include" "-internal-externc-isystem" "/include" "-internal-externc-isystem" "/usr/include" "-fdeprecated-macro" "-ferror-limit" "19" "-stack-protector" "2" "-fgnuc-version=4.2.1" "-fskip-odr-check-in-gmf" "-fcxx-exceptions" "-fexceptions" "-faddrsig" "-D__GCC_HAVE_DWARF2_CFI_ASM=1" "-o" "/tmp/hello-5bcb74.o" "-x" "c++" "hello.cpp"
    "/usr/bin/ld" "--hash-style=gnu" "--build-id" "--eh-frame-hdr" "-m" "elf_x86_64" "-pie" "-dynamic-linker" "/lib64/ld-linux-x86-64.so.2" "-o" "a.out" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib64/Scrt1.o" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib64/crti.o" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/crtbeginS.o" "-L/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1" "-L/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib64" "-L/lib/../lib64" "-L/usr/lib/../lib64" "-L/lib" "-L/usr/lib" "/tmp/hello-5bcb74.o" "-lstdc++" "-lm" "-lgcc_s" "-lgcc" "-lc" "-lgcc_s" "-lgcc" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/crtendS.o" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib64/crtn.o"
            "#;
        let expected = Commands {
            build_and_assemble: vec![
                r#""/usr/bin/clang++" "-cc1" "-triple" "x86_64-pc-linux-gnu" "-emit-obj" "-dumpdir" "a-" "-disable-free" "-clear-ast-before-backend" "-disable-llvm-verifier" "-discard-value-names" "-main-file-name" "hello.cpp" "-mrelocation-model" "pic" "-pic-level" "2" "-pic-is-pie" "-mframe-pointer=all" "-fmath-errno" "-ffp-contract=on" "-fno-rounding-math" "-mconstructor-aliases" "-funwind-tables=2" "-target-cpu" "x86-64" "-tune-cpu" "generic" "-debugger-tuning=gdb" "-fdebug-compilation-dir=/tmp" "-fcoverage-compilation-dir=/tmp" "-resource-dir" "/usr/lib/clang/19" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../include/c++/14.2.1" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../include/c++/14.2.1/x86_64-pc-linux-gnu" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../include/c++/14.2.1/backward" "-internal-isystem" "/usr/lib/clang/19/include" "-internal-isystem" "/usr/local/include" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../x86_64-pc-linux-gnu/include" "-internal-externc-isystem" "/include" "-internal-externc-isystem" "/usr/include" "-fdeprecated-macro" "-ferror-limit" "19" "-stack-protector" "2" "-fgnuc-version=4.2.1" "-fskip-odr-check-in-gmf" "-fcxx-exceptions" "-fexceptions" "-faddrsig" "-D__GCC_HAVE_DWARF2_CFI_ASM=1" "-o" "/tmp/hello-5bcb74.o" "-x" "c++" "hello.cpp""#,
            ],
            link: Some(
                r#""/usr/bin/ld" "--hash-style=gnu" "--build-id" "--eh-frame-hdr" "-m" "elf_x86_64" "-pie" "-dynamic-linker" "/lib64/ld-linux-x86-64.so.2" "-o" "a.out" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib64/Scrt1.o" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib64/crti.o" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/crtbeginS.o" "-L/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1" "-L/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib64" "-L/lib/../lib64" "-L/usr/lib/../lib64" "-L/lib" "-L/usr/lib" "/tmp/hello-5bcb74.o" "-lstdc++" "-lm" "-lgcc_s" "-lgcc" "-lc" "-lgcc_s" "-lgcc" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/crtendS.o" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib64/crtn.o""#,
            ),
        };
        assert_eq!(expected, obtain_whole_command(input.lines()).unwrap());
    }

    #[test]
    fn parse_clang_without_link() {
        let input = r#"
clang version 19.1.7
Target: x86_64-pc-linux-gnu
Thread model: posix
InstalledDir: /usr/bin
    (in-process)
    "/usr/bin/clang++" "-cc1" "-triple" "x86_64-pc-linux-gnu" "-emit-obj" "-disable-free" "-clear-ast-before-backend" "-disable-llvm-verifier" "-discard-value-names" "-main-file-name" "hello.cpp" "-mrelocation-model" "pic" "-pic-level" "2" "-pic-is-pie" "-mframe-pointer=all" "-fmath-errno" "-ffp-contract=on" "-fno-rounding-math" "-mconstructor-aliases" "-funwind-tables=2" "-target-cpu" "x86-64" "-tune-cpu" "generic" "-debugger-tuning=gdb" "-fdebug-compilation-dir=/tmp" "-fcoverage-compilation-dir=/tmp" "-resource-dir" "/usr/lib/clang/19" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../include/c++/14.2.1" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../include/c++/14.2.1/x86_64-pc-linux-gnu" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../include/c++/14.2.1/backward" "-internal-isystem" "/usr/lib/clang/19/include" "-internal-isystem" "/usr/local/include" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../x86_64-pc-linux-gnu/include" "-internal-externc-isystem" "/include" "-internal-externc-isystem" "/usr/include" "-fdeprecated-macro" "-ferror-limit" "19" "-stack-protector" "2" "-fgnuc-version=4.2.1" "-fskip-odr-check-in-gmf" "-fcxx-exceptions" "-fexceptions" "-faddrsig" "-D__GCC_HAVE_DWARF2_CFI_ASM=1" "-o" "hello.o" "-x" "c++" "hello.cpp"
            "#;
        let expected = Commands {
            build_and_assemble: vec![
                r#""/usr/bin/clang++" "-cc1" "-triple" "x86_64-pc-linux-gnu" "-emit-obj" "-disable-free" "-clear-ast-before-backend" "-disable-llvm-verifier" "-discard-value-names" "-main-file-name" "hello.cpp" "-mrelocation-model" "pic" "-pic-level" "2" "-pic-is-pie" "-mframe-pointer=all" "-fmath-errno" "-ffp-contract=on" "-fno-rounding-math" "-mconstructor-aliases" "-funwind-tables=2" "-target-cpu" "x86-64" "-tune-cpu" "generic" "-debugger-tuning=gdb" "-fdebug-compilation-dir=/tmp" "-fcoverage-compilation-dir=/tmp" "-resource-dir" "/usr/lib/clang/19" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../include/c++/14.2.1" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../include/c++/14.2.1/x86_64-pc-linux-gnu" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../include/c++/14.2.1/backward" "-internal-isystem" "/usr/lib/clang/19/include" "-internal-isystem" "/usr/local/include" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../x86_64-pc-linux-gnu/include" "-internal-externc-isystem" "/include" "-internal-externc-isystem" "/usr/include" "-fdeprecated-macro" "-ferror-limit" "19" "-stack-protector" "2" "-fgnuc-version=4.2.1" "-fskip-odr-check-in-gmf" "-fcxx-exceptions" "-fexceptions" "-faddrsig" "-D__GCC_HAVE_DWARF2_CFI_ASM=1" "-o" "hello.o" "-x" "c++" "hello.cpp""#,
            ],
            link: None,
        };
        assert_eq!(expected, obtain_whole_command(input.lines()).unwrap());
    }

    #[test]
    fn parse_gcc() {
        let input = r#"
Using built-in specs.
COLLECT_GCC=g++
COLLECT_LTO_WRAPPER=/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/lto-wrapper
Target: x86_64-pc-linux-gnu
Configured with: /tmp/pkg/src/gcc/configure --enable-languages=ada,c,c++,d,fortran,go,lto,m2,objc,obj-c++,rust --enable-bootstrap --prefix=/usr --libdir=/usr/lib --libexecdir=/usr/lib --mandir=/usr/share/man --infodir=/usr/share/info --with-bugurl=https://github.com/CachyOS/CachyOS-PKGBUILDS/issues --with-build-config=bootstrap-lto --with-linker-hash-style=gnu --with-system-zlib --enable-__cxa_atexit --enable-cet=auto --enable-checking=release --enable-clocale=gnu --enable-default-pie --enable-default-ssp --enable-gnu-indirect-function --enable-gnu-unique-object --enable-libstdcxx-backtrace --enable-link-serialization=1 --enable-linker-build-id --enable-lto --enable-multilib --enable-plugin --enable-shared --enable-threads=posix --disable-libssp --disable-libstdcxx-pch --disable-werror
Thread model: posix
Supported LTO compression algorithms: zlib zstd
gcc version 14.2.1 20250207 (GCC)
COLLECT_GCC_OPTIONS='-shared-libgcc' '-mtune=generic' '-march=x86-64' '-dumpdir' 'a-'
    /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/cc1plus -quiet -D_GNU_SOURCE hello.cpp -quiet -dumpdir a- -dumpbase hello.cpp -dumpbase-ext .cpp "-mtune=generic" "-march=x86-64" -o /tmp/ccxGHCn4.s
COLLECT_GCC_OPTIONS='-shared-libgcc' '-mtune=generic' '-march=x86-64' '-dumpdir' 'a-'
    as --64 -o /tmp/ccql7Oad.o /tmp/ccxGHCn4.s
COMPILER_PATH=/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/
LIBRARY_PATH=/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib/:/lib/../lib/:/usr/lib/../lib/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../:/lib/:/usr/lib/
COLLECT_GCC_OPTIONS='-shared-libgcc' '-mtune=generic' '-march=x86-64' '-dumpdir' 'a.'
    /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/collect2 -plugin /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/liblto_plugin.so "-plugin-opt=/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/lto-wrapper" "-plugin-opt=-fresolution=/tmp/ccIkCFvS.res" "-plugin-opt=-pass-through=-lgcc_s" "-plugin-opt=-pass-through=-lgcc" "-plugin-opt=-pass-through=-lc" "-plugin-opt=-pass-through=-lgcc_s" "-plugin-opt=-pass-through=-lgcc" --build-id --eh-frame-hdr "--hash-style=gnu" -m elf_x86_64 -dynamic-linker /lib64/ld-linux-x86-64.so.2 -pie /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib/Scrt1.o /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib/crti.o /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/crtbeginS.o -L/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1 -L/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib -L/lib/../lib -L/usr/lib/../lib -L/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../.. /tmp/ccql7Oad.o "-lstdc++" -lm -lgcc_s -lgcc -lc -lgcc_s -lgcc /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/crtendS.o /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib/crtn.o
COLLECT_GCC_OPTIONS='-shared-libgcc' '-mtune=generic' '-march=x86-64' '-dumpdir' 'a.'
            "#;
        let expected = Commands {
            build_and_assemble: vec![
                r#"/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/cc1plus -quiet -D_GNU_SOURCE hello.cpp -quiet -dumpdir a- -dumpbase hello.cpp -dumpbase-ext .cpp "-mtune=generic" "-march=x86-64" -o /tmp/ccxGHCn4.s"#,
                r#"as --64 -o /tmp/ccql7Oad.o /tmp/ccxGHCn4.s"#,
            ],
            link: Some(
                r#"/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/collect2 -plugin /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/liblto_plugin.so "-plugin-opt=/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/lto-wrapper" "-plugin-opt=-fresolution=/tmp/ccIkCFvS.res" "-plugin-opt=-pass-through=-lgcc_s" "-plugin-opt=-pass-through=-lgcc" "-plugin-opt=-pass-through=-lc" "-plugin-opt=-pass-through=-lgcc_s" "-plugin-opt=-pass-through=-lgcc" --build-id --eh-frame-hdr "--hash-style=gnu" -m elf_x86_64 -dynamic-linker /lib64/ld-linux-x86-64.so.2 -pie /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib/Scrt1.o /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib/crti.o /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/crtbeginS.o -L/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1 -L/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib -L/lib/../lib -L/usr/lib/../lib -L/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../.. /tmp/ccql7Oad.o "-lstdc++" -lm -lgcc_s -lgcc -lc -lgcc_s -lgcc /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/crtendS.o /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib/crtn.o"#,
            ),
        };
        assert_eq!(expected, obtain_whole_command(input.lines()).unwrap());
    }

    #[test]
    fn parse_gcc_without_link() {
        let input = r#"
Using built-in specs.
COLLECT_GCC=g++
Target: x86_64-pc-linux-gnu
Configured with: /tmp/pkg/src/gcc/configure --enable-languages=ada,c,c++,d,fortran,go,lto,m2,objc,obj-c++,rust --enable-bootstrap --prefix=/usr --libdir=/usr/lib --libexecdir=/usr/lib --mandir=/usr/share/man --infodir=/usr/share/info --with-bugurl=https://github.com/CachyOS/CachyOS-PKGBUILDS/issues --with-build-config=bootstrap-lto --with-linker-hash-style=gnu --with-system-zlib --enable-__cxa_atexit --enable-cet=auto --enable-checking=release --enable-clocale=gnu --enable-default-pie --enable-default-ssp --enable-gnu-indirect-function --enable-gnu-unique-object --enable-libstdcxx-backtrace --enable-link-serialization=1 --enable-linker-build-id --enable-lto --enable-multilib --enable-plugin --enable-shared --enable-threads=posix --disable-libssp --disable-libstdcxx-pch --disable-werror
Thread model: posix
Supported LTO compression algorithms: zlib zstd
gcc version 14.2.1 20250207 (GCC)
COLLECT_GCC_OPTIONS='-c' '-shared-libgcc' '-mtune=generic' '-march=x86-64'
    /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/cc1plus -quiet -D_GNU_SOURCE hello.cpp -quiet -dumpbase hello.cpp -dumpbase-ext .cpp "-mtune=generic" "-march=x86-64" -o /tmp/cc47fLtr.s
COLLECT_GCC_OPTIONS='-c' '-shared-libgcc' '-mtune=generic' '-march=x86-64'
    as --64 -o hello.o /tmp/cc47fLtr.s
COMPILER_PATH=/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/
LIBRARY_PATH=/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib/:/lib/../lib/:/usr/lib/../lib/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../:/lib/:/usr/lib/
COLLECT_GCC_OPTIONS='-c' '-shared-libgcc' '-mtune=generic' '-march=x86-64'
            "#;
        let expected = Commands {
            build_and_assemble: vec![
                r#"/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/cc1plus -quiet -D_GNU_SOURCE hello.cpp -quiet -dumpbase hello.cpp -dumpbase-ext .cpp "-mtune=generic" "-march=x86-64" -o /tmp/cc47fLtr.s"#,
                r#"as --64 -o hello.o /tmp/cc47fLtr.s"#,
            ],
            link: None,
        };
        assert_eq!(expected, obtain_whole_command(input.lines()).unwrap());
    }
}
