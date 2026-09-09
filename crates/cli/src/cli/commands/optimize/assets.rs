//! Assets distributed in the binary and installed independently of remote templates.

use anyhow::{Context, Result};
use std::{fs, io::Write, path::Path};

macro_rules! asset {
    ($name:literal) => {
        (
            $name,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../resources/newton-template/newton/definitions/software-security/",
                $name
            )),
        )
    };
}

pub(super) const FILES: &[(&str, &str)] = &[
    asset!("definition.yaml"),
    asset!("grade.yaml"),
    asset!("plan.yaml"),
    asset!("develop.yaml"),
    asset!("security.py"),
];

pub(crate) fn install(newton_dir: &Path) -> Result<()> {
    let directory = newton_dir.join("definitions/software-security");
    fs::create_dir_all(&directory)?;
    for (name, source) in FILES {
        let path = directory.join(name);
        if path.exists() {
            if fs::read_to_string(&path)? != *source {
                anyhow::bail!(
                    "refusing to replace existing Optimization Definition asset {}",
                    path.display()
                );
            }
            continue;
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .with_context(|| format!("install {}", path.display()))?;
        file.write_all(source.as_bytes())?;
    }
    Ok(())
}
