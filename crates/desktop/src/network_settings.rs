//! Network and Sharing Center backed by the active miniports and IPv4 stack.
use crate::{
    compositor::aero,
    graphics::{Rect, Surface},
    platform::services,
};
use alloc::{format, string::String, vec::Vec};
use nt61_abi::{desktop_bridge::STATUS_INVALID_PARAMETER, session_bridge::NETWORK};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Adapter {
    pub index: u32,
    pub enabled: bool,
    pub configured: bool,
    pub mtu: u32,
    pub address: u32,
    pub mask: u32,
    pub gateway: u32,
    pub dns: u32,
    pub mac: [u8; 6],
    pub name: String,
}
fn decode(bytes: &[u8]) -> Result<Vec<Adapter>, u32> {
    if bytes.len() < 8 || &bytes[..4] != b"NET1" {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let count = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    if count > 32 || bytes.len() != 8 + count * 50 {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let mut adapters = Vec::new();
    for r in bytes[8..].chunks_exact(50) {
        let word = |n| u32::from_le_bytes(r[n..n + 4].try_into().unwrap());
        if word(4) & !3 != 0 || adapters.iter().any(|a: &Adapter| a.index == word(0)) {
            return Err(STATUS_INVALID_PARAMETER);
        }
        let n = r[34..50].iter().position(|b| *b == 0).unwrap_or(16);
        let name = core::str::from_utf8(&r[34..34 + n]).map_err(|_| STATUS_INVALID_PARAMETER)?;
        adapters.push(Adapter {
            index: word(0),
            enabled: word(4) & 1 != 0,
            configured: word(4) & 2 != 0,
            mtu: word(8),
            address: word(12),
            mask: word(16),
            gateway: word(20),
            dns: word(24),
            mac: r[28..34].try_into().unwrap(),
            name: name.into(),
        });
    }
    Ok(adapters)
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub adapters: Vec<Adapter>,
    pub selected: usize,
    pub message: String,
}
impl Settings {
    pub fn refresh(&mut self) {
        match services::transfer(NETWORK, "", &[]).and_then(|bytes| decode(&bytes)) {
            Ok(adapters) => {
                self.adapters = adapters;
                self.selected = self.selected.min(self.adapters.len().saturating_sub(1));
                self.message.clear();
            }
            Err(code) => {
                self.message = format!("Cannot read network configuration (0x{code:08X}).")
            }
        }
    }
    pub fn click(&mut self, b: Rect, x: i32, y: i32) -> bool {
        if !b.contains(x, y) {
            return false;
        }
        if button(b, 0).contains(x, y) {
            self.refresh();
        }
        if button(b, 1).contains(x, y) {
            self.message = match services::transfer(NETWORK, "flush-dns", &[]) {
                Ok(_) => "Successfully flushed the DNS Resolver Cache.".into(),
                Err(code) => format!("Cannot flush DNS (0x{code:08X})."),
            };
        }
        for next in [false, true] {
            if nav(b, next).contains(x, y) && !self.adapters.is_empty() {
                self.selected = if next {
                    (self.selected + 1) % self.adapters.len()
                } else {
                    (self.selected + self.adapters.len() - 1) % self.adapters.len()
                };
            }
        }
        true
    }
}
fn ip(value: u32) -> String {
    let [a, b, c, d] = value.to_be_bytes();
    format!("{a}.{b}.{c}.{d}")
}
fn button(b: Rect, i: usize) -> Rect {
    Rect::new(
        b.x + 20 + i as i32 * 175,
        b.y + b.height as i32 - 80,
        165,
        27,
    )
}
fn nav(b: Rect, next: bool) -> Rect {
    Rect::new(
        b.x + b.width as i32 - if next { 55 } else { 95 },
        b.y + 50,
        36,
        26,
    )
}
pub fn draw(s: &Settings, f: &mut Surface, b: Rect, text: impl Fn(&mut Surface, &str, Rect, u32)) {
    f.fill_rect(b.x, b.y, b.width, b.height, 0xFFFFFFFF);
    let blue = 0xFF0066BB;
    let ink = 0xFF222222;
    text(
        f,
        "View your basic network information and set up connections",
        Rect::new(b.x + 20, b.y + 15, b.width.saturating_sub(40), 28),
        blue,
    );
    text(
        f,
        "View your active networks",
        Rect::new(b.x + 20, b.y + 52, b.width.saturating_sub(120), 25),
        blue,
    );
    if let Some(a) = s.adapters.get(s.selected) {
        for next in [false, true] {
            let r = nav(b, next);
            aero::blend_rounded_rect(f, r, 3, 0xFFE7F0F8);
            text(
                f,
                if next { ">" } else { "<" },
                Rect::new(r.x + 13, r.y + 5, 20, 20),
                ink,
            );
        }
        let [m0, m1, m2, m3, m4, m5] = a.mac;
        let rows = [
            format!("Local Area Connection {} ({})", a.index + 1, a.name),
            format!(
                "IPv4 connectivity: {}",
                if a.configured {
                    "Address configured"
                } else {
                    "No network address"
                }
            ),
            format!("IPv4 Address: {}", ip(a.address)),
            format!("IPv4 Subnet Mask: {}", ip(a.mask)),
            format!("IPv4 Default Gateway: {}", ip(a.gateway)),
            format!("IPv4 DNS Server: {}", ip(a.dns)),
            format!("Physical Address: {m0:02X}-{m1:02X}-{m2:02X}-{m3:02X}-{m4:02X}-{m5:02X}"),
            format!("MTU: {}", a.mtu),
        ];
        for (i, line) in rows.iter().enumerate() {
            text(
                f,
                line,
                Rect::new(
                    b.x + 28,
                    b.y + 91 + i as i32 * 22,
                    b.width.saturating_sub(56),
                    22,
                ),
                ink,
            );
        }
    } else {
        text(
            f,
            "No network adapter is installed.",
            Rect::new(b.x + 28, b.y + 100, b.width.saturating_sub(56), 28),
            ink,
        );
    }
    for (i, label) in ["Refresh", "Flush DNS cache"].iter().enumerate() {
        let r = button(b, i);
        aero::blend_rounded_rect(f, r, 3, 0xFFE6EEF5);
        text(
            f,
            label,
            Rect::new(r.x + 12, r.y + 6, r.width - 20, 22),
            ink,
        );
    }
    text(
        f,
        &s.message,
        Rect::new(
            b.x + 20,
            b.y + b.height as i32 - 44,
            b.width.saturating_sub(40),
            40,
        ),
        0xFF992222,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_malformed_adapter_snapshots() {
        let mut b = b"NET1".to_vec();
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&[0; 50]);
        assert_eq!(decode(&b).unwrap().len(), 1);
        for n in 0..b.len() {
            assert!(decode(&b[..n]).is_err());
        }
        b[12] = 4;
        assert!(decode(&b).is_err());
        b[12] = 0;
        let row = b[8..].to_vec();
        b[4..8].copy_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&row);
        assert!(decode(&b).is_err());
    }
}
