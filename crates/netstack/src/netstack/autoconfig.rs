//! Per-adapter DHCP lifecycle, driven by the ordinary network poller.
//! No address, gateway or DNS server is inferred from the hypervisor model.
use alloc::vec::Vec;
use crate::drivers::net::{self, NicType};
use crate::ke::sync::Spinlock;
use super::{dhcp::{DhcpClient, DhcpLease, DhcpState}, ethif, ipif};

struct Adapter {
    ethernet: ethif::EthInterface,
    client: DhcpClient,
    interface: Option<u32>,
    published: Option<DhcpLease>,
    transmit: bool,
}
static ADAPTERS: Spinlock<Vec<Adapter>> = Spinlock::new(Vec::new());

pub fn init() {
    let mut adapters = ADAPTERS.lock();
    for ethernet in ethif::get_all_interfaces() {
        if !adapters.iter().any(|adapter| adapter.ethernet.if_index == ethernet.if_index) {
            adapters.push(Adapter { ethernet, client: DhcpClient::new(), interface: None,
                published: None, transmit: true });
        }
    }
}

fn publish(adapter: &mut Adapter) {
    let Some(lease) = adapter.client.get_lease().cloned() else { return; };
    if let Some(id) = adapter.interface.take() { ipif::remove_interface(id); }
    adapter.interface = ipif::add_interface(lease.ip_address, lease.subnet_mask,
        lease.gateway, adapter.ethernet.if_index);
    if adapter.interface.is_some() {
        crate::hal::serial::write_string_always(&alloc::format!(
            "[NETWORK] DHCP {} bound {} gateway {} DNS {}\r\n",
            adapter.ethernet.if_index, ipif::ip_to_string(lease.ip_address),
            ipif::ip_to_string(lease.gateway), ipif::ip_to_string(lease.dns_servers[0])));
        adapter.published = Some(lease);
    }
}

fn publish_dns(adapters: &[Adapter]) {
    let dns = adapters.iter().filter_map(|a| a.published.as_ref())
        .flat_map(|lease| lease.dns_servers).find(|ip| *ip != 0).unwrap_or(0);
    super::dns::set_dns_server(dns);
}

/// Claims only DHCP server-to-client datagrams, including pre-address unicast.
/// All other UDP packets continue through the regular socket demultiplexer.
pub fn input(src_ip:u32, dst_ip:u32, data:&[u8], nic:NicType, index:usize)->bool {
    if data.len() < 8 || data[..4] != [0, 67, 0, 68] { return false; }
    let length = u16::from_be_bytes([data[4], data[5]]) as usize;
    if length < 248 || length > data.len() { return true; }
    if data[6..8] != [0,0] && super::udp::calculate_checksum(src_ip,dst_ip,&data[..length]) != 0 {
        return true;
    }
    let Some(mut adapters) = ADAPTERS.try_lock() else { return true; };
    let Some(adapter) = adapters.iter_mut().find(|a|
        a.ethernet.nic_type == nic && a.ethernet.nic_index == index) else { return true; };
    if adapter.client.process_message(&data[8..length], &adapter.ethernet.mac) {
        match adapter.client.get_state() {
            DhcpState::Bound => { publish(adapter); adapter.transmit = false; }
            DhcpState::Init => {
                if let Some(id) = adapter.interface.take() { ipif::remove_interface(id); }
                adapter.published = None; adapter.transmit = true;
            }
            _ => adapter.transmit = true,
        }
        publish_dns(&adapters);
    }
    true
}

pub fn poll() {
    let Some(mut adapters) = ADAPTERS.try_lock() else { return; };
    let mut dns_changed = false;
    for adapter in adapters.iter_mut() {
        let before = adapter.client.get_state();
        adapter.client.check_lease();
        if adapter.client.get_state() != before { adapter.transmit = true; }
        if adapter.client.get_state() == DhcpState::Init {
            if let Some(id) = adapter.interface.take() { ipif::remove_interface(id); }
            if adapter.published.take().is_some() { dns_changed = true; }
        }
        if adapter.client.get_state() == DhcpState::Bound { continue; }
        if !adapter.transmit && !adapter.client.should_retransmit() { continue; }
        let payload = match adapter.client.get_state() {
            DhcpState::Init | DhcpState::Selecting => adapter.client.start_discovery(&adapter.ethernet.mac),
            DhcpState::Requesting => adapter.client.build_request_from_offer(&adapter.ethernet.mac),
            DhcpState::Renewing | DhcpState::Rebinding => adapter.client.build_renewal_request(&adapter.ethernet.mac),
            _ => continue,
        };
        let source = adapter.published.as_ref().map_or(0, |lease| lease.ip_address);
        let destination = if adapter.client.get_state() == DhcpState::Renewing {
            adapter.published.as_ref().map_or(u32::MAX, |lease| lease.server_id)
        } else { u32::MAX };
        let mut udp = Vec::with_capacity(8 + payload.len());
        udp.extend_from_slice(&[0,68,0,67]);
        udp.extend_from_slice(&((8 + payload.len()) as u16).to_be_bytes());
        udp.extend_from_slice(&[0,0]); // IPv4 permits an omitted UDP checksum.
        udp.extend_from_slice(&payload);
        if destination != u32::MAX {
            super::ipv4::send_ipv4(source,destination,17,&udp);
        } else {
            let packet = super::ipv4::build_ipv4_packet(source,destination,17,64,&udp);
            let frame = super::eth::build_ethernet_frame(&[255;6],&adapter.ethernet.mac,
                super::eth::ETHER_TYPE_IPV4,&packet);
            net::send_to_nic(adapter.ethernet.nic_type,adapter.ethernet.nic_index,&frame);
        }
        adapter.client.increment_retransmit();
        adapter.transmit = false;
    }
    if dns_changed { publish_dns(&adapters); }
}
