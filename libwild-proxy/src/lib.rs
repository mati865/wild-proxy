mod outputs_cleanup;

use std::{
    path::{Path, PathBuf},
    process::{Command, exit},
    str::Lines,
};

use anyhow::{Context, Result, bail};

use outputs_cleanup::DeleteOutputs;

// TODOs:
// - Implement the TODOs
// - Better error handling
// - Move fallback to a separate module
// - Implement proper solution and use fallback as a fallback
// - Preserve colors for errors

/// Fallback and ask the OG linker if we cannot figure it out ourselves
pub fn fallback() -> Result<()> {
    let mut files_to_delete = DeleteOutputs::with_capacity(2);
    let mut exe_with_args = std::env::args();
    let zero_position_arg = exe_with_args
        .next()
        .context("Could not obtain binary name from args")?;
    let args = exe_with_args.collect::<Vec<_>>();
    let binary = parse_binary_name(&zero_position_arg)?;

    if args
        .iter()
        .any(|arg| ["--help", "--version", "-###"].contains(&arg.as_str()))
    {
        Command::new(binary).args(&args).status()?;
        return Ok(());
    }

    let compiler_output = Command::new(binary)
        .args(&args)
        .arg("-###")
        .output()
        .with_context(|| format!("Failed to run {binary}"))?;
    if !compiler_output.status.success() {
        String::from_utf8_lossy(&compiler_output.stderr)
            .lines()
            .filter(|line| line.contains("error: "))
            .for_each(|error_line| eprintln!("{}", error_line.trim_end()));
        if let Some(code) = compiler_output.status.code() {
            exit(code);
        } else {
            return Ok(());
        }
    }
    let raw_dump = String::from_utf8(compiler_output.stderr)?;

    let commands = obtain_whole_command(raw_dump.lines())?;
    let mut steps_iterator = commands.build_and_assemble.into_iter().peekable();
    while let Some(command) = steps_iterator.next() {
        let args = shell_words::split(command)?;
        let exit_status = Command::new(args.first().unwrap())
            .args(&args[1..])
            .status()?;

        if !exit_status.success() {
            if let Some(code) = exit_status.code() {
                exit(code);
            } else {
                return Ok(());
            }
        }

        // Add ouytput files from intermediate steps to cleanup.
        if steps_iterator.peek().is_some() || commands.link.is_some() {
            let mut args_iter = args.windows(2);
            if let Some(arg) =
                args_iter.find_map(|window| (window[0] == "-o").then_some(&window[1]))
            {
                files_to_delete.add_output(PathBuf::from(arg));
            }
        }
    }

    let mut wild_result = Ok(());

    if let Some(command) = commands.link {
        let args = shell_words::split(command)?;
        let wild_args = libwild::Args::parse(args.iter().skip(1));

        match wild_args {
            Ok(wild_args) => {
                // Need to cleanup temp files
                // unsafe { libwild::run_in_subprocess(&wild_args) }
                wild_result = libwild::run(&wild_args);
            }
            Err(e) => wild_result = Err(e),
        }
    }

    wild_result
}

fn parse_binary_name(zero_position_arg: &str) -> Result<&str> {
    const NEEDLE: &str = "wild-";
    // Rfind because we may have been given full path to the binary
    let needle_position = zero_position_arg.rfind(NEEDLE);
    let trimmed = needle_position.map(|pos| &zero_position_arg[pos + NEEDLE.len()..]);
    let binary = match trimmed {
        Some(valid_command @ ("cc" | "c++" | "gcc" | "g++" | "clang" | "clang++")) => valid_command,
        _ => bail!(
            "Argument at zero position must follow pattern: `wild-<command>` where command is one of: cc,c++,gcc,g++,clang,clang++ got: {zero_position_arg}"
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
    let mut commands = dumped_lines
        .filter_map(|line| {
            (line.starts_with(' ') && !line.ends_with("(in-process)"))
                .then(|| line.trim())
                .filter(|trimmed| !trimmed.is_empty())
        })
        .collect::<Vec<_>>();

    let linker_command = commands.pop_if(|command| {
        let path = Path::new(command.split(' ').next().unwrap());
        // clang/clang++ binaries perform everything except linking
        !path
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .starts_with("clang")
    });

    let commands = Commands {
        build_and_assemble: commands,
        link: linker_command,
    };

    Ok(commands)
}

fn parse_gcc(dumped_lines: Lines) -> Result<Commands> {
    let mut commands = dumped_lines
        .filter_map(|line| {
            if line.starts_with(' ') {
                Some(line.trim()).filter(|s| !s.is_empty())
            } else if line.contains("error: ") {
                // GCC exits with 0 when passing `-### -wrong-arg` but shows the error message
                eprintln!("{line}");
                exit(1);
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let linker_command = commands.pop_if(|command| {
        let path = Path::new(command.split(' ').next().unwrap());
        // Collect2 binary is responsible for linking, other binaries compile or assebmle
        path.file_stem().unwrap() == "collect2"
    });

    let commands = Commands {
        build_and_assemble: commands,
        link: linker_command,
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
    fn parse_clang_preprocess_only() {
        let input = r#"
clang version 19.1.7
Target: x86_64-pc-linux-gnu
Thread model: posix
InstalledDir: /usr/bin
 (in-process)
    "/usr/bin/clang-19" "-cc1" "-triple" "x86_64-pc-linux-gnu" "-E" "-disable-free" "-clear-ast-before-backend" "-disable-llvm-verifier" "-discard-value-names" "-main-file-name" "-" "-mrelocation-model" "pic" "-pic-level" "2" "-pic-is-pie" "-mframe-pointer=all" "-fmath-errno" "-ffp-contract=on" "-fno-rounding-math" "-mconstructor-aliases" "-funwind-tables=2" "-target-cpu" "x86-64" "-tune-cpu" "generic" "-debugger-tuning=gdb" "-fdebug-compilation-dir=/home/mateusz/Projects/rust" "-fcoverage-compilation-dir=/home/mateusz/Projects/rust" "-resource-dir" "/usr/lib/clang/19" "-internal-isystem" "/usr/lib/clang/19/include" "-internal-isystem" "/usr/local/include" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../x86_64-pc-linux-gnu/include" "-internal-externc-isystem" "/include" "-internal-externc-isystem" "/usr/include" "-ferror-limit" "19" "-stack-protector" "2" "-fgnuc-version=4.2.1" "-fskip-odr-check-in-gmf" "-fcolor-diagnostics" "-faddrsig" "-D__GCC_HAVE_DWARF2_CFI_ASM=1" "-o" "-" "-x" "c" "-"
        "#;
        let expected = Commands {
            build_and_assemble: vec![
                r#""/usr/bin/clang-19" "-cc1" "-triple" "x86_64-pc-linux-gnu" "-E" "-disable-free" "-clear-ast-before-backend" "-disable-llvm-verifier" "-discard-value-names" "-main-file-name" "-" "-mrelocation-model" "pic" "-pic-level" "2" "-pic-is-pie" "-mframe-pointer=all" "-fmath-errno" "-ffp-contract=on" "-fno-rounding-math" "-mconstructor-aliases" "-funwind-tables=2" "-target-cpu" "x86-64" "-tune-cpu" "generic" "-debugger-tuning=gdb" "-fdebug-compilation-dir=/home/mateusz/Projects/rust" "-fcoverage-compilation-dir=/home/mateusz/Projects/rust" "-resource-dir" "/usr/lib/clang/19" "-internal-isystem" "/usr/lib/clang/19/include" "-internal-isystem" "/usr/local/include" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../x86_64-pc-linux-gnu/include" "-internal-externc-isystem" "/include" "-internal-externc-isystem" "/usr/include" "-ferror-limit" "19" "-stack-protector" "2" "-fgnuc-version=4.2.1" "-fskip-odr-check-in-gmf" "-fcolor-diagnostics" "-faddrsig" "-D__GCC_HAVE_DWARF2_CFI_ASM=1" "-o" "-" "-x" "c" "-""#,
            ],
            link: None,
        };
        assert_eq!(expected, obtain_whole_command(input.lines()).unwrap());
    }

    #[test]
    fn parse_clang_compile_only() {
        let input = r#"
clang version 19.1.7
Target: x86_64-pc-linux-gnu
Thread model: posix
InstalledDir: /usr/bin
 (in-process)
 "/usr/bin/clang++" "-cc1" "-triple" "x86_64-pc-linux-gnu" "-S" "-disable-free" "-clear-ast-before-backend" "-disable-llvm-verifier" "-discard-value-names" "-main-file-name" "hello.cpp" "-mrelocation-model" "pic" "-pic-level" "2" "-pic-is-pie" "-mframe-pointer=all" "-fmath-errno" "-ffp-contract=on" "-fno-rounding-math" "-mconstructor-aliases" "-funwind-tables=2" "-target-cpu" "x86-64" "-tune-cpu" "generic" "-debugger-tuning=gdb" "-fdebug-compilation-dir=/tmp" "-fcoverage-compilation-dir=/tmp" "-resource-dir" "/usr/lib/clang/19" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../include/c++/14.2.1" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../include/c++/14.2.1/x86_64-pc-linux-gnu" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../include/c++/14.2.1/backward" "-internal-isystem" "/usr/lib/clang/19/include" "-internal-isystem" "/usr/local/include" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../x86_64-pc-linux-gnu/include" "-internal-externc-isystem" "/include" "-internal-externc-isystem" "/usr/include" "-fdeprecated-macro" "-ferror-limit" "19" "-stack-protector" "2" "-fgnuc-version=4.2.1" "-fskip-odr-check-in-gmf" "-fcxx-exceptions" "-fexceptions" "-fcolor-diagnostics" "-faddrsig" "-D__GCC_HAVE_DWARF2_CFI_ASM=1" "-o" "hello.s" "-x" "c++" "hello.cpp"
                "#;
        let expected = Commands {
            build_and_assemble: vec![
                r#""/usr/bin/clang++" "-cc1" "-triple" "x86_64-pc-linux-gnu" "-S" "-disable-free" "-clear-ast-before-backend" "-disable-llvm-verifier" "-discard-value-names" "-main-file-name" "hello.cpp" "-mrelocation-model" "pic" "-pic-level" "2" "-pic-is-pie" "-mframe-pointer=all" "-fmath-errno" "-ffp-contract=on" "-fno-rounding-math" "-mconstructor-aliases" "-funwind-tables=2" "-target-cpu" "x86-64" "-tune-cpu" "generic" "-debugger-tuning=gdb" "-fdebug-compilation-dir=/tmp" "-fcoverage-compilation-dir=/tmp" "-resource-dir" "/usr/lib/clang/19" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../include/c++/14.2.1" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../include/c++/14.2.1/x86_64-pc-linux-gnu" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../include/c++/14.2.1/backward" "-internal-isystem" "/usr/lib/clang/19/include" "-internal-isystem" "/usr/local/include" "-internal-isystem" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../x86_64-pc-linux-gnu/include" "-internal-externc-isystem" "/include" "-internal-externc-isystem" "/usr/include" "-fdeprecated-macro" "-ferror-limit" "19" "-stack-protector" "2" "-fgnuc-version=4.2.1" "-fskip-odr-check-in-gmf" "-fcxx-exceptions" "-fexceptions" "-fcolor-diagnostics" "-faddrsig" "-D__GCC_HAVE_DWARF2_CFI_ASM=1" "-o" "hello.s" "-x" "c++" "hello.cpp""#,
            ],
            link: None,
        };
        assert_eq!(expected, obtain_whole_command(input.lines()).unwrap());
    }

    #[test]
    fn parse_clang_assemble_only() {
        let input = r#"
clang version 19.1.7
Target: x86_64-pc-linux-gnu
Thread model: posix
InstalledDir: /usr/bin
 (in-process)
 "/usr/bin/clang++" "-cc1as" "-triple" "x86_64-pc-linux-gnu" "-filetype" "obj" "-main-file-name" "hello.s" "-target-cpu" "x86-64" "-fdebug-compilation-dir=/tmp" "-dwarf-debug-producer" "clang version 19.1.7" "-dwarf-version=5" "-mrelocation-model" "pic" "-o" "hello.o" "hello.s"
                "#;
        let expected = Commands {
            build_and_assemble: vec![
                r#""/usr/bin/clang++" "-cc1as" "-triple" "x86_64-pc-linux-gnu" "-filetype" "obj" "-main-file-name" "hello.s" "-target-cpu" "x86-64" "-fdebug-compilation-dir=/tmp" "-dwarf-debug-producer" "clang version 19.1.7" "-dwarf-version=5" "-mrelocation-model" "pic" "-o" "hello.o" "hello.s""#,
            ],
            link: None,
        };
        assert_eq!(expected, obtain_whole_command(input.lines()).unwrap());
    }

    #[test]
    fn parse_clang_link_only() {
        let input = r#"
clang version 19.1.7
Target: x86_64-pc-linux-gnu
Thread model: posix
InstalledDir: /usr/bin
 (in-process)
 "/usr/bin/ld" "--hash-style=gnu" "--build-id" "--eh-frame-hdr" "-m" "elf_x86_64" "-pie" "-dynamic-linker" "/lib64/ld-linux-x86-64.so.2" "-o" "a.out" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib64/Scrt1.o" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib64/crti.o" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/crtbeginS.o" "-L/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1" "-L/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib64" "-L/lib/../lib64" "-L/usr/lib/../lib64" "-L/lib" "-L/usr/lib" "hello.o" "-lstdc++" "-lm" "-lgcc_s" "-lgcc" "-lc" "-lgcc_s" "-lgcc" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/crtendS.o" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib64/crtn.o"
                    "#;
        let expected = Commands {
            build_and_assemble: vec![],
            link: Some(
                r#""/usr/bin/ld" "--hash-style=gnu" "--build-id" "--eh-frame-hdr" "-m" "elf_x86_64" "-pie" "-dynamic-linker" "/lib64/ld-linux-x86-64.so.2" "-o" "a.out" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib64/Scrt1.o" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib64/crti.o" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/crtbeginS.o" "-L/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1" "-L/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib64" "-L/lib/../lib64" "-L/usr/lib/../lib64" "-L/lib" "-L/usr/lib" "hello.o" "-lstdc++" "-lm" "-lgcc_s" "-lgcc" "-lc" "-lgcc_s" "-lgcc" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/crtendS.o" "/usr/bin/../lib64/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib64/crtn.o""#,
            ),
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

    #[test]
    fn parse_gcc_preprocess_only() {
        let input = r#"
Using built-in specs.
COLLECT_GCC=gcc
Target: x86_64-pc-linux-gnu
Configured with: /tmp/pkg/src/gcc/configure --enable-languages=ada,c,c++,d,fortran,go,lto,m2,objc,obj-c++,rust --enable-bootstrap --prefix=/usr --libdir=/usr/lib --libexecdir=/usr/lib --mandir=/usr/share/man --infodir=/usr/share/info --with-bugurl=https://github.com/CachyOS/CachyOS-PKGBUILDS/issues --with-build-config=bootstrap-lto --with-linker-hash-style=gnu --with-system-zlib --enable-__cxa_atexit --enable-cet=auto --enable-checking=release --enable-clocale=gnu --enable-default-pie --enable-default-ssp --enable-gnu-indirect-function --enable-gnu-unique-object --enable-libstdcxx-backtrace --enable-link-serialization=1 --enable-linker-build-id --enable-lto --enable-multilib --enable-plugin --enable-shared --enable-threads=posix --disable-libssp --disable-libstdcxx-pch --disable-werror
Thread model: posix
Supported LTO compression algorithms: zlib zstd
gcc version 14.2.1 20250207 (GCC)
COLLECT_GCC_OPTIONS='-E' '-mtune=generic' '-march=x86-64'
 /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/cc1 -E -quiet - "-mtune=generic" "-march=x86-64" -dumpbase -
COMPILER_PATH=/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/
LIBRARY_PATH=/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib/:/lib/../lib/:/usr/lib/../lib/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../:/lib/:/usr/lib/
COLLECT_GCC_OPTIONS='-E' '-mtune=generic' '-march=x86-64'
        "#;
        let expected = Commands {
            build_and_assemble: vec![
                r#"/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/cc1 -E -quiet - "-mtune=generic" "-march=x86-64" -dumpbase -"#,
            ],
            link: None,
        };
        assert_eq!(expected, obtain_whole_command(input.lines()).unwrap());
    }

    #[test]
    fn parse_gcc_compile_only() {
        let input = r#"
Using built-in specs.
COLLECT_GCC=g++
Target: x86_64-pc-linux-gnu
Configured with: /tmp/pkg/src/gcc/configure --enable-languages=ada,c,c++,d,fortran,go,lto,m2,objc,obj-c++,rust --enable-bootstrap --prefix=/usr --libdir=/usr/lib --libexecdir=/usr/lib --mandir=/usr/share/man --infodir=/usr/share/info --with-bugurl=https://github.com/CachyOS/CachyOS-PKGBUILDS/issues --with-build-config=bootstrap-lto --with-linker-hash-style=gnu --with-system-zlib --enable-__cxa_atexit --enable-cet=auto --enable-checking=release --enable-clocale=gnu --enable-default-pie --enable-default-ssp --enable-gnu-indirect-function --enable-gnu-unique-object --enable-libstdcxx-backtrace --enable-link-serialization=1 --enable-linker-build-id --enable-lto --enable-multilib --enable-plugin --enable-shared --enable-threads=posix --disable-libssp --disable-libstdcxx-pch --disable-werror
Thread model: posix
Supported LTO compression algorithms: zlib zstd
gcc version 14.2.1 20250207 (GCC)
COLLECT_GCC_OPTIONS='-S' '-shared-libgcc' '-mtune=generic' '-march=x86-64'
 /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/cc1plus -quiet -D_GNU_SOURCE hello.cpp -quiet -dumpbase hello.cpp -dumpbase-ext .cpp "-mtune=generic" "-march=x86-64" -o hello.s
COMPILER_PATH=/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/
LIBRARY_PATH=/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib/:/lib/../lib/:/usr/lib/../lib/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../:/lib/:/usr/lib/
COLLECT_GCC_OPTIONS='-S' '-shared-libgcc' '-mtune=generic' '-march=x86-64'
            "#;
        let expected = Commands {
            build_and_assemble: vec![
                r#"/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/cc1plus -quiet -D_GNU_SOURCE hello.cpp -quiet -dumpbase hello.cpp -dumpbase-ext .cpp "-mtune=generic" "-march=x86-64" -o hello.s"#,
            ],
            link: None,
        };
        assert_eq!(expected, obtain_whole_command(input.lines()).unwrap());
    }

    #[test]
    fn parse_gcc_assemble_only() {
        let input = r#"
Using built-in specs.
COLLECT_GCC=g++
Target: x86_64-pc-linux-gnu
Configured with: /tmp/pkg/src/gcc/configure --enable-languages=ada,c,c++,d,fortran,go,lto,m2,objc,obj-c++,rust --enable-bootstrap --prefix=/usr --libdir=/usr/lib --libexecdir=/usr/lib --mandir=/usr/share/man --infodir=/usr/share/info --with-bugurl=https://github.com/CachyOS/CachyOS-PKGBUILDS/issues --with-build-config=bootstrap-lto --with-linker-hash-style=gnu --with-system-zlib --enable-__cxa_atexit --enable-cet=auto --enable-checking=release --enable-clocale=gnu --enable-default-pie --enable-default-ssp --enable-gnu-indirect-function --enable-gnu-unique-object --enable-libstdcxx-backtrace --enable-link-serialization=1 --enable-linker-build-id --enable-lto --enable-multilib --enable-plugin --enable-shared --enable-threads=posix --disable-libssp --disable-libstdcxx-pch --disable-werror
Thread model: posix
Supported LTO compression algorithms: zlib zstd
gcc version 14.2.1 20250207 (GCC)
COLLECT_GCC_OPTIONS='-c' '-shared-libgcc' '-mtune=generic' '-march=x86-64'
 as --64 -o hello.o hello.s
COMPILER_PATH=/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/
LIBRARY_PATH=/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib/:/lib/../lib/:/usr/lib/../lib/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../:/lib/:/usr/lib/
COLLECT_GCC_OPTIONS='-c' '-shared-libgcc' '-mtune=generic' '-march=x86-64'
            "#;
        let expected = Commands {
            build_and_assemble: vec![r#"as --64 -o hello.o hello.s"#],
            link: None,
        };
        assert_eq!(expected, obtain_whole_command(input.lines()).unwrap());
    }

    #[test]
    fn parse_gcc_link_only() {
        let input = r#"
Using built-in specs.
COLLECT_GCC=g++
COLLECT_LTO_WRAPPER=/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/lto-wrapper
Target: x86_64-pc-linux-gnu
Configured with: /tmp/pkg/src/gcc/configure --enable-languages=ada,c,c++,d,fortran,go,lto,m2,objc,obj-c++,rust --enable-bootstrap --prefix=/usr --libdir=/usr/lib --libexecdir=/usr/lib --mandir=/usr/share/man --infodir=/usr/share/info --with-bugurl=https://github.com/CachyOS/CachyOS-PKGBUILDS/issues --with-build-config=bootstrap-lto --with-linker-hash-style=gnu --with-system-zlib --enable-__cxa_atexit --enable-cet=auto --enable-checking=release --enable-clocale=gnu --enable-default-pie --enable-default-ssp --enable-gnu-indirect-function --enable-gnu-unique-object --enable-libstdcxx-backtrace --enable-link-serialization=1 --enable-linker-build-id --enable-lto --enable-multilib --enable-plugin --enable-shared --enable-threads=posix --disable-libssp --disable-libstdcxx-pch --disable-werror
Thread model: posix
Supported LTO compression algorithms: zlib zstd
gcc version 14.2.1 20250207 (GCC)
COMPILER_PATH=/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/
LIBRARY_PATH=/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib/:/lib/../lib/:/usr/lib/../lib/:/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../:/lib/:/usr/lib/
COLLECT_GCC_OPTIONS='-shared-libgcc' '-mtune=generic' '-march=x86-64' '-dumpdir' 'a.'
 /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/collect2 -plugin /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/liblto_plugin.so "-plugin-opt=/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/lto-wrapper" "-plugin-opt=-fresolution=/tmp/ccluTT6J.res" "-plugin-opt=-pass-through=-lgcc_s" "-plugin-opt=-pass-through=-lgcc" "-plugin-opt=-pass-through=-lc" "-plugin-opt=-pass-through=-lgcc_s" "-plugin-opt=-pass-through=-lgcc" --build-id --eh-frame-hdr "--hash-style=gnu" -m elf_x86_64 -dynamic-linker /lib64/ld-linux-x86-64.so.2 -pie /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib/Scrt1.o /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib/crti.o /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/crtbeginS.o -L/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1 -L/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib -L/lib/../lib -L/usr/lib/../lib -L/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../.. hello.o "-lstdc++" -lm -lgcc_s -lgcc -lc -lgcc_s -lgcc /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/crtendS.o /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib/crtn.o
COLLECT_GCC_OPTIONS='-shared-libgcc' '-mtune=generic' '-march=x86-64' '-dumpdir' 'a.'
            "#;
        let expected = Commands {
            build_and_assemble: vec![],
            link: Some(
                r#"/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/collect2 -plugin /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/liblto_plugin.so "-plugin-opt=/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/lto-wrapper" "-plugin-opt=-fresolution=/tmp/ccluTT6J.res" "-plugin-opt=-pass-through=-lgcc_s" "-plugin-opt=-pass-through=-lgcc" "-plugin-opt=-pass-through=-lc" "-plugin-opt=-pass-through=-lgcc_s" "-plugin-opt=-pass-through=-lgcc" --build-id --eh-frame-hdr "--hash-style=gnu" -m elf_x86_64 -dynamic-linker /lib64/ld-linux-x86-64.so.2 -pie /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib/Scrt1.o /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib/crti.o /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/crtbeginS.o -L/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1 -L/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib -L/lib/../lib -L/usr/lib/../lib -L/usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../.. hello.o "-lstdc++" -lm -lgcc_s -lgcc -lc -lgcc_s -lgcc /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/crtendS.o /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/../../../../lib/crtn.o"#,
            ),
        };
        assert_eq!(expected, obtain_whole_command(input.lines()).unwrap());
    }
}
