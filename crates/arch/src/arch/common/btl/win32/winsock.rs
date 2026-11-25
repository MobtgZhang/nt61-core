use super::{allocate, error, free, string, word, ApiError, DecodeMode, GuestImage, GuestMemory, READ, WRITE};
use crate::arch::common::btl::nt61::{GuestServices, SocketKind};
use alloc::string::ToString;
use core::net::{Ipv4Addr, Ipv6Addr};

const VERSION_NOT_SUPPORTED: u64 = 10092;
const FAULT: u64 = 10014;
const NOT_INITIALISED: u32 = 10093;
const OPERATION_NOT_SUPPORTED: u32 = 10045;
const HOST_NOT_FOUND: u32 = 11001;
const NO_BUFFERS: u32 = 10055;
const INVALID_ARGUMENT: u32 = 10022;
const WOULD_BLOCK: u32 = 10035;
const ADDRESS_FAMILY_NOT_SUPPORTED: u32 = 10047;
const MAX_SEND_CHUNK: usize = 1200;
const MAX_RECEIVE_CHUNK: usize = 64 * 1024;

pub(super) fn failure_value(name: &str, code: u32, x86: bool) -> u64 {
    match name {
        "WSAStartup" | "getaddrinfo" | "getnameinfo" => u64::from(code),
        "gethostbyaddr" | "gethostbyname" | "getprotobyname" | "getprotobynumber" |
        "inet_ntoa" | "inet_ntop" | "__WSAFDIsSet" => 0,
        "socket" | "WSASocketW" | "accept" if !x86 => u64::MAX,
        _ => u64::from(u32::MAX),
    }
}

pub(super) fn invoke(
    image: &mut GuestImage, services: &mut dyn GuestServices, name: &str, args: &[u64],
) -> Result<u64, ApiError> {
    let x86 = image.thread.context.mode == DecodeMode::X86;
    match name {
        "WSAStartup" => startup(image, args[0] as u16, args[1], x86),
        "WSACleanup" => {
            image.win32.winsock_startups = image.win32.winsock_startups.checked_sub(1)
                .ok_or(NOT_INITIALISED)?;
            if image.win32.winsock_startups == 0 { services.close_sockets(); }
            Ok(0)
        }
        "socket" | "WSASocketW" => {
            if image.win32.winsock_startups == 0 { return Err(NOT_INITIALISED.into()); }
            if name == "WSASocketW" && (args[3] != 0 || args[4] != 0 || args[5] & !0x81 != 0) {
                return Err(OPERATION_NOT_SUPPORTED.into());
            }
            let kind = socket_kind(args[0] as i32, args[1] as i32, args[2] as i32)?;
            services.open_socket(kind).map_err(Into::into)
        }
        "closesocket" => {
            if image.win32.winsock_startups == 0 { return Err(NOT_INITIALISED.into()); }
            services.close_socket(args[0])?;
            Ok(0)
        }
        "WSAGetLastError" => Ok(word(&mut image.memory, image.teb + if x86 { 0x34 } else { 0x68 }, 4)?),
        "WSASetLastError" => {
            error(image, args[0] as u32);
            Ok(0)
        }
        "htons" | "ntohs" => Ok(u64::from((args[0] as u16).swap_bytes())),
        "htonl" | "ntohl" => Ok(u64::from((args[0] as u32).swap_bytes())),
        "__WSAFDIsSet" => fd_is_set(image, args[0], args[1], if x86 { 4 } else { 8 }),
        "inet_pton" => inet_pton(&mut image.memory, args),
        "inet_ntop" => inet_ntop(&mut image.memory, args),
        "WSCEnumProtocols" | "WSCGetProviderInfo" | "WSCGetProviderPath" => {
            let output = args[if name == "WSCGetProviderInfo" { 5 } else { 3 }];
            if !image.memory.write_virt(output, &OPERATION_NOT_SUPPORTED.to_le_bytes()) {
                return Err((FAULT as u32).into());
            }
            Ok(u64::from(u32::MAX))
        }
        "freeaddrinfo" => {
            if let Some(index) = image.win32.winsock_address_lists.iter().position(|root| *root == args[0]) {
                free(image, args[0])?;
                image.win32.winsock_address_lists.swap_remove(index);
            }
            Ok(0)
        }
        _ if image.win32.winsock_startups == 0 => Err(NOT_INITIALISED.into()),
        "getaddrinfo" => getaddrinfo(image, services, args, if x86 { 4 } else { 8 }),
        "connect" => {
            let mut address = [0; 16];
            if (args[2] as i32) < address.len() as i32
                || image.memory.read_virt(args[1], &mut address) != address.len() {
                return Err((FAULT as u32).into());
            }
            if u16::from_le_bytes([address[0], address[1]]) != 2 {
                return Err(ADDRESS_FAMILY_NOT_SUPPORTED.into());
            }
            let port = u16::from_be_bytes([address[2], address[3]]);
            let ip = [address[4], address[5], address[6], address[7]];
            services.connect_tcp(args[0], ip, port)?;
            Ok(0)
        }
        "send" | "recv" => transfer(&mut image.memory, services, name == "send", args),
        _ => Err(OPERATION_NOT_SUPPORTED.into()),
    }
}

fn transfer(memory: &mut dyn GuestMemory, services: &mut dyn GuestServices, send: bool, args: &[u64])
    -> Result<u64, ApiError>
{
    let length = usize::try_from(args[2] as i32).map_err(|_| INVALID_ARGUMENT)?;
    if !send && length == 0 { return Err(INVALID_ARGUMENT.into()); }
    if args[3] as u32 != 0 { return Err(OPERATION_NOT_SUPPORTED.into()); }
    let size = length.min(if send { MAX_SEND_CHUNK } else { MAX_RECEIVE_CHUNK });
    let mut bytes = alloc::vec::Vec::new();
    bytes.try_reserve_exact(size).map_err(|_| NO_BUFFERS)?;
    bytes.resize(size, 0);
    let count = if send {
        if size != 0 && memory.read_virt(args[1], &mut bytes) != size {
            return Err((FAULT as u32).into());
        }
        services.send_tcp(args[0], &bytes)?
    } else {
        if !memory.probe_write(args[1], size) { return Err((FAULT as u32).into()); }
        let count = match services.recv_tcp(args[0], &mut bytes) {
            Ok(count) => count,
            Err(WOULD_BLOCK) => return Err(ApiError::Retry),
            Err(error) => return Err(error.into()),
        };
        if count > size { return Err(NO_BUFFERS.into()); }
        if count != 0 && !memory.write_virt(args[1], &bytes[..count]) {
            return Err((FAULT as u32).into());
        }
        count
    };
    if count > size { return Err(NO_BUFFERS.into()); }
    Ok(count as u64)
}

fn getaddrinfo(image: &mut GuestImage, services: &mut dyn GuestServices, args: &[u64], width: usize)
    -> Result<u64, ApiError>
{
    let result = args[3];
    if result == 0 || !image.memory.write_virt(result, &[0; 8][..width]) {
        return Err((FAULT as u32).into());
    }
    if args[0] == 0 && args[1] == 0 { return Err(HOST_NOT_FOUND.into()); }
    let mut hints = [0u8; 16];
    if args[2] != 0 && image.memory.read_virt(args[2], &mut hints) != hints.len() {
        return Err((FAULT as u32).into());
    }
    let hint = |offset| i32::from_le_bytes(hints[offset..offset + 4].try_into().unwrap());
    let (flags, family, kind, protocol) = (hint(0), hint(4), hint(8), hint(12));
    if flags & !0x40f != 0 { return Err(10022u32.into()); }
    if !matches!(family, 0 | 2) { return Err(10047u32.into()); }
    let valid_transports = [(1i32, 6i32), (2, 17)];
    let transports = valid_transports.into_iter().filter(|&(socket_kind, socket_protocol)| {
        (kind == 0 || kind == socket_kind) && (protocol == 0 || protocol == socket_protocol)
    });
    let count = transports.clone().count();
    if count == 0 { return Err(socket_kind(family, kind, protocol).err().unwrap_or(10022).into()); }
    let port = if args[1] == 0 {
        0u16
    } else {
        let service = string(&mut image.memory, args[1], false).map_err(|_| FAULT as u32)?;
        service.parse::<u16>().map_err(|_| 10109u32)?
    };
    let mut canonical = if args[0] == 0 {
        if flags & 2 != 0 { return Err(10022u32.into()); }
        None
    } else {
        Some(string(&mut image.memory, args[0], false).map_err(|_| FAULT as u32)?)
    };
    let address = match canonical.as_deref() {
        None if flags & 1 != 0 => [0; 4],
        None => [127, 0, 0, 1],
        Some(name) => match name.parse::<Ipv4Addr>() {
            Ok(address) => address.octets(),
            Err(_) if flags & 4 != 0 => return Err(HOST_NOT_FOUND.into()),
            Err(_) => {
                let query = name.strip_suffix('.').unwrap_or(name);
                if query.len() > 253 || query.split('.').any(|label| label.is_empty() || label.len() > 63) {
                    return Err(HOST_NOT_FOUND.into());
                }
                services.resolve_ipv4(query)?
            }
        },
    };
    if flags & 2 == 0 { canonical = None; }
    if let Some(name) = &mut canonical { name.push('\0'); }
    let header = 16 + width * 4;
    let stride = header + 16;
    let name_offset = count * stride;
    let total = name_offset + canonical.as_ref().map_or(0, |name| name.len());
    image.win32.winsock_address_lists.try_reserve(1).map_err(|_| NO_BUFFERS)?;
    let base = allocate(image, 0, total as u64, READ | WRITE).map_err(|_| NO_BUFFERS)?;
    let mut records = [0; 128];
    for (index, (socket_kind, socket_protocol)) in transports.enumerate() {
        let offset = index * stride;
        let entry = &mut records[offset..offset + stride];
        entry[4..8].copy_from_slice(&2i32.to_le_bytes());
        entry[8..12].copy_from_slice(&socket_kind.to_le_bytes());
        entry[12..16].copy_from_slice(&socket_protocol.to_le_bytes());
        entry[16] = 16;
        let mut put_ptr = |field, pointer: u64| {
            let position = 16 + field * width;
            entry[position..position + width].copy_from_slice(&pointer.to_le_bytes()[..width]);
        };
        if index == 0 && canonical.is_some() { put_ptr(1, base + name_offset as u64); }
        put_ptr(2, base + (offset + header) as u64);
        if index + 1 < count { put_ptr(3, base + (offset + stride) as u64); }
        entry[header..header + 2].copy_from_slice(&2u16.to_le_bytes());
        entry[header + 2..header + 4].copy_from_slice(&port.to_be_bytes());
        entry[header + 4..header + 8].copy_from_slice(&address);
    }
    if !image.memory.write_virt(base, &records[..name_offset])
        || canonical.as_ref().is_some_and(|name| !image.memory.write_virt(base + name_offset as u64, name.as_bytes()))
        || !image.memory.write_virt(result, &base.to_le_bytes()[..width])
    {
        let _ = free(image, base);
        return Err((FAULT as u32).into());
    }
    image.win32.winsock_address_lists.push(base);
    Ok(0)
}

fn inet_pton(memory: &mut dyn GuestMemory, args: &[u64]) -> Result<u64, ApiError> {
    let input = string(memory, args[1], false).map_err(|_| FAULT as u32)?;
    let mut octets = [0; 16];
    let length = match args[0] as i32 {
        2 => {
            let Ok(address) = input.parse::<Ipv4Addr>() else { return Ok(0); };
            octets[..4].copy_from_slice(&address.octets());
            4
        }
        23 => {
            let Ok(address) = input.parse::<Ipv6Addr>() else { return Ok(0); };
            octets = address.octets();
            16
        }
        _ => return Err(10047u32.into()),
    };
    if !memory.write_virt(args[2], &octets[..length]) { return Err((FAULT as u32).into()); }
    Ok(1)
}

fn inet_ntop(memory: &mut dyn GuestMemory, args: &[u64]) -> Result<u64, ApiError> {
    let mut octets = [0; 16];
    let length = match args[0] as i32 {
        2 => 4,
        23 => 16,
        _ => return Err(10047u32.into()),
    };
    if args[1] == 0 || memory.read_virt(args[1], &mut octets[..length]) != length {
        return Err((FAULT as u32).into());
    }
    let mut output = if length == 4 {
        Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]).to_string()
    } else {
        Ipv6Addr::from(octets).to_string()
    };
    output.push('\0');
    if args[2] == 0 || args[3] < output.len() as u64 || !memory.write_virt(args[2], output.as_bytes()) {
        return Err((FAULT as u32).into());
    }
    Ok(args[2])
}

fn socket_kind(family: i32, kind: i32, protocol: i32) -> Result<SocketKind, u32> {
    if !matches!(family, 0 | 2) { return Err(10047); }
    match (kind, protocol) {
        (1, 0 | 6) | (0, 6) => Ok(SocketKind::TcpV4),
        (2, 0 | 17) | (0, 17) => Ok(SocketKind::UdpV4),
        (1 | 2, 6 | 17) => Err(10041),
        (0..=2, _) => Err(10043),
        _ => Err(10044),
    }
}

fn startup(image: &mut GuestImage, requested: u16, output: u64, x86: bool) -> Result<u64, ApiError> {
    let major = requested as u8;
    let minor = (requested >> 8) as u8;
    let negotiated: u16 = match major {
        0 => return Ok(VERSION_NOT_SUPPORTED),
        1 => 1 | (u16::from(minor.min(1)) << 8),
        2 => 2 | (u16::from(minor.min(2)) << 8),
        _ => 0x0202,
    };
    let Some(startups) = image.win32.winsock_startups.checked_add(1) else { return Ok(10055); };

    let (length, description, status, limits) = if x86 { (400, 4, 261, 390) } else { (408, 16, 273, 4) };
    let mut data = [0; 408];
    let data = &mut data[..length];
    if output == 0 || !image.memory.probe_write(output, length)
        || image.memory.read_virt(output, data) != length
    {
        return Ok(FAULT);
    }
    data[..2].copy_from_slice(&negotiated.to_le_bytes());
    data[2..4].copy_from_slice(&0x0202u16.to_le_bytes());
    data[description..description + 12].copy_from_slice(b"WinSock 2.0\0");
    data[status..status + 8].copy_from_slice(b"Running\0");
    let (max_sockets, max_datagram): (u16, u16) = if major == 1 { (0x7fff, 0xffbb) } else { (0, 0) };
    data[limits..limits + 2].copy_from_slice(&max_sockets.to_le_bytes());
    data[limits + 2..limits + 4].copy_from_slice(&max_datagram.to_le_bytes());
    if !image.memory.write_virt(output, data) { return Ok(FAULT); }
    image.win32.winsock_startups = startups;
    Ok(0)
}

fn fd_is_set(image: &mut GuestImage, socket: u64, set: u64, width: usize) -> Result<u64, ApiError> {
    let count = word(&mut image.memory, set, 4).map_err(|_| ApiError::MemoryFault)? as usize;
    if count > 64 { return Err(ApiError::MemoryFault); }
    for index in 0..count {
        let address = set.checked_add(((index + 1) * width) as u64).ok_or(ApiError::MemoryFault)?;
        if word(&mut image.memory, address, width).map_err(|_| ApiError::MemoryFault)? == socket {
            return Ok(1);
        }
    }
    Ok(0)
}
