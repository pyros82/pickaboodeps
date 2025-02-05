use std::{
    fs::{self, DirEntry},
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::Stdio,
    str::FromStr,
};

use anyhow::{bail, Result};
use clap::Parser;

// one possible implementation of walking a directory only visiting files
fn visit_dirs(dir: &Path, cb: &dyn Fn(&DirEntry) -> anyhow::Result<()>) -> anyhow::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit_dirs(&path, cb)?;
            } else {
                cb(&entry)?;
            }
        }
    }
    Ok(())
}

#[derive(clap::Parser)]
struct Opts {
    #[clap(
        long,
        default_value = "cargo check",
        help = "Define how the neccesity of the dependencies should be check. If any of these fail when a dependency is removed, it will remain in the Cargo.toml . This allows features support, --test, etc."
    )]
    cargo_check_command: Vec<String>,
}

impl Opts {
    fn create_cargo_checker(&self) -> CargoChecker {
        CargoChecker {
            cmds: self
                .cargo_check_command
                .iter()
                .map(|cmd| {
                    cmd.split_once(' ')
                        .map_or((cmd.to_owned(), vec![]), |(exe, rest)| {
                            (
                                exe.to_owned(),
                                rest.split(' ').map(|arg| arg.to_owned()).collect(),
                            )
                        })
                })
                .collect(),
        }
    }
}

struct CargoChecker {
    cmds: Vec<(String, Vec<String>)>,
}

impl CargoChecker {
    fn cargo_check_ok(&self) -> bool {
        self.cmds.iter().all(|(exec, args)| {
            !std::process::Command::new(exec)
                .args(args)
                .stderr(Stdio::null())
                .status()
                .expect("cargo check must work")
                .success()
        })
    }
}

fn main() -> Result<()> {
    let opts = Opts::parse();
    let checker = opts.create_cargo_checker();
    if !checker.cargo_check_ok() {
        bail!("Project must compile properly before pickaboo deps check!");
    }
    let dot = PathBuf::from(".");
    visit_dirs(&dot, &|de| {
        if de.file_name() == "Cargo.toml" && !de.path().starts_with("./target/") {
            eprintln!(">>> Working on {}", de.path().display());

            let mut contents =
                String::with_capacity(de.metadata().expect("no metadata??").size() as usize);
            std::fs::OpenOptions::new()
                .read(true)
                .open(de.path())?
                .read_to_string(&mut contents)?;

            if let Some(deps) = toml_edit::DocumentMut::from_str(&contents)?.get("dependencies") {
                let keys: Vec<_> = deps
                    .as_table()
                    .ok_or_else(|| anyhow::format_err!("deps must be a table"))?
                    .iter()
                    .map(|(key, _)| key.to_owned())
                    .collect();

                let mut edit = toml_edit::DocumentMut::from_str(&contents).unwrap();

                for key in &keys {
                    let deps_table = edit["dependencies"].as_table_mut().unwrap();
                    let orig = deps_table.clone();
                    deps_table.remove(key);

                    std::fs::OpenOptions::new()
                        .write(true)
                        .truncate(true)
                        .open(de.path())
                        .expect("Failed to open to write")
                        .write_all(edit.to_string().as_bytes())?;

                    if checker.cargo_check_ok() {
                        *edit["dependencies"].as_table_mut().unwrap() = orig;
                        eprintln!("  > Required: {:?}", key)
                    } else {
                        eprintln!("  > Useless: {:?}", key)
                    }
                }

                std::fs::OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(de.path())
                    .expect("Failed to open to write")
                    .write_all(edit.to_string().as_bytes())?;
            }
        }

        Ok(())
    })?;

    Ok(())
}
