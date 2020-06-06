use clap::Clap;
use env_logger::fmt::Color;
use log::{debug, trace, warn, Level, LevelFilter};
use std::io::{ErrorKind, Write};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::{Arc, Mutex};

pub mod args;

/// Parses arguments and returns a `Flags`.
pub fn parse() -> args::Flags {
    args::Flags::parse()
}

/// Sets the logger.
pub fn set_logger(flags: &args::Flags) {
    let level = match flags.vverbose {
        true => LevelFilter::Trace,
        false => match flags.verbose {
            true => LevelFilter::Debug,
            false => LevelFilter::Info,
        },
    };
    env_logger::builder()
        .filter_level(level)
        .format(|buf, record| {
            let mut style = buf.style();

            let level = match record.level() {
                Level::Error => style.set_bold(true).set_color(Color::Red).value("error: "),
                Level::Warn => style
                    .set_bold(true)
                    .set_color(Color::Yellow)
                    .value("warning: "),
                Level::Info => style.set_bold(true).set_color(Color::Green).value(""),
                _ => style.set_color(Color::Rgb(165, 165, 165)).value(""),
            };
            writeln!(buf, "{}{}", level, record.args())
        })
        .init();
}

/// Validate arguments and returns an `Opts`.
pub fn validate(flags: &args::Flags) -> Result<args::Opts, String> {
    match args::Opts::validate(flags) {
        Ok(opts) => Ok(opts),
        Err(e) => Err(e),
    }
}

pub mod pcap;
use pcap::{arp, ethernet, layer, layer::Layer, Indicator, Interface};

/// Gets a list of available network interfaces for the current machine.
pub fn interfaces() -> Vec<Interface> {
    pcap::interfaces()
        .into_iter()
        .filter(|inter| !inter.is_loopback)
        .collect()
}

/// Gets an available network iterface match the name.
pub fn interface(name: Option<String>) -> Result<Interface, String> {
    let mut inters = interfaces();
    if inters.len() <= 0 {
        return Err(String::from("no available interface"));
    }
    if inters.len() > 1 {
        if let None = name {
            return Err(String::from("multiple available interfaces"));
        }
    }
    if let Some(inter_name) = name {
        inters.retain(|current_inter| current_inter.name == inter_name);
        if inters.len() <= 0 {
            return Err(format!("unknown interface {}", inter_name));
        }
    }
    Ok(inters[0].clone())
}

pub fn proxy(
    inter: Interface,
    publish: Option<Ipv4Addr>,
    src: Ipv4Addr,
    dst: SocketAddrV4,
) -> Result<(), String> {
    let (tx, mut rx) = match inter.open() {
        Ok((tx, rx)) => (tx, rx),
        Err(e) => return Err(format!("open: {}", e)),
    };
    let mutex_tx = Arc::new(Mutex::new(tx));

    // Handle received
    loop {
        match rx.next() {
            Ok(frame) => {
                match Indicator::from(frame) {
                    Some(indicator) => {
                        trace!("{}", indicator);

                        match indicator.get_network_type() {
                            Some(t) => {
                                match t {
                                    layer::LayerTypes::Arp => {
                                        if let Some(publish) = publish {
                                            let arp = indicator.get_arp().unwrap();
                                            match arp.is_request_of(src, publish) {
                                                true => {
                                                    let new_arp =
                                                        arp::Arp::reply(&arp, inter.hardware_addr);
                                                    let new_ethernet = ethernet::Ethernet::from(
                                                        new_arp.get_type(),
                                                        new_arp.get_src_hardware_addr(),
                                                        new_arp.get_dst_hardware_addr(),
                                                    )
                                                    .unwrap();

                                                    // Serialize
                                                    let size = new_arp.get_size()
                                                        + new_ethernet.get_size();
                                                    let mut buffer = vec![0u8; size];
                                                    match serialize!(
                                                        &mut buffer,
                                                        &new_ethernet,
                                                        &new_arp
                                                    ) {
                                                        Ok(_) => {}
                                                        Err(e) => {
                                                            warn!("serialize: {}", e);
                                                            continue;
                                                        }
                                                    }

                                                    // Send
                                                }
                                                false => continue,
                                            };
                                        }
                                    }
                                    layer::LayerTypes::Ipv4 => continue,
                                    _ => continue,
                                };
                            }
                            None => continue,
                        };
                    }
                    None => continue,
                };
            }
            Err(e) => {
                if e.kind() != ErrorKind::TimedOut {
                    return Err(format!("pcap: {}", e));
                }
            }
        }
    }
}
