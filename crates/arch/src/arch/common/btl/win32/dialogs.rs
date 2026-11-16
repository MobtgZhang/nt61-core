//! Win32 structure conversion at the isolated guest boundary.
use super::*;
use crate::arch::common::btl::dialog_wire as w;

pub(super) fn modal(
    image: &mut GuestImage,
    name: &str,
    address: u64,
    services: &mut dyn GuestServices,
) -> Result<bool, u32> {
    let x86 = image.thread.context.mode == DecodeMode::X86;
    let width = if x86 { 4 } else { 8 };
    let wide = name.ends_with('W');
    let settings=if name.starts_with("PrintDlg")||name.starts_with("PageSetupDlg"){Some(super::printing::inputs(image,address,x86,wide)?)}else{None};
    let m = &mut image.memory;
    let get = |m: &mut dyn GuestMemory, a: u64, n: usize| word(m, a, n).map_err(|_| 2u32);
    let mut p = w::EMPTY;
    let (kind, flags_offset) = if name.starts_with("GetOpenFileName") {
        (w::OPEN, if x86 { 52 } else { 96 })
    } else if name.starts_with("GetSaveFileName") {
        (w::SAVE, if x86 { 52 } else { 96 })
    } else if name.starts_with("ChooseFont") {
        (w::FONT, if x86 { 20 } else { 36 })
    } else if name.starts_with("ChooseColor") {
        (w::COLOR, if x86 { 20 } else { 40 })
    } else if name.starts_with("PrintDlg") {
        (w::PRINT, if x86 { 20 } else { 40 })
    } else {
        (w::PAGE, if x86 { 16 } else { 32 })
    };
    p.values[0] = kind;
    p.values[60] = get(m, address + if x86 { 4 } else { 8 }, width)? as u32;
    p.values[3] = get(m, address + flags_offset, 4)? as u32;
    // Custom hooks and templates execute guest code; the copied desktop protocol
    // cannot execute those callbacks. Refuse instead of silently skipping them.
    let unsupported = match kind {
        w::OPEN | w::SAVE => 0x20 | 0x40 | 0x80 | 0x200,
        w::FONT => 8 | 0x10 | 0x20,
        w::COLOR => 0x10 | 0x20 | 0x40,
        w::PRINT => 0x1000 | 0x2000 | 0x4000 | 0x8000 | 0x10000 | 0x20000,
        _ => 0x2000 | 0x4000 | 0x8000 | 0x10000 | 0x40000,
    };
    if p.values[3] & unsupported != 0 {
        return Err(2);
    }
    let mut output = 0;
    let mut capacity = 0;
    if matches!(kind, w::OPEN | w::SAVE) {
        output = get(m, address + if x86 { 28 } else { 48 }, width)?;
        capacity = get(m, address + if x86 { 32 } else { 56 }, 4)? as usize;
        let unit=if wide{2}else{1};
        if !(0..capacity).any(|i|word(m,output+i as u64*unit,unit as usize).is_ok_and(|v|v==0)){return Err(2);}
        let file = string(m, output, wide).map_err(|_| 2u32)?;
        if !p.set_string(0, &file) {
            return Err(2);
        }
        for (slot, offset) in [
            (1, if x86 { 48 } else { 88 }),
            (3, if x86 { 60 } else { 104 }),
        ] {
            let pointer = get(m, address + offset, width)?;
            if pointer != 0 && !p.set_string(slot, &string(m, pointer, wide).map_err(|_| 2u32)?) {
                return Err(2);
            }
        }
        let directory = get(m, address + if x86 { 44 } else { 80 }, width)?;
        if directory != 0 && file.is_empty() {
            p.set_string(0, &string(m, directory, wide).map_err(|_| 2u32)?);
        }
        let filter = get(m, address + if x86 { 12 } else { 24 }, width)?;
        p.values[4] = get(m, address + if x86 { 24 } else { 44 }, 4)? as u32;
        if filter != 0 {
            let mut at = filter;
            let mut used = 0;
            loop {
                let value = string(m, at, wide).map_err(|_| 2u32)?;
                if used + value.len() + 1 >= 1024 {
                    return Err(2);
                }
                p.text[2][used..used + value.len()].copy_from_slice(value.as_bytes());
                used += value.len() + 1;
                at += if wide {
                    (value.encode_utf16().count() + 1) * 2
                } else {
                    value.len() + 1
                } as u64;
                if value.is_empty() {
                    break;
                }
            }
        }
    } else if kind == w::COLOR {
        p.values[4] = get(m, address + if x86 { 12 } else { 24 }, 4)? as u32;
        let colors = get(m, address + if x86 { 16 } else { 32 }, width)?;
        for i in 0..16 {
            p.values[8 + i] = get(m, colors + i as u64 * 4, 4)? as u32;
        }
    } else if kind == w::FONT {
        let font = get(m, address + if x86 { 12 } else { 24 }, width)?;
        let flags = p.values[3];
        if flags & 0x40 != 0 {
            let height = get(m, font, 4)? as u32 as i32;
            p.values[4] = height
                .unsigned_abs()
                .saturating_mul(72)
                .div_ceil(96)
                .clamp(1, 144);
            p.values[5] =
                u32::from(get(m, font + 16, 4)? >= 700) | ((get(m, font + 20, 1)? as u32 & 1) << 1);
            p.values[6] = get(m, font + 21, 1)? as u32;
            p.values[7] = get(m, font + 22, 1)? as u32;
            let face = string(m, font + 28, wide).map_err(|_| 2u32)?;
            p.set_string(2, &face);
        } else {
            p.values[4] = 10;
            p.set_string(2, "Consolas");
        }
        p.values[8] = get(m, address + if x86 { 24 } else { 40 }, 4)? as u32;
    } else if kind == w::PAGE {
        let s=settings.unwrap();p.values[12]=u32::from(s.orientation==2);p.values[13]=u32::from(s.paper==1);
        let offset = if x86 { 44 } else { 60 };
        for i in 0..4 {
            p.values[8 + i] = get(m, address + offset + i as u64 * 4, 4)? as u32;
        }
    } else {
        if !services.printer_available() {
            return Err(0x1008);
        }
        p.values[4] = get(m, address + if x86 { 24 } else { 44 }, 2)? as u32;
        p.values[5] = get(m, address + if x86 { 26 } else { 46 }, 2)? as u32;
        p.values[6] = get(m, address + if x86 { 32 } else { 52 }, 2)? as u32;
        let min=get(m,address+if x86{28}else{48},2)?as u32;let max=get(m,address+if x86{30}else{50},2)?as u32;
        p.values[14]=min;p.values[15]=max;
        if p.values[3]&0x40000!=0{p.values[6]=settings.unwrap().copies as u32;}
        if p.values[3]&2!=0&&(p.values[4]<min||p.values[5]<p.values[4]||p.values[5]>max){return Err(0x1002);}
    }
    let mut bytes = alloc::vec![0;w::PACKET_SIZE];
    p.encode(&mut bytes);
    let reply = if matches!(kind,w::PRINT|w::PAGE) && p.values[3] & 0x400 != 0 {
        if !services.printer_available(){return Err(0x1008);}
        if kind==w::PAGE{let s=settings.unwrap();let(mut width,mut height)=if s.paper==1{(215.9,279.4)}else{(210.0,297.0)};if s.orientation==2{core::mem::swap(&mut width,&mut height);}let scale=if p.values[3]&8!=0{100.0}else{1000.0/25.4};p.values[4]=(width*scale+0.5)as u32;p.values[5]=(height*scale+0.5)as u32;}
        p.values[1] = w::ACCEPTED;
        p
    } else {
        let bytes = services.desktop_dialog(&bytes).map_err(|_| 0xffffu32)?;
        w::Packet::decode(&bytes).ok_or(0xffffu32)?
    };
    if reply.values[0] != kind {
        return Err(0xffff);
    }
    match reply.values[1] {
        w::CANCELLED => return Ok(false),
        w::FAILED => return Err(reply.values[2].max(1)),
        w::ACCEPTED => {}
        _ => return Err(0xffff),
    }
    if matches!(kind, w::OPEN | w::SAVE) {
        let path = reply.string(0).ok_or(0xffffu32)?;
        if !wide && !path.is_ascii() {
            return Err(2);
        }
        let needed = if wide {
            path.encode_utf16().count() + 1
        } else {
            path.len() + 1
        };
        if needed > capacity {
            put(m, output, needed.min(65535) as u64, 2).map_err(|_| 2u32)?;
            return Err(0x3003);
        }
        let title = path.rsplit(['\\', '/']).next().ok_or(0xffffu32)?;
        let title_pointer = get(m, address + if x86 { 36 } else { 64 }, width)?;
        let title_capacity = get(m, address + if x86 { 40 } else { 72 }, 4)? as usize;
        let title_length = if wide {
            title.encode_utf16().count() + 1
        } else {
            title.len() + 1
        };
        if title_pointer != 0
            && !m.probe_write(title_pointer, title_capacity * if wide { 2 } else { 1 })
        {
            return Err(2);
        }
        write_string(m, output, path, wide).map_err(|_| 2u32)?;
        if title_pointer != 0 && title_length <= title_capacity {
            write_string(m, title_pointer, title, wide).map_err(|_| 2u32)?;
        }
        let prefix = &path[..path.len() - title.len()];
        let offset = if wide {
            prefix.encode_utf16().count()
        } else {
            prefix.len()
        };
        let ext = title
            .rfind('.')
            .map(|i| {
                offset
                    + if wide {
                        title[..i + 1].encode_utf16().count()
                    } else {
                        i + 1
                    }
            })
            .unwrap_or(0);
        put(m, address + if x86 { 56 } else { 100 }, offset as u64, 2).map_err(|_| 2u32)?;
        put(m, address + if x86 { 58 } else { 102 }, ext as u64, 2).map_err(|_| 2u32)?;
        put(
            m,
            address + if x86 { 24 } else { 44 },
            reply.values[4] as u64,
            4,
        )
        .map_err(|_| 2u32)?;
    } else if kind == w::COLOR {
        let colors = get(m, address + if x86 { 16 } else { 32 }, width)?;
        put(
            m,
            address + if x86 { 12 } else { 24 },
            reply.values[4] as u64,
            4,
        )
        .map_err(|_| 2u32)?;
        for i in 0..16 {
            put(m, colors + i as u64 * 4, reply.values[8 + i] as u64, 4).map_err(|_| 2u32)?;
        }
    } else if kind == w::FONT {
        let font = get(m, address + if x86 { 12 } else { 24 }, width)?;
        let face = reply.string(2).ok_or(0xffffu32)?;
        if reply.values[4] == 0
            || reply.values[4] > 144
            || face.encode_utf16().count() > 31
            || (!wide && !face.is_ascii())
        {
            return Err(0xffff);
        }
        put(
            m,
            font,
            (-(reply.values[4] as i32 * 96 / 72)) as u32 as u64,
            4,
        )
        .map_err(|_| 2u32)?;
        put(
            m,
            font + 16,
            if reply.values[5] & 1 != 0 { 700 } else { 400 },
            4,
        )
        .map_err(|_| 2u32)?;
        for (i, v) in [
            (20, (reply.values[5] >> 1) & 1),
            (21, reply.values[6]),
            (22, reply.values[7]),
        ] {
            put(m, font + i, v as u64, 1).map_err(|_| 2u32)?;
        }
        write_string(m, font + 28, face, wide).map_err(|_| 2u32)?;
        put(m,address+if x86{24}else{40},reply.values[8]as u64,4).map_err(|_|2u32)?;
        put(
            m,
            address + if x86 { 16 } else { 32 },
            reply.values[4] as u64 * 10,
            4,
        )
        .map_err(|_| 2u32)?;
    } else if kind == w::PAGE {
        for i in 0..4 {
            put(
                m,
                address + if x86 { 44 } else { 60 } + i as u64 * 4,
                reply.values[8 + i] as u64,
                4,
            )
            .map_err(|_| 2u32)?;
        }
        for i in 0..2 {
            put(
                m,
                address + if x86 { 20 } else { 36 } + i as u64 * 4,
                reply.values[4 + i] as u64,
                4,
            )
            .map_err(|_| 2u32)?;
        }
    }
    if matches!(kind,w::PRINT|w::PAGE) {
        super::printing::dialog_result(image, address, x86, wide, &reply)?;
    }
    Ok(true)
}
