use std::collections::{BTreeMap, HashSet};

use oci_spec::{
    image::ImageConfiguration,
    runtime::{
        Capability, LinuxBuilder, LinuxCapabilitiesBuilder, LinuxIdMappingBuilder,
        LinuxNamespaceBuilder, LinuxNamespaceType, ProcessBuilder, RootBuilder, Spec, SpecBuilder,
    },
    OciSpecError,
};

const DEFAULT_CAPS: &[Capability] = &[
    Capability::AuditWrite,
    Capability::Chown,
    Capability::DacOverride,
    Capability::Fowner,
    Capability::Fsetid,
    Capability::Kill,
    Capability::Mknod,
    Capability::NetBindService,
    Capability::NetRaw,
    Capability::Setfcap,
    Capability::Setgid,
    Capability::Setfcap,
    Capability::Setuid,
    Capability::SysChroot,
];

const DEFAULT_NAMESPACES: &[LinuxNamespaceType] = &[
    // LinuxNamespaceType::User, //TODO: Enable this when network ns is enabled
    LinuxNamespaceType::Mount,
    LinuxNamespaceType::Pid,
    // LinuxNamespaceType::Network, // TODO: Enable this and route correctly...
    LinuxNamespaceType::Ipc,
    LinuxNamespaceType::Uts,
    LinuxNamespaceType::Cgroup,
];

#[derive(Debug, Default)]
pub struct RuntimeOverrides {
    pub additional_args: Option<Vec<String>>,
    pub additional_env: Option<BTreeMap<String, String>>,
    pub terminal: bool,
}

pub fn create_runtime_spec(
    config: &ImageConfiguration,
    overrides: &RuntimeOverrides,
) -> Result<Spec, OciSpecError> {
    if config.config().is_none() {
        return Ok(Spec::default());
    }
    let config = config.config().as_ref().unwrap();
    let spec = SpecBuilder::default();

    let mut args;
    if let Some(override_args) = &overrides.additional_args {
        match config.entrypoint() {
            Some(entry_args) => {
                args = entry_args.clone();
                args.extend(override_args.clone());
            }
            None => {
                args = override_args.clone();
            }
        }
    } else if let Some(cmd_args) = config.cmd() {
        match config.entrypoint() {
            Some(entry_args) => {
                args = entry_args.clone();
                args.extend(cmd_args.clone());
            }
            None => {
                args = cmd_args.clone();
            }
        }
    } else if let Some(entry_args) = config.entrypoint() {
        args = entry_args.clone();
    } else {
        args = vec!["/bin/sh".to_string()];
    }

    let mut env: Vec<String> = config.env().clone().unwrap_or_else(|| {
        vec!["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_string()]
    });

    if let Some(additional_env) = &overrides.additional_env {
        for (key, value) in additional_env.iter() {
            env.push(format!("{}={}", key, value));
        }
    }

    if overrides.terminal {
        env.push("TERM=xterm".to_string());
    }

    let default_caps = DEFAULT_CAPS.iter().cloned().collect::<HashSet<_>>();

    let caps = LinuxCapabilitiesBuilder::default()
        .ambient(default_caps.clone())
        .bounding(default_caps.clone())
        .effective(default_caps.clone())
        .inheritable(default_caps.clone())
        .permitted(default_caps)
        .build()?;

    let process = ProcessBuilder::default()
        .terminal(overrides.terminal)
        .env(env)
        .capabilities(caps)
        .args(args);

    let root = RootBuilder::default()
        .path("rootfs")
        .readonly(false)
        .build()?;

    let mapping = LinuxIdMappingBuilder::default()
        .container_id(0u32)
        .host_id(0u32)
        .size(65536u32)
        .build()?;

    let namespaces = DEFAULT_NAMESPACES
        .iter()
        .map(|ns_type| LinuxNamespaceBuilder::default().typ(*ns_type).build())
        .collect::<Result<Vec<_>, OciSpecError>>()?;

    let linux: oci_spec::runtime::Linux = LinuxBuilder::default()
        .namespaces(namespaces)
        .uid_mappings(vec![mapping.clone()])
        .gid_mappings(vec![mapping])
        .build()?;

    spec.process(process.build()?)
        .root(root)
        .hostname("node")
        .linux(linux)
        .uid_mappings(vec![mapping])
        .gid_mappings(vec![mapping])
        .build()
}
