use anyhow::{Context, Result, bail};
use std::{fmt::Display, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arch {
    X86_64,
    Aarch64,
    Riscv64,
}

impl Display for Arch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            Arch::X86_64 => "x86_64".to_string(),
            Arch::Aarch64 => "aarch64".to_string(),
            Arch::Riscv64 => "riscv64".to_string(),
        };
        write!(f, "{}", str)
    }
}

impl Arch {
    pub(crate) fn emulation(&self) -> &str {
        match self {
            Arch::X86_64 => "elf_x86_64",
            Arch::Aarch64 => "aarch64linux",
            Arch::Riscv64 => "elf64lriscv",
        }
    }

    pub(crate) fn dynamic_linker(&self) -> &str {
        match self {
            Arch::X86_64 => "/lib64/ld-linux-x86-64.so.2",
            Arch::Aarch64 => "/lib/ld-linux-aarch64.so.1",
            Arch::Riscv64 => "/lib/ld-linux-riscv64-lp64d.so.1",
        }
    }
}

impl FromStr for Arch {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "x86_64" => Ok(Self::X86_64),
            "aarch64" => Ok(Self::Aarch64),
            "riscv64" => Ok(Self::Riscv64),
            _ => bail!("Unsupported target architecture: {s}"),
        }
    }
}

/// Parse target arch from the triple and verify it's a supported target.
pub(crate) fn target_arch(string: &str) -> Result<Arch> {
    let (arch, rest) = string
        .split_once('-')
        .with_context(|| format!("Unknown target triple: {string}"))?;
    if rest != "linux-gnu" && rest != "pc-linux-gnu" {
        bail!("Unsupported target triple: {string}");
    }
    arch.parse()
        .with_context(|| format!("While parsing triple {string}"))
}
