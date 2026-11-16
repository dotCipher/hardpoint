//! Host-side helpers, not device firmware. Only recipes compiled into this
//! program may install software. Cards (including private cards) can select an
//! adapter, never a binary, URL, package name or shell command to execute.

use crate::catalog::{Catalog, Observation};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Deserialize)]
struct Registry {
    adapter: BTreeMap<String, Adapter>,
}

#[derive(Deserialize)]
struct Adapter {
    #[serde(default)]
    tool: Vec<Tool>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Tool {
    platform: String,
    arches: Vec<String>,
    binary: String,
    packages: BTreeMap<String, String>,
    note: String,
}

fn registry() -> &'static Registry {
    static REGISTRY: std::sync::OnceLock<Registry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| toml::from_str(include_str!("../catalog/adapters/adapters.toml")).expect("the compiled adapter registry must parse"))
}

fn applicable(tool: &Tool, os: &str, arch: &str) -> bool {
    tool.platform == os && (tool.arches.is_empty() || tool.arches.iter().any(|a| a == arch))
}

fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Services do not inherit the interactive shell's PATH. Include the standard
/// package-manager locations, but never search the current directory implicitly.
pub fn find(binary: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"].map(PathBuf::from))
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(binary))
        .find(|path| executable(path))
}

/// The same OS/architecture selection serves installation and the adapter.
pub fn helper(adapter: &str) -> Option<PathBuf> {
    registry().adapter.get(adapter)?.tool.iter()
        .find(|t| applicable(t, std::env::consts::OS, std::env::consts::ARCH))
        .and_then(|t| find(&t.binary))
}

fn adapters(catalog: &Catalog, seen: &[Observation]) -> BTreeSet<String> {
    catalog.cards.iter().filter(|card| seen.iter().any(|s| card.fits(s)))
        .flat_map(|card| card.capabilities.iter().map(|c| c.adapter.clone())).collect()
}

/// Fixed argument vectors: no shell, extra repositories, casks, upgrades or
/// package-manager bootstrap scripts. Packages come only from the embedded registry.
fn install_args(manager: &str, package: &str) -> Option<Vec<String>> {
    let mut args: Vec<String> = match manager {
        "brew" => vec!["install", "--formula"],
        "apt-get" => vec!["install", "-y"],
        "dnf" => vec!["install", "-y"],
        "pacman" => vec!["-S", "--needed", "--noconfirm"],
        "zypper" => vec!["--non-interactive", "install"],
        "apk" => vec!["add"],
        _ => return None,
    }.into_iter().map(String::from).collect();
    args.push(package.into());
    Some(args)
}

fn manager(tool: &Tool, find: impl Fn(&str) -> Option<PathBuf>) -> Option<(String, PathBuf, Vec<String>)> {
    ["brew", "apt-get", "dnf", "pacman", "zypper", "apk"].into_iter()
        .filter_map(|name| Some((name.to_string(), find(name)?, install_args(name, tool.packages.get(name)?)?)))
        .next()
}

/// Package-manager output goes to the daemon's log (or the setup command's
/// stderr), never the MCP transport. No sudo or interactive password prompt.
fn install(program: &Path, args: &[String]) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let mut child = Command::new(program).args(args)
        .env("HOMEBREW_NO_AUTO_UPDATE", "1").env("HOMEBREW_NO_INSTALL_CLEANUP", "1")
        .env("HOMEBREW_NO_ANALYTICS", "1").env("NONINTERACTIVE", "1")
        .stdin(Stdio::null()).stdout(Stdio::from(std::io::stderr())).stderr(Stdio::inherit()).process_group(0)
        .spawn().map_err(|e| format!("could not start helper installation: {e}"))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("helper installation exited with {status}; see the setup output or daemon log")),
            Ok(None) if start.elapsed() < Duration::from_secs(300) => std::thread::sleep(Duration::from_millis(200)),
            result => {
                // Stop package-manager children as well as the direct process.
                let _ = Command::new("/bin/kill").args(["-TERM", "--", &format!("-{}", child.id())]).status();
                let _ = child.kill();
                let _ = child.wait();
                return Err(match result {
                    Err(e) => format!("could not wait for helper installation: {e}"),
                    _ => "helper installation timed out after five minutes; check for a package-manager lock before retrying".into(),
                });
            }
        }
    }
}

trait Host {
    fn os(&self) -> &str;
    fn arch(&self) -> &str;
    fn find(&self, name: &str) -> Option<PathBuf>;
    fn install(&self, path: &Path, args: &[String]) -> Result<(), String>;
}

struct Native;
impl Host for Native {
    fn os(&self) -> &str { std::env::consts::OS }
    fn arch(&self) -> &str { std::env::consts::ARCH }
    fn find(&self, name: &str) -> Option<PathBuf> { find(name) }
    fn install(&self, path: &Path, args: &[String]) -> Result<(), String> { install(path, args) }
}

/// One attempt per helper per daemon lifetime, shared across cards and reloads.
/// `hardpoint setup` explicitly retries in a fresh invocation. No retry storm
/// when offline, when a formula is broken, or when a package manager is locked.
#[derive(Default)]
pub struct Setup {
    attempted: BTreeMap<String, String>,
}

impl Setup {
    pub fn check(&mut self, catalog: &Catalog, seen: &[Observation], auto: bool, install: bool) -> Value {
        self.check_on(catalog, seen, auto, install, &Native)
    }

    fn check_on(&mut self, catalog: &Catalog, seen: &[Observation], auto: bool, install: bool, host: &impl Host) -> Value {
        let registry = registry();
        let os = host.os();
        let arch = host.arch();
        let mut tools = Vec::new();
        for id in adapters(catalog, seen) {
            let Some(adapter) = registry.adapter.get(&id) else {
                tools.push(json!({ "adapter": id, "state": "unsupported", "note": "this adapter is not in this program; check for a program update" }));
                continue;
            };
            if adapter.tool.is_empty() { continue; }
            let Some(tool) = adapter.tool.iter().find(|t| applicable(t, os, arch)) else {
                tools.push(json!({ "adapter": id, "state": "unsupported", "note": "no helper backend is implemented for this OS and architecture; installing another platform's tool will not fix that" }));
                continue;
            };
            let mut path = host.find(&tool.binary);
            let recipe = manager(tool, |name| host.find(name));
            let command = recipe.as_ref().map(|(name, _, args)| format!("{}{} {}", if name == "brew" { "" } else { "sudo " }, name, args.join(" ")));
            let mut state = "ready";
            let mut note = tool.note.clone();
            if path.is_none() {
                if !auto {
                    state = "missing";
                    note = format!("automatic installation is disabled. {}", tool.note);
                } else if !install {
                    state = "missing";
                    note = format!("read-only check; run `hardpoint setup` or start the daemon for automatic setup. {}", tool.note);
                } else if let Some(error) = self.attempted.get(&tool.binary) {
                    state = "failed";
                    note = error.clone();
                } else if let Some((name, program, args)) = recipe.as_ref() {
                    if name != "brew" {
                        state = "needs-person";
                        note = format!("system package installation needs a person. Hardpoint never elevates privileges. {}", tool.note);
                    } else {
                        eprintln!("tools: installing {} for {id}: {name} {}", tool.binary, args.join(" "));
                        let result = host.install(program, args).and_then(|_| {
                            path = host.find(&tool.binary);
                            path.as_ref().map(|_| ()).ok_or_else(|| "the package manager succeeded but the helper is not executable or not on the search path".into())
                        });
                        match result {
                            Ok(()) => {
                                self.attempted.insert(tool.binary.clone(), "the helper was installed once but is now missing; run `hardpoint setup` to retry".into());
                                eprintln!("tools: {} is ready", tool.binary);
                            }
                            Err(error) => {
                                eprintln!("tools: {error}");
                                self.attempted.insert(tool.binary.clone(), error.clone());
                                state = "failed";
                                note = error;
                            }
                        }
                    }
                } else {
                    state = "needs-person";
                    note = "no supported package manager found. Install the helper or your chosen package manager yourself; Hardpoint does not bootstrap package managers".into();
                }
            }
            tools.push(json!({ "adapter": id, "binary": tool.binary, "state": state, "path": path, "install": command, "note": note }));
        }
        // Requirements are instructions, not a claim that a permission is granted.
        let permissions: Vec<_> = catalog.cards.iter().filter(|c| seen.iter().any(|s| c.fits(s)))
            .filter_map(|c| {
                let needs = c.needs(os);
                (!needs.is_empty()).then(|| json!({ "device": c.device.id, "requirements": needs, "verified": false }))
            }).collect();
        json!({ "os": os, "arch": arch, "auto_install": auto, "tools": tools, "permissions": permissions })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_tools_by_os_and_architecture() {
        let r = registry();
        let tools = &r.adapter["ddc-ci"].tool;
        let selected = |os, arch| tools.iter().find(|t| applicable(t, os, arch)).map(|t| t.binary.as_str());
        assert_eq!(selected("macos", "aarch64"), Some("m1ddc"));
        assert_eq!(selected("macos", "x86_64"), None);
        assert_eq!(selected("linux", "x86_64"), Some("ddcutil"));
        assert_eq!(selected("linux", "aarch64"), Some("ddcutil"));
        assert_eq!(selected("windows", "x86_64"), None);
    }

    #[test]
    fn only_attached_cards_select_helpers_and_shared_adapters_are_deduplicated() {
        let mut c = crate::catalog::repo_catalog();
        assert!(adapters(&c, &[]).is_empty());
        let card = c.cards.iter().find(|c| c.has_adapter("ddc-ci")).unwrap().clone();
        let m = &card.matches[0];
        let seen = Observation { bus: m.bus.clone(), vendor_id: m.vendor_id, product_id: m.product_id,
            usage_pages: m.usage_page.into_iter().collect(), model: m.model.clone(), name: m.name.clone().unwrap_or_default() };
        c.cards.push(card);
        assert_eq!(adapters(&c, &[seen]).iter().filter(|id| *id == "ddc-ci").count(), 1);
    }

    #[test]
    fn install_recipes_are_fixed_argument_vectors() {
        assert_eq!(install_args("brew", "m1ddc").unwrap(), ["install", "--formula", "m1ddc"]);
        assert_eq!(install_args("apt-get", "ddcutil").unwrap(), ["install", "-y", "ddcutil"]);
        assert!(install_args("sh", "anything").is_none());
        let r = registry();
        let linux = &r.adapter["ddc-ci"].tool[1];
        let chosen = manager(linux, |name| (name == "dnf").then(|| PathBuf::from("/usr/bin/dnf"))).unwrap();
        assert_eq!(chosen.0, "dnf");
    }

    #[test]
    fn a_catalog_update_cannot_replace_the_embedded_recipes() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("adapters")).unwrap();
        std::fs::write(temp.path().join("adapters/adapters.toml"), "not valid TOML or an installer").unwrap();
        let (mut c, seen) = display();
        c.root = temp.path().into();
        let host = Fake::new("macos", "aarch64", "brew");
        assert_eq!(Setup::default().check_on(&c, &seen, true, true, &host)["tools"][0]["state"], "ready");
        assert_eq!(*host.calls.borrow(), vec![vec!["install", "--formula", "m1ddc"]]);
    }

    #[test]
    fn non_executable_files_are_not_helpers() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        assert!(!executable(temp.path()));
    }

    struct Fake {
        os: &'static str,
        arch: &'static str,
        found: std::cell::RefCell<BTreeSet<String>>,
        calls: std::cell::RefCell<Vec<Vec<String>>>,
        fail: bool,
    }

    impl Fake {
        fn new(os: &'static str, arch: &'static str, manager: &str) -> Self {
            Self { os, arch, found: std::cell::RefCell::new([manager.into()].into()), calls: Default::default(), fail: false }
        }
    }
    impl Host for Fake {
        fn os(&self) -> &str { self.os }
        fn arch(&self) -> &str { self.arch }
        fn find(&self, name: &str) -> Option<PathBuf> { self.found.borrow().contains(name).then(|| PathBuf::from("/fixture/bin").join(name)) }
        fn install(&self, _: &Path, args: &[String]) -> Result<(), String> {
            self.calls.borrow_mut().push(args.to_vec());
            if self.fail { return Err("fixture offline".into()); }
            self.found.borrow_mut().insert(args.last().unwrap().clone());
            Ok(())
        }
    }

    fn display() -> (Catalog, Vec<Observation>) {
        let c = crate::catalog::repo_catalog();
        let m = &c.cards.iter().find(|c| c.has_adapter("ddc-ci")).unwrap().matches[0];
        let seen = Observation { bus: m.bus.clone(), vendor_id: m.vendor_id, product_id: m.product_id,
            usage_pages: m.usage_page.into_iter().collect(), model: m.model.clone(), name: m.name.clone().unwrap_or_default() };
        (c, vec![seen])
    }

    #[test]
    fn setup_installs_once_and_reuses_the_helper_after_reload() {
        let (c, seen) = display();
        let host = Fake::new("macos", "aarch64", "brew");
        let mut setup = Setup::default();
        assert_eq!(setup.check_on(&c, &seen, true, true, &host)["tools"][0]["state"], "ready");
        assert_eq!(setup.check_on(&c.clone(), &seen, true, true, &host)["tools"][0]["state"], "ready");
        assert_eq!(*host.calls.borrow(), vec![vec!["install", "--formula", "m1ddc"]]);
    }

    #[test]
    fn read_only_disabled_unattached_and_unsupported_never_install() {
        let (c, seen) = display();
        for (os, arch, auto, install, attached) in [
            ("macos", "aarch64", true, false, true),
            ("macos", "aarch64", false, true, true),
            ("macos", "aarch64", true, true, false),
            ("macos", "x86_64", true, true, true),
            ("windows", "x86_64", true, true, true),
        ] {
            let host = Fake::new(os, arch, "brew");
            Setup::default().check_on(&c, if attached { &seen } else { &[] }, auto, install, &host);
            assert!(host.calls.borrow().is_empty());
        }
    }

    #[test]
    fn system_packages_and_absent_package_managers_need_a_person() {
        let (c, seen) = display();
        for host in [Fake::new("linux", "x86_64", "apt-get"), Fake::new("macos", "aarch64", "none")] {
            assert_eq!(Setup::default().check_on(&c, &seen, true, true, &host)["tools"][0]["state"], "needs-person");
            assert!(host.calls.borrow().is_empty(), "no elevation or package manager bootstrap");
        }
    }

    #[test]
    fn failed_installs_do_not_repeat_on_discovery_or_reload() {
        let (c, seen) = display();
        let mut host = Fake::new("macos", "aarch64", "brew");
        host.fail = true;
        let mut setup = Setup::default();
        for _ in 0..3 {
            assert_eq!(setup.check_on(&c, &seen, true, true, &host)["tools"][0]["state"], "failed");
        }
        assert_eq!(host.calls.borrow().len(), 1);
        // A later explicit `setup` invocation can retry.
        host.fail = false;
        assert_eq!(Setup::default().check_on(&c, &seen, true, true, &host)["tools"][0]["state"], "ready");
    }
}
