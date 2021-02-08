// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]

mod netlink;

use netlink::{Address, Interface, Subnet};

use std::collections::{HashMap, HashSet};
use std::fs::{read_dir, read_link, File};
use std::io::{BufRead, BufReader, Result};
use std::net::IpAddr;
use std::os::unix::prelude::*;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;

use caps::{CapSet, Capability};
use structopt::StructOpt;

fn flock(fd: &impl AsRawFd, flags: libc::c_int) -> Result<()> {
    match unsafe { libc::flock(fd.as_raw_fd(), flags) } {
        -1 => Err(std::io::Error::last_os_error()),
        0 => Ok(()),
        _ => unreachable!(),
    }
}

fn setns(fd: &impl AsRawFd, flags: libc::c_int) -> Result<()> {
    caps::with(Capability::CAP_SYS_ADMIN, || {
        match unsafe { libc::setns(fd.as_raw_fd(), flags) } {
            -1 => Err(std::io::Error::last_os_error()),
            0 => Ok(()),
            _ => unreachable!(),
        }
    })
}

fn unshare(flags: libc::c_int) -> Result<()> {
    caps::with(Capability::CAP_SYS_ADMIN, || {
        match unsafe { libc::unshare(flags) } {
            -1 => Err(std::io::Error::last_os_error()),
            0 => Ok(()),
            _ => unreachable!(),
        }
    })
}

/// Returns an iterator to all `/proc/<pid>` directories
fn processes() -> Result<impl Iterator<Item = PathBuf>> {
    Ok(read_dir("/proc")?.filter_map(Result::ok).filter_map(|e| {
        e.file_name()
            .to_str()
            .map(|s| u64::from_str(s).ok().map(|_| e.path()))
            .flatten()
    }))
}

/// Loads all unique network namespaces for all processes
fn load_namespaces() -> Result<Vec<File>> {
    let mut namespaces = HashMap::new();

    for process in processes()? {
        for file in read_dir(process.join("fd"))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter_map(|p| read_link(&p).ok().map(|l| (p, l)))
            .filter(|(_, l)| l.starts_with("net:"))
            .filter_map(|(p, _)| File::open(p).ok())
        {
            if let Ok(metadata) = file.metadata() {
                namespaces.insert((metadata.dev(), metadata.ino()), file);
            }
        }

        if let Ok(file) = File::open(process.join("ns").join("net")) {
            if let Ok(metadata) = file.metadata() {
                namespaces.insert((metadata.dev(), metadata.ino()), file);
            }
        }
    }

    Ok(namespaces.into_iter().map(|(_, v)| v).collect())
}

/// Finds all in-use ip addresses for each subnet in each namespace
fn scan_namespaces(subnets: HashSet<Subnet>) -> Result<HashSet<IpAddr>> {
    let saved = File::open("/proc/self/ns/net")?;
    let mut used = HashSet::<IpAddr>::new();

    let namespaces = caps::with(Capability::CAP_DAC_OVERRIDE, load_namespaces)?;
    caps::drop(None, CapSet::Permitted, Capability::CAP_DAC_OVERRIDE)?;
    for ns in namespaces {
        setns(&ns, libc::CLONE_NEWNET)?;

        for address in Address::list()? {
            for subnet in &subnets {
                let addr = address.address();
                if subnet.contains(addr) {
                    used.insert(addr);
                }
            }
        }
    }

    setns(&saved, libc::CLONE_NEWNET)?;
    Ok(used)
}

/// Reads in the configuration, deduplicating subnets
fn load_config(config: impl BufRead) -> Result<HashSet<Subnet>> {
    let mut subnets = HashSet::<Subnet>::new();

    for line in config.lines() {
        let line = line?;
        if !line.starts_with('#') {
            subnets.insert(line.parse()?);
        }
    }

    Ok(subnets)
}

#[derive(Debug, StructOpt)]
#[structopt(name = "ipvlan", about = "Builds an ipvlan network namespace.")]
struct Options {
    /// The ipvlan subnet configuration file.
    #[structopt(short, long, default_value = "/etc/ipvlan.conf")]
    config: PathBuf,

    /// The binary to execute and its arguments
    #[structopt(default_value = "/bin/bash")]
    argv: Vec<String>,
}

fn main() -> Result<()> {
    const LO_ADDR6: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
    const LO_ADDR4: [u8; 4] = [127, 0, 0, 1];

    // Parse our arguments.
    let options = Options::from_args();

    // Validate our capabilities.
    let permitted = caps::read(None, CapSet::Permitted)?;
    let effective = caps::read(None, CapSet::Effective)?;
    assert!(permitted.contains(&Capability::CAP_DAC_OVERRIDE));
    assert!(permitted.contains(&Capability::CAP_NET_ADMIN));
    assert!(permitted.contains(&Capability::CAP_SYS_ADMIN));
    assert_eq!(permitted.len(), 3);
    assert!(effective.is_empty());

    // Open and lock the configuration file.
    let conf = File::open(options.config)?;
    flock(&conf, libc::LOCK_EX)?;

    // Validate configuration file permissions.
    let md = conf.metadata()?;
    //assert_eq!(md.dev(), File::open("/proc/self/exe")?.metadata()?.dev());
    assert_eq!(md.uid(), 0); // Must be owned by root.
    let mut mode = md.mode();
    mode &= 0o7777;
    mode &= !0o0444; // Remove read bits
    mode &= !0o0200; // Remove owner write bit.
    assert_eq!(mode, 0o0000);

    // Parse the configuration file.
    let mut conf = BufReader::new(conf);
    let subnets = load_config(&mut conf)?;

    // Collect the interfaces we want to vlan and their gateway addresses.
    let mut ipvlans = HashMap::<Interface, Vec<Address>>::new();
    for subnet in &subnets {
        let gateway = Address::list()?
            .into_iter()
            .find(|x| x.subnet() == *subnet)
            .unwrap_or_else(|| panic!("unable to find gateway for {}", subnet));

        ipvlans
            .entry(gateway.interface()?)
            .and_modify(|x| x.push(gateway))
            .or_insert_with(|| vec![gateway]);
    }
    let mut ipvlans: Vec<(Interface, Vec<Address>)> = ipvlans.into_iter().collect();

    // Scan for in-use ip addresses.
    let used = scan_namespaces(subnets)?;

    // Set up the namespaces.
    let oldns = File::open("/proc/self/ns/net")?;
    unshare(libc::CLONE_NEWNET)?;
    let newns = File::open("/proc/self/ns/net")?;
    setns(&oldns, libc::CLONE_NEWNET)?;

    // Create our macvlan interfaces in the new namespace.
    for (i, (interface, _)) in ipvlans.iter_mut().enumerate() {
        let name = format!("ipvl{}", i);
        caps::with(Capability::CAP_NET_ADMIN, || -> Result<()> {
            let ipvlan = interface.add_ipvlan(&name)?;
            match ipvlan.move_to_namespace(&newns) {
                Ok(..) => Ok(()),
                Err((ipvlan, error)) => {
                    ipvlan.delete().unwrap();
                    Err(error.into())
                }
            }
        })?;
    }

    // Swap to the new namespace.
    setns(&newns, libc::CLONE_NEWNET)?;
    drop(oldns);
    drop(newns);

    caps::drop(None, CapSet::Permitted, Capability::CAP_SYS_ADMIN)?;

    // Bring up the new ipvlan interfaces.
    for (i, (_, gateways)) in ipvlans.iter().enumerate() {
        let name = format!("ipvl{}", i);

        for gateway in gateways {
            let subnet = gateway.subnet();
            let address = loop {
                let proposed = subnet.random();
                if !used.contains(&proposed) {
                    break proposed;
                }
            };

            let mut ipvlan = Interface::find(&name)?;
            caps::with(Capability::CAP_NET_ADMIN, || -> Result<()> {
                ipvlan.add_address(address, subnet.prefix())?;
                ipvlan.up()?;
                ipvlan.add_gateway(gateway.address())?;
                Ok(())
            })?
        }
    }

    // Bring up the loopback interface.
    let mut ipvlan = Interface::find("lo")?;
    caps::with(Capability::CAP_NET_ADMIN, || -> Result<()> {
        ipvlan.add_address(IpAddr::V6(LO_ADDR6.into()), 128)?;
        ipvlan.add_address(IpAddr::V4(LO_ADDR4.into()), 8)?;
        ipvlan.up()?;
        Ok(())
    })?;

    caps::drop(None, CapSet::Permitted, Capability::CAP_NET_ADMIN)?;

    // Release the lock and execute.
    drop(conf);
    Err(Command::new(&options.argv[0])
        .args(&options.argv[1..])
        .exec())
}
