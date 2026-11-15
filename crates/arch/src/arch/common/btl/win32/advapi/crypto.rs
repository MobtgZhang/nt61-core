//! Checked CryptoAPI hash contexts for one isolated guest process.
//!
//! Implements the documented hash lifecycle, not key storage, signing, encryption
//! or a cryptographic random source. Inputs are bounded to 1 MiB per hash and
//! 8 MiB across the process, with at most 256 providers and 512 hashes.
//! Microsoft contracts:
//! https://learn.microsoft.com/windows/win32/api/wincrypt/nf-wincrypt-cryptacquirecontextw
//! https://learn.microsoft.com/windows/win32/api/wincrypt/nf-wincrypt-cryptcreatehash
//! https://learn.microsoft.com/windows/win32/api/wincrypt/nf-wincrypt-crypthashdata
//! https://learn.microsoft.com/windows/win32/api/wincrypt/nf-wincrypt-cryptgethashparam
//! https://learn.microsoft.com/windows/win32/api/wincrypt/nf-wincrypt-cryptsethashparam
//! https://learn.microsoft.com/windows/win32/api/wincrypt/nf-wincrypt-cryptduplicatehash
//! https://learn.microsoft.com/windows/win32/api/wincrypt/nf-wincrypt-cryptreleasecontext

use super::super::{put, string, word};
use crate::arch::common::btl::{ir::DecodeMode, memory::GuestMemory, pe::GuestImage};
use alloc::vec::Vec;

#[path = "../../../../../../../crypto/src/crypto/hash.rs"]
mod hash;

const BAD_UID: u32 = 0x8009_0001;
const BAD_HASH: u32 = 0x8009_0002;
const BAD_KEY: u32 = 0x8009_0003;
const BAD_ALGID: u32 = 0x8009_0008;
const BAD_FLAGS: u32 = 0x8009_0009;
const BAD_TYPE: u32 = 0x8009_000a;
const BAD_HASH_STATE: u32 = 0x8009_000c;
const NO_MEMORY: u32 = 0x8009_000e;
const BAD_PROV_TYPE: u32 = 0x8009_0014;
const KEYSET_NOT_DEF: u32 = 0x8009_0019;
const BAD_KEYSET: u32 = 0x8009_0016;
const FAIL: u32 = 0x8009_0020;
const MAX_HASH_BYTES: usize = 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 8 * MAX_HASH_BYTES;
const PROVIDER_NAME: &str = "NT61 Software Hash Provider";

struct Provider {
    handle: u64,
    kind: u32,
    refs: u32,
}
struct Hash {
    handle: u64,
    provider: u64,
    algorithm: u32,
    data: Vec<u8>,
    digest: Option<Vec<u8>>,
}
pub struct State {
    providers: Vec<Provider>,
    hashes: Vec<Hash>,
    next: u64,
}
impl State {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
            hashes: Vec::new(),
            next: 0x3a00_0000,
        }
    }
    fn next_handle(&mut self) -> Result<u64, u32> {
        let value = self.next;
        self.next = self
            .next
            .checked_add(4)
            .filter(|n| *n < 0x3b00_0000)
            .ok_or(NO_MEMORY)?;
        Ok(value)
    }
    fn provider(&self, handle: u64) -> Result<&Provider, u32> {
        self.providers
            .iter()
            .find(|p| p.handle == handle)
            .ok_or(BAD_UID)
    }
    fn hash_index(&self, handle: u64) -> Result<usize, u32> {
        self.hashes
            .iter()
            .position(|h| h.handle == handle)
            .ok_or(BAD_HASH)
    }
    fn bytes(&self) -> usize {
        self.hashes.iter().map(|h| h.data.len()).sum()
    }
}
fn hash_size(algorithm: u32) -> Option<usize> {
    Some(match algorithm {
        0x8003 => 16,
        0x8004 => 20,
        0x800c => 32,
        0x800d => 48,
        0x800e => 64,
        _ => return None,
    })
}
fn digest(algorithm: u32, data: &[u8]) -> Vec<u8> {
    match algorithm {
        0x8003 => hash::md5(data).to_vec(),
        0x8004 => hash::sha1(data).to_vec(),
        0x800c => hash::sha256(data).to_vec(),
        0x800d => hash::sha384(data).to_vec(),
        0x800e => hash::sha512(data).to_vec(),
        _ => Vec::new(), // Created contexts are validated by CryptCreateHash.
    }
}
fn copy_bytes(data: &[u8]) -> Result<Vec<u8>, u32> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(data.len()).map_err(|_| NO_MEMORY)?;
    copy.extend_from_slice(data);
    Ok(copy)
}
fn read_bytes(image: &mut GuestImage, pointer: u64, length: usize) -> Result<Vec<u8>, u32> {
    let mut data = Vec::new();
    data.try_reserve_exact(length).map_err(|_| NO_MEMORY)?;
    data.resize(length, 0);
    if image.memory.read_virt(pointer, &mut data) != length {
        return Err(998);
    }
    Ok(data)
}
fn output(image: &mut GuestImage, pointer: u64, length: u64, data: &[u8]) -> Result<(), u32> {
    if length == 0 {
        return Err(87);
    }
    let size = word(&mut image.memory, length, 4)? as usize;
    if !image.memory.probe_write(length, 4) {
        return Err(998);
    }
    if pointer != 0 && size >= data.len() && !image.memory.probe_write(pointer, data.len()) {
        return Err(998);
    }
    put(&mut image.memory, length, data.len() as u64, 4)?;
    if pointer == 0 {
        return Ok(());
    }
    if size < data.len() {
        return Err(234);
    }
    if !image.memory.write_virt(pointer, data) {
        return Err(998);
    }
    Ok(())
}
fn implementation(image: &mut GuestImage, name: &str, a: &[u64]) -> Result<u64, u32> {
    let width = if image.thread.context.mode == DecodeMode::X86 {
        4
    } else {
        8
    };
    match name {
        "CryptAcquireContextA" | "CryptAcquireContextW" => {
            if a[0] == 0 {
                return Err(87);
            }
            if !image.memory.probe_write(a[0], width) {
                return Err(998);
            }
            let flags = a[4] as u32;
            if flags & !(0xf000_0000 | 0x40) != 0 || flags & 0xf000_0000 != 0xf000_0000 {
                return Err(if flags == 0 { BAD_KEYSET } else { BAD_FLAGS });
            }
            let wide = name.ends_with('W');
            if a[1] != 0 && !string(&mut image.memory, a[1], wide)?.is_empty() {
                return Err(87);
            }
            if a[2] != 0 {
                let provider = string(&mut image.memory, a[2], wide)?;
                if !provider.is_empty() && !provider.eq_ignore_ascii_case(PROVIDER_NAME) {
                    return Err(KEYSET_NOT_DEF);
                }
            }
            if !matches!(a[3] as u32, 1 | 24) {
                return Err(BAD_PROV_TYPE);
            }
            let state = &mut image.win32.advapi.crypto;
            if state.providers.len() >= 256 {
                return Err(NO_MEMORY);
            }
            state.providers.try_reserve(1).map_err(|_| NO_MEMORY)?;
            let handle = state.next_handle()?;
            state.providers.push(Provider {
                handle,
                kind: a[3] as u32,
                refs: 1,
            });
            put(&mut image.memory, a[0], handle, width)?;
        }
        "CryptContextAddRef" => {
            if a[1] != 0 {
                return Err(87);
            }
            if a[2] as u32 != 0 {
                return Err(BAD_FLAGS);
            }
            let provider = image
                .win32
                .advapi
                .crypto
                .providers
                .iter_mut()
                .find(|p| p.handle == a[0])
                .ok_or(BAD_UID)?;
            provider.refs = provider.refs.checked_add(1).ok_or(NO_MEMORY)?;
        }
        "CryptReleaseContext" => {
            if a[1] as u32 != 0 {
                return Err(BAD_FLAGS);
            }
            let state = &mut image.win32.advapi.crypto;
            let index = state
                .providers
                .iter()
                .position(|p| p.handle == a[0])
                .ok_or(BAD_UID)?;
            if state.providers[index].refs > 1 {
                state.providers[index].refs -= 1;
            } else {
                state.providers.remove(index);
            }
        }
        "CryptCreateHash" => {
            if a[4] == 0 {
                return Err(87);
            }
            if !image.memory.probe_write(a[4], width) {
                return Err(998);
            }
            let state = &mut image.win32.advapi.crypto;
            let kind = state.provider(a[0])?.kind;
            let algorithm = a[1] as u32;
            if hash_size(algorithm).is_none()
                || (kind == 1 && !matches!(algorithm, 0x8003 | 0x8004))
            {
                return Err(BAD_ALGID);
            }
            if a[2] != 0 {
                return Err(BAD_KEY);
            }
            if a[3] as u32 & !1 != 0 {
                return Err(BAD_FLAGS);
            }
            if state.hashes.len() >= 512 {
                return Err(NO_MEMORY);
            }
            state.hashes.try_reserve(1).map_err(|_| NO_MEMORY)?;
            let handle = state.next_handle()?;
            state.hashes.push(Hash {
                handle,
                provider: a[0],
                algorithm,
                data: Vec::new(),
                digest: None,
            });
            put(&mut image.memory, a[4], handle, width)?;
        }
        "CryptHashData" => {
            let index = image.win32.advapi.crypto.hash_index(a[0])?;
            let state = &image.win32.advapi.crypto;
            state.provider(state.hashes[index].provider)?;
            if a[3] as u32 != 0 {
                return Err(BAD_FLAGS);
            }
            if state.hashes[index].digest.is_some() {
                return Err(BAD_HASH_STATE);
            }
            let len = a[2] as u32 as usize;
            if len > MAX_HASH_BYTES
                || state.hashes[index]
                    .data
                    .len()
                    .checked_add(len)
                    .filter(|n| *n <= MAX_HASH_BYTES)
                    .is_none()
                || state
                    .bytes()
                    .checked_add(len)
                    .filter(|n| *n <= MAX_TOTAL_BYTES)
                    .is_none()
            {
                return Err(NO_MEMORY);
            }
            let data = if len == 0 {
                Vec::new()
            } else {
                read_bytes(image, a[1], len)?
            };
            let hash = &mut image.win32.advapi.crypto.hashes[index];
            hash.data.try_reserve(data.len()).map_err(|_| NO_MEMORY)?;
            hash.data.extend_from_slice(&data);
        }
        "CryptGetHashParam" => {
            let index = image.win32.advapi.crypto.hash_index(a[0])?;
            let state = &image.win32.advapi.crypto;
            let context = &state.hashes[index];
            state.provider(context.provider)?;
            if a[4] as u32 != 0 {
                return Err(BAD_FLAGS);
            }
            let parameter = a[1] as u32;
            let data = match parameter {
                1 => context.algorithm.to_le_bytes().to_vec(),
                4 => (hash_size(context.algorithm).ok_or(BAD_ALGID)? as u32)
                    .to_le_bytes()
                    .to_vec(),
                2 => match &context.digest {
                    Some(value) => copy_bytes(value)?,
                    None => digest(context.algorithm, &context.data),
                },
                _ => return Err(BAD_TYPE),
            };
            output(image, a[2], a[3], &data)?;
            if parameter == 2 && a[2] != 0 {
                let context = &mut image.win32.advapi.crypto.hashes[index];
                context.digest = Some(data);
                context.data = Vec::new();
            }
        }
        "CryptSetHashParam" => {
            let state = &image.win32.advapi.crypto;
            let index = state.hash_index(a[0])?;
            let context = &state.hashes[index];
            state.provider(context.provider)?;
            if a[3] as u32 != 0 {
                return Err(BAD_FLAGS);
            }
            if a[1] as u32 != 2 {
                return Err(BAD_TYPE);
            }
            if context.digest.is_some() {
                return Err(BAD_HASH_STATE);
            }
            let size = hash_size(context.algorithm).ok_or(BAD_ALGID)?;
            let data = read_bytes(image, a[2], size)?;
            let context = &mut image.win32.advapi.crypto.hashes[index];
            context.digest = Some(data);
            context.data = Vec::new();
        }
        "CryptDuplicateHash" => {
            if a[1] != 0 {
                return Err(87);
            }
            if a[2] as u32 != 0 {
                return Err(BAD_FLAGS);
            }
            if a[3] == 0 {
                return Err(87);
            }
            if !image.memory.probe_write(a[3], width) {
                return Err(998);
            }
            let state = &mut image.win32.advapi.crypto;
            let index = state.hash_index(a[0])?;
            state.provider(state.hashes[index].provider)?;
            if state.hashes.len() >= 512
                || state.bytes() + state.hashes[index].data.len() > MAX_TOTAL_BYTES
            {
                return Err(NO_MEMORY);
            }
            let source = &state.hashes[index];
            let mut copy = Hash {
                handle: 0,
                provider: source.provider,
                algorithm: source.algorithm,
                data: copy_bytes(&source.data)?,
                digest: source.digest.as_deref().map(copy_bytes).transpose()?,
            };
            state.hashes.try_reserve(1).map_err(|_| NO_MEMORY)?;
            let handle = state.next_handle()?;
            copy.handle = handle;
            state.hashes.push(copy);
            put(&mut image.memory, a[3], handle, width)?;
        }
        "CryptDestroyHash" => {
            let state = &mut image.win32.advapi.crypto;
            let index = state.hash_index(a[0])?;
            state.hashes.remove(index);
        }
        "CryptGetProvParam" => {
            let state = &image.win32.advapi.crypto;
            let provider = state.provider(a[0])?;
            if a[4] as u32 != 0 {
                return Err(BAD_FLAGS);
            }
            let data = match a[1] as u32 {
                4 => PROVIDER_NAME.bytes().chain(core::iter::once(0)).collect(),
                5 => 0x100u32.to_le_bytes().to_vec(),
                6 => Vec::from([0]),
                3 => 2u32.to_le_bytes().to_vec(), // CRYPT_IMPL_SOFTWARE
                16 => provider.kind.to_le_bytes().to_vec(),
                _ => return Err(BAD_TYPE),
            };
            output(image, a[2], a[3], &data)?;
        }
        "CryptGenRandom" => {
            image.win32.advapi.crypto.provider(a[0])?;
            return Err(FAIL); // No cryptographic entropy backend in this gateway.
        }
        _ => return Err(120),
    }
    Ok(1)
}
pub fn invoke(image: &mut GuestImage, name: &str, a: &[u64]) -> Option<Result<u64, u32>> {
    if !matches!(
        name,
        "CryptAcquireContextA"
            | "CryptAcquireContextW"
            | "CryptContextAddRef"
            | "CryptReleaseContext"
            | "CryptCreateHash"
            | "CryptHashData"
            | "CryptGetHashParam"
            | "CryptSetHashParam"
            | "CryptDuplicateHash"
            | "CryptDestroyHash"
            | "CryptGetProvParam"
            | "CryptGenRandom"
    ) {
        return None;
    }
    Some(implementation(image, name, a))
}
