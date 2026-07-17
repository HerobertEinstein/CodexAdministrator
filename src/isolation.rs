use std::{
    collections::BTreeSet,
    ffi::{OsStr, OsString},
    path::{Component, Path, PathBuf, Prefix},
};

use anyhow::{Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectIsolationContract {
    executable: PathBuf,
    daily_profile: PathBuf,
    isolated_profile: PathBuf,
    daily_codex_home: PathBuf,
    isolated_codex_home: PathBuf,
    cdp_port: u16,
}

impl DirectIsolationContract {
    pub fn new(
        executable: PathBuf,
        daily_profile: PathBuf,
        isolated_profile: PathBuf,
        daily_codex_home: PathBuf,
        isolated_codex_home: PathBuf,
        cdp_port: u16,
    ) -> Result<Self> {
        for (path, name) in [
            (&executable, "official executable"),
            (&daily_profile, "daily profile"),
            (&isolated_profile, "isolated profile"),
            (&daily_codex_home, "daily CODEX_HOME"),
            (&isolated_codex_home, "isolated CODEX_HOME"),
        ] {
            normalized_components(path)
                .map_err(|error| anyhow::anyhow!("{name} is invalid: {error}"))?;
        }
        if cdp_port < 1024 {
            bail!("isolated CDP port must not use a system-reserved port");
        }

        let daily_paths = [&daily_profile, &daily_codex_home];
        let isolated_paths = [&isolated_profile, &isolated_codex_home];
        for daily in daily_paths {
            for isolated in isolated_paths {
                if paths_overlap(daily, isolated)? {
                    bail!(
                        "isolated path {} overlaps daily path {}",
                        isolated.display(),
                        daily.display()
                    );
                }
            }
        }
        if paths_overlap(&isolated_profile, &isolated_codex_home)? {
            bail!("isolated profile and isolated CODEX_HOME must be separate directories");
        }

        Ok(Self {
            executable,
            daily_profile,
            isolated_profile,
            daily_codex_home,
            isolated_codex_home,
            cdp_port,
        })
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn daily_profile(&self) -> &Path {
        &self.daily_profile
    }

    pub fn isolated_profile(&self) -> &Path {
        &self.isolated_profile
    }

    pub fn daily_codex_home(&self) -> &Path {
        &self.daily_codex_home
    }

    pub fn isolated_codex_home(&self) -> &Path {
        &self.isolated_codex_home
    }

    pub const fn cdp_port(&self) -> u16 {
        self.cdp_port
    }

    pub fn verify_owned_root(&self, root: &Path) -> Result<()> {
        normalized_components(root)
            .map_err(|error| anyhow::anyhow!("isolated instance root is invalid: {error}"))?;
        for daily in [&self.daily_profile, &self.daily_codex_home] {
            if paths_overlap(root, daily)? {
                bail!(
                    "isolated instance root {} overlaps daily path {}",
                    root.display(),
                    daily.display()
                );
            }
        }
        if !paths_equal(&root.join("profile"), &self.isolated_profile)?
            || !paths_equal(&root.join("codex-home"), &self.isolated_codex_home)?
        {
            bail!("isolated profile and CODEX_HOME must be exact children of the owned root");
        }
        Ok(())
    }

    pub fn initial_launch_arguments(&self) -> Vec<OsString> {
        vec![
            OsString::from(format!(
                "--user-data-dir={}",
                self.isolated_profile.display()
            )),
            OsString::from("--remote-debugging-address=127.0.0.1"),
            OsString::from(format!("--remote-debugging-port={}", self.cdp_port)),
            OsString::from("--do-not-de-elevate"),
            OsString::from("--no-first-run"),
        ]
    }

    pub fn activation_arguments(&self) -> Vec<OsString> {
        let mut arguments = self.initial_launch_arguments();
        arguments.push(OsString::from("--new-window"));
        arguments
    }

    pub fn environment_overrides(&self) -> Vec<(OsString, OsString)> {
        vec![
            (
                OsString::from("CODEX_HOME"),
                self.isolated_codex_home.as_os_str().to_os_string(),
            ),
            (
                OsString::from("CODEX_SQLITE_HOME"),
                self.isolated_codex_home
                    .join("sqlite")
                    .as_os_str()
                    .to_os_string(),
            ),
        ]
    }

    pub fn verify_runtime(&self, observation: &IsolatedRuntimeObservation) -> Result<()> {
        if !observation.daily_root_alive {
            bail!("daily ChatGPT instance is no longer alive");
        }
        if observation.owned_pids.is_empty() {
            bail!("isolated process tree is empty");
        }
        if !observation
            .owned_pids
            .is_disjoint(&observation.preexisting_pids)
        {
            bail!("isolated process tree overlaps a pre-existing ChatGPT process");
        }
        if !observation
            .owned_pids
            .contains(&observation.cdp_listener_pid)
        {
            bail!("isolated CDP listener is not owned by the project process tree");
        }
        if observation.cdp_port != self.cdp_port {
            bail!("isolated CDP port does not match the launch contract");
        }
        if observation.cdp_target_url.as_deref() != Some("app://-/index.html") {
            bail!("isolated CDP target is not the official app renderer");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsolatedRuntimeObservation {
    pub preexisting_pids: BTreeSet<u32>,
    pub owned_pids: BTreeSet<u32>,
    pub cdp_listener_pid: u32,
    pub daily_root_alive: bool,
    pub cdp_port: u16,
    pub cdp_target_url: Option<String>,
}

fn paths_equal(left: &Path, right: &Path) -> Result<bool> {
    Ok(normalized_components(left)? == normalized_components(right)?)
}

fn paths_overlap(left: &Path, right: &Path) -> Result<bool> {
    let left = normalized_components(left)?;
    let right = normalized_components(right)?;
    Ok(is_prefix(&left, &right) || is_prefix(&right, &left))
}

fn is_prefix(prefix: &[String], value: &[String]) -> bool {
    prefix.len() <= value.len() && prefix.iter().zip(value).all(|(left, right)| left == right)
}

fn normalized_components(path: &Path) -> Result<Vec<String>> {
    if !path.is_absolute() {
        bail!("path must be absolute: {}", path.display());
    }
    let mut normalized = Vec::new();
    for component in path.components() {
        let value = match component {
            Component::Prefix(value) => match value.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                    format!("disk:{}", char::from(letter).to_ascii_lowercase())
                }
                Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                    format!("unc:{}\\{}", normalize_name(server), normalize_name(share))
                }
                Prefix::DeviceNS(device) => format!("device:{}", normalize_name(device)),
                Prefix::Verbatim(value) => format!("verbatim:{}", normalize_name(value)),
            },
            Component::RootDir => "\\".into(),
            Component::Normal(value) => normalize_name(value),
            Component::CurDir | Component::ParentDir => {
                bail!(
                    "path may not contain relative components: {}",
                    path.display()
                )
            }
        };
        normalized.push(value);
    }
    Ok(normalized)
}

fn normalize_name(value: &OsStr) -> String {
    value
        .to_string_lossy()
        .trim_end_matches([' ', '.'])
        .to_lowercase()
}
