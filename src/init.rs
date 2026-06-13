use std::fs;
use std::path::Path;

use anyhow::{Result, bail};
use tapcue::cli::{Cli, InitCli};
use tapcue::config::EffectiveConfig;

pub(crate) fn run_init(init: &InitCli) -> Result<()> {
    let path = Path::new(".tapcue.toml");

    if path.exists() && !init.force {
        bail!(
            "tapcue: {} already exists; rerun with `tapcue init --force` to overwrite",
            path.display()
        );
    }

    let config = if init.current {
        EffectiveConfig::load(&Cli::without_overrides())?
    } else {
        EffectiveConfig::default()
    };

    let rendered = config.to_pretty_toml()?;
    fs::write(path, rendered)?;

    if init.current {
        println!("tapcue: wrote {} from current effective config", path.display());
    } else {
        println!("tapcue: wrote {} from built-in defaults", path.display());
    }

    Ok(())
}
