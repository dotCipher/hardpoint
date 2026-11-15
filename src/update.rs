//! Keeping an installed copy current.
//!
//! Two things can change, and they are treated differently:
//!
//! - The catalog is data. It can update by itself, and by itself it can only
//!   stay as strict or get stricter. An update that would make any label
//!   looser is held back until a person accepts it.
//! - The program is code. It never updates by itself. It is rebuilt from
//!   source when a person asks.
//!
//! Both come from the checkout the installer made, over git, so every change
//! is a commit that can be read.

use crate::catalog::Catalog;
use crate::config::data_dir;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Which commit the installed catalog and the installed program came from.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Stamp {
    pub catalog: String,
    pub program: String,
}

fn checkout() -> PathBuf {
    data_dir().join("src")
}

fn stamp_path() -> PathBuf {
    data_dir().join("installed.json")
}

pub fn stamp() -> Stamp {
    std::fs::read_to_string(stamp_path()).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

fn save(stamp: &Stamp) -> Result<()> {
    std::fs::write(stamp_path(), serde_json::to_string_pretty(stamp)? + "\n").context("recording what is installed")
}

fn git(args: &[&str]) -> Result<String> {
    let out = Command::new("git").arg("-C").arg(checkout()).args(args).output().context("running git")?;
    if !out.status.success() {
        bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Every label the new catalog would make less strict than the old one.
pub fn looser(old: &Catalog, new: &Catalog) -> Vec<String> {
    let mut found = Vec::new();
    for (id, word) in &new.vocabulary {
        if let Some(before) = old.vocabulary.get(id) {
            if word.min_risk < before.min_risk {
                found.push(format!("the floor for {id} would drop from {:?} to {:?}", before.min_risk, word.min_risk));
            }
        }
    }
    for card in &new.cards {
        let Some(before) = old.card(&card.device.id) else { continue };
        for cap in &card.capabilities {
            let Some(was) = before.capabilities.iter().find(|c| c.id == cap.id) else { continue };
            let (then, now) = (old.effective_risk(was, None), new.effective_risk(cap, None));
            if now < then {
                found.push(format!("{} on {} would drop from {then:?} to {now:?}", cap.id, card.device.id));
            }
        }
    }
    found
}

/// The newest commit on the project's main branch, after asking the remote.
fn newest() -> Result<String> {
    if !checkout().join(".git").exists() {
        bail!("this copy was not put in place by the installer, so there is nothing to update from (expected {})", checkout().display());
    }
    git(&["fetch", "--quiet", "origin", "main"])?;
    git(&["rev-parse", "origin/main"])
}

/// What an update did, in words for a person and a flag for the daemon.
pub struct Outcome {
    pub said: String,
    pub changed: bool,
}

/// Bring the catalog up to date. `live` is the catalog directory in use.
pub fn catalog(live: &Path, allow_looser: bool, apply: bool) -> Result<Outcome> {
    let newest = newest()?;
    let mut installed = stamp();
    if installed.catalog == newest {
        return Ok(Outcome { said: "the catalog is up to date".to_string(), changed: false });
    }
    // Lay the new catalog out beside the old one, and read it with this very
    // program: a catalog this build cannot read is not taken.
    let incoming = data_dir().join("catalog.incoming");
    let _ = std::fs::remove_dir_all(&incoming);
    std::fs::create_dir_all(&incoming)?;
    let tar = incoming.join("catalog.tar");
    git(&["archive", "--format=tar", "-o", &tar.to_string_lossy(), &newest, "catalog"])?;
    let unpacked = Command::new("tar").arg("-xf").arg(&tar).arg("-C").arg(&incoming).status().context("unpacking the catalog")?;
    if !unpacked.success() {
        bail!("the new catalog could not be unpacked");
    }
    let fresh = incoming.join("catalog");
    let new = Catalog::load(&fresh).context("the new catalog cannot be read by this version of the program; update the program first")?;
    let old = Catalog::load(live)?;
    let loosened = looser(&old, &new);
    let summary = format!("{} cards (was {})", new.cards.len(), old.cards.len());
    if !loosened.is_empty() && !allow_looser {
        let _ = std::fs::remove_dir_all(&incoming);
        return Ok(Outcome {
            said: format!(
                "a catalog update is waiting, but it would make {} label(s) less strict, so it was not taken: {}. \
                 To accept it, a person runs `hardpoint update --allow-looser`.",
                loosened.len(),
                loosened.join("; ")
            ),
            changed: false,
        });
    }
    if !apply {
        let _ = std::fs::remove_dir_all(&incoming);
        return Ok(Outcome { said: format!("a catalog update is available: {summary}"), changed: false });
    }
    // Swap it in, keeping the old one so a bad update can be put back by hand.
    let previous = data_dir().join("catalog.previous");
    let _ = std::fs::remove_dir_all(&previous);
    std::fs::rename(live, &previous).with_context(|| format!("setting aside {}", live.display()))?;
    std::fs::rename(&fresh, live).context("putting the new catalog in place")?;
    let _ = std::fs::remove_dir_all(&incoming);
    installed.catalog = newest;
    save(&installed)?;
    Ok(Outcome { said: format!("the catalog was updated: {summary}"), changed: true })
}

/// Whether the program's own source has changed since it was built.
pub fn program_is_behind() -> Result<bool> {
    let installed = stamp();
    if installed.program.is_empty() {
        return Ok(false);
    }
    let newest = git(&["rev-parse", "origin/main"])?;
    if installed.program == newest {
        return Ok(false);
    }
    let same = Command::new("git")
        .arg("-C")
        .arg(checkout())
        .args(["diff", "--quiet", &installed.program, &newest, "--", "src", "Cargo.toml", "Cargo.lock", "config"])
        .status()?;
    Ok(!same.success())
}

/// Rebuild the program from the newest source and put it where this one is.
pub fn program() -> Result<String> {
    let newest = newest()?;
    git(&["merge", "--ff-only", "--quiet", "origin/main"])?;
    let built = Command::new("cargo").args(["build", "--release", "--locked"]).current_dir(checkout()).status().context("running cargo")?;
    if !built.success() {
        bail!("the new version did not build; the installed program is unchanged");
    }
    let here = std::env::current_exe()?.canonicalize()?;
    let fresh = checkout().join("target").join("release").join("hardpoint");
    if fresh.canonicalize().ok().as_deref() != Some(&here) {
        let staged = here.with_extension("new");
        std::fs::copy(&fresh, &staged).with_context(|| format!("copying to {}", staged.display()))?;
        std::fs::rename(&staged, &here).with_context(|| format!("replacing {}", here.display()))?;
    }
    let mut installed = stamp();
    installed.program = newest;
    save(&installed)?;
    let mut said = format!("the program was rebuilt and put at {}. Restart the daemon to use it: `hardpoint service restart`.", here.display());
    if let Some(app) = app_of(&here) {
        sign(app);
        said += " macOS ties permissions to the exact program, so it may ask you to grant them again.";
    }
    Ok(said)
}

// ---- running at login

const LABEL: &str = "hardpoint.daemon";

/// The mark, as macOS wants it. Drawn by tools/build.py from the same shapes as the site's.
const ICON: &[u8] = include_bytes!("../packaging/Hardpoint.icns");

fn sign(app: &Path) {
    // Signed for this machine only. macOS ties a permission to the signature,
    // so it may ask again after the program is rebuilt.
    let _ = run("codesign", &["--force", "--sign", "-", &app.to_string_lossy()]);
}

/// The app the program lives in, if it lives in one.
fn app_of(exe: &Path) -> Option<&Path> {
    exe.ancestors().nth(3).filter(|app| app.extension().is_some_and(|e| e == "app"))
}

/// macOS lists a program in its permission settings by the app it belongs
/// to. A bare program shows up as a nameless terminal icon, so the program
/// is given a small app of its own, with Hardpoint's name and mark. The
/// command people type becomes a link to the program inside it.
fn bundle(exe: &Path) -> Result<PathBuf> {
    let app = data_dir().join("Hardpoint.app");
    let inside = app.join("Contents/MacOS/hardpoint");
    std::fs::create_dir_all(app.join("Contents/MacOS"))?;
    std::fs::create_dir_all(app.join("Contents/Resources"))?;
    std::fs::write(app.join("Contents/Resources/Hardpoint.icns"), ICON)?;
    let version = env!("CARGO_PKG_VERSION");
    std::fs::write(
        app.join("Contents/Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Hardpoint</string>
<key>CFBundleDisplayName</key><string>Hardpoint</string>
<key>CFBundleIdentifier</key><string>{LABEL}</string>
<key>CFBundleExecutable</key><string>hardpoint</string>
<key>CFBundleIconFile</key><string>Hardpoint</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>{version}</string>
<key>CFBundleVersion</key><string>{version}</string>
<key>LSUIElement</key><true/>
</dict></plist>
"#
        ),
    )?;
    if exe != inside {
        std::fs::copy(exe, &inside).with_context(|| format!("copying the program into {}", app.display()))?;
        std::fs::remove_file(exe)?;
        std::os::unix::fs::symlink(&inside, exe).with_context(|| format!("linking {} to the app", exe.display()))?;
    }
    sign(&app);
    Ok(inside)
}

fn unit() -> Result<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?);
    Ok(if cfg!(target_os = "macos") {
        home.join("Library/LaunchAgents").join(format!("{LABEL}.plist"))
    } else {
        home.join(".config/systemd/user/hardpoint.service")
    })
}

fn run(program: &str, args: &[&str]) -> Result<bool> {
    Ok(Command::new(program).args(args).output().with_context(|| format!("running {program}"))?.status.success())
}

fn user() -> Result<String> {
    let out = Command::new("id").arg("-u").output()?;
    Ok(format!("gui/{}", String::from_utf8_lossy(&out.stdout).trim()))
}

/// Start the daemon now and at every login, using the system's own service manager.
pub fn service(action: &str) -> Result<String> {
    let unit = unit()?;
    let exe = std::env::current_exe()?.canonicalize()?;
    let log = crate::config::state_dir().join("hardpoint.log");
    let mac = cfg!(target_os = "macos");
    match action {
        "install" => {
            for dir in [unit.parent(), log.parent()].into_iter().flatten() {
                std::fs::create_dir_all(dir)?;
            }
            let exe = if mac { bundle(&exe)? } else { exe };
            let text = if mac {
                format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{LABEL}</string>
<key>ProgramArguments</key><array><string>{}</string><string>run</string></array>
<key>AssociatedBundleIdentifiers</key><array><string>{LABEL}</string></array>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><true/>
<key>StandardOutPath</key><string>{log}</string>
<key>StandardErrorPath</key><string>{log}</string>
</dict></plist>
"#,
                    exe.display(),
                    log = log.display()
                )
            } else {
                format!(
                    "[Unit]\nDescription=Hardpoint\n\n[Service]\nExecStart={} run\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n",
                    exe.display()
                )
            };
            std::fs::write(&unit, text).with_context(|| format!("writing {}", unit.display()))?;
            let started = if mac {
                let _ = run("launchctl", &["bootout", &format!("{}/{LABEL}", user()?)]);
                run("launchctl", &["bootstrap", &user()?, &unit.to_string_lossy()])?
            } else {
                run("systemctl", &["--user", "daemon-reload"])? && run("systemctl", &["--user", "enable", "--now", "hardpoint"])?
            };
            if !started {
                bail!("the service file was written to {} but the service did not start", unit.display());
            }
            Ok(format!("Hardpoint now starts at login. Service file: {}. Log: {}", unit.display(), log.display()))
        }
        "uninstall" => {
            if mac {
                let _ = run("launchctl", &["bootout", &format!("{}/{LABEL}", user()?)]);
            } else {
                let _ = run("systemctl", &["--user", "disable", "--now", "hardpoint"]);
            }
            let _ = std::fs::remove_file(&unit);
            Ok("Hardpoint no longer starts at login, and has been stopped.".to_string())
        }
        "restart" => {
            let ok = if mac {
                run("launchctl", &["kickstart", "-k", &format!("{}/{LABEL}", user()?)])?
            } else {
                run("systemctl", &["--user", "restart", "hardpoint"])?
            };
            if !ok {
                bail!("the service is not installed; start one with `hardpoint service install`, or run `hardpoint run` yourself");
            }
            Ok("Restarted.".to_string())
        }
        "status" => Ok(if unit.exists() { format!("installed: {}", unit.display()) } else { "not installed".to_string() }),
        other => bail!("unknown action {other:?}: use install, uninstall, restart or status"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{repo_catalog, Risk};

    #[test]
    fn an_update_that_loosens_a_label_is_noticed() {
        let old = repo_catalog();
        assert!(looser(&old, &repo_catalog()).is_empty(), "the same catalog loosens nothing");

        // A card that used to be stricter than the floor, and no longer is.
        let mut was_stricter = repo_catalog();
        let cap = was_stricter.cards.iter_mut().flat_map(|c| c.capabilities.iter_mut()).find(|c| c.risk == Risk::Reversible).expect("a reversible ability");
        cap.risk = Risk::Gated;
        let id = cap.id.clone();
        let found = looser(&was_stricter, &old);
        assert!(found.iter().any(|line| line.contains(&id)), "{found:?}");

        // A card cannot loosen anything by claiming less than the floor: the floor still applies.
        let mut below_floor = repo_catalog();
        for cap in below_floor.cards.iter_mut().flat_map(|c| c.capabilities.iter_mut()) {
            cap.risk = Risk::Read;
        }
        assert!(looser(&old, &below_floor).iter().all(|line| !line.contains("floor")));

        // A vocabulary that lowers a floor, which loosens every card using it.
        let mut lowered = repo_catalog();
        lowered.vocabulary.get_mut("power.outlet").expect("the word").min_risk = Risk::Reversible;
        assert!(looser(&old, &lowered).iter().any(|line| line.contains("floor for power.outlet")));

        // Getting stricter, and adding new cards, is always fine.
        let mut stricter = repo_catalog();
        for cap in stricter.cards.iter_mut().flat_map(|c| c.capabilities.iter_mut()) {
            cap.risk = Risk::Gated;
        }
        assert!(looser(&old, &stricter).is_empty());
        let mut fewer = repo_catalog();
        fewer.cards.truncate(1);
        assert!(looser(&fewer, &old).is_empty(), "cards the old catalog never had cannot loosen it");
    }
}
