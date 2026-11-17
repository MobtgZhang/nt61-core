//! Host tests exercise the same RFC 2131 client used by the kernel.
extern crate alloc;
pub mod hal {
    pub mod common { pub mod pit { pub fn get_system_time_ms() -> u64 { 42 } } }
    pub mod serial { pub fn write_string_always(_: &str) {} }
}
pub mod netstack {
    pub mod udp {
        pub fn create_socket(_: u16, _: u32) -> Option<usize> { Some(0) }
        pub fn send(_: usize, _: u32, _: u16, data: &[u8]) -> Option<usize> { Some(data.len()) }
        pub fn receive(_: usize, _: &mut [u8]) -> Option<(u32, u16, usize)> { None }
    }
    pub mod dns { pub fn set_dns_server(_: u32) {} }
    pub mod ipif {
        pub struct Interface {pub eth_if:u32,pub if_index:u32}
        pub fn get_all_interfaces()->Vec<Interface>{vec![]}
        pub fn add_route(_:u32,_:u32,_:u32,_:u32,_:u32)->bool{true}
    }
    pub mod routing {pub fn set_default_gateway(_:u32,_:u32,_:u32){}}
}
#[path = "../../netstack/src/netstack/dhcp.rs"]
pub mod dhcp;

const MAC:[u8;6]=[0x52,0x54,0,0x12,0x34,0x56];
fn reply(request:&[u8],kind:u8)->Vec<u8>{
    let mut packet=request[..240].to_vec();
    packet[0]=2;packet[16..20].copy_from_slice(&[10,0,2,15]);
    packet.extend_from_slice(&[53,1,kind,54,4,10,0,2,2,1,4,255,255,255,0,3,4,10,0,2,2,6,4,10,0,2,3,51,4,0,0,14,16,255]);
    packet
}
#[test]
fn offer_request_keep_one_transaction_and_publish_ack(){
    let mut client=dhcp::create_client();
    let discover=client.start_discovery(&MAC);
    assert!(client.process_message(&reply(&discover,2),&MAC));
    let request=client.build_request_from_offer(&MAC);
    assert_eq!(&request[4..8],&discover[4..8],"DISCOVER/OFFER/REQUEST/ACK form one transaction");
    assert!(client.process_message(&reply(&request,5),&MAC));
    let lease=client.get_lease().unwrap();
    assert_eq!(lease.ip_address,0x0a00020f);
    assert_eq!(lease.dns_servers[0],0x0a000203);
}
#[test]
fn rejects_non_dhcp_bootp_packet(){
    let mut client=dhcp::create_client();
    let discover=client.start_discovery(&MAC);
    let mut offer=reply(&discover,2);offer[236]=0;
    assert!(!client.process_message(&offer,&MAC));
    assert_eq!(client.get_state(),dhcp::DhcpState::Selecting);
}
