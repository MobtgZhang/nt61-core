use super::*;

const CENTER: u32 = 0x0001;
const RIGHT: u32 = 0x0002;
const VCENTER: u32 = 0x0004;
const BOTTOM: u32 = 0x0008;
const WORDBREAK: u32 = 0x0010;
const SINGLELINE: u32 = 0x0020;
const EXPANDTABS: u32 = 0x0040;
const TABSTOP: u32 = 0x0080;
const NOCLIP: u32 = 0x0100;
const EXTERNALLEADING: u32 = 0x0200;
const CALCRECT: u32 = 0x0400;
const NOPREFIX: u32 = 0x0800;
const EDITCONTROL: u32 = 0x2000;
const SUPPORTED: u32 = CENTER
    | RIGHT
    | VCENTER
    | BOTTOM
    | WORDBREAK
    | SINGLELINE
    | EXPANDTABS
    | TABSTOP
    | NOCLIP
    | EXTERNALLEADING
    | CALCRECT
    | NOPREFIX
    | EDITCONTROL;
const MAX_UNITS: usize = 8192;

pub(super) struct Line {
    pub units: Vec<u16>,
    width: i32,
}

fn line_layout(
    units: &[u16],
    flags: u32,
    available: i32,
    tab_chars: usize,
    mut measure: impl FnMut(&[u16]) -> Option<i32>,
) -> Option<Vec<Line>> {
    let single = flags & SINGLELINE != 0;
    let wrap = !single && flags & WORDBREAK != 0;
    let mut lines = Vec::new();
    let mut line = Line {
        units: Vec::new(),
        width: 0,
    };
    let mut pending_spaces = 0usize;
    let mut position = 0;
    while position < units.len() {
        match units[position] {
            13 | 10 if !single => {
                if units[position] == 13 && units.get(position + 1) == Some(&10) {
                    position += 1;
                }
                lines.push(core::mem::replace(
                    &mut line,
                    Line {
                        units: Vec::new(),
                        width: 0,
                    },
                ));
                pending_spaces = 0;
                position += 1;
            }
            32 | 13 | 10 => {
                pending_spaces += 1;
                position += 1;
            }
            9 if flags & EXPANDTABS != 0 => {
                pending_spaces += tab_chars - (line.units.len() + pending_spaces) % tab_chars;
                position += 1;
            }
            _ => {
                let start = position;
                while position < units.len() {
                    let unit = units[position];
                    if matches!(unit, 10 | 13 | 32) || (unit == 9 && flags & EXPANDTABS != 0) {
                        break;
                    }
                    position += 1;
                }
                let mut token = Vec::with_capacity(position - start);
                let mut offset = start;
                while offset < position {
                    if units[offset] == 38 && flags & NOPREFIX == 0 && offset + 1 < position {
                        offset += 1;
                    }
                    token.push(units[offset]);
                    offset += 1;
                }
                let token_width = measure(&token)?;
                let spaces = vec![32; pending_spaces];
                let gap = measure(&spaces)?;
                if wrap
                    && !line.units.is_empty()
                    && line.width.saturating_add(gap).saturating_add(token_width) > available
                {
                    lines.push(core::mem::replace(
                        &mut line,
                        Line {
                            units: Vec::new(),
                            width: 0,
                        },
                    ));
                } else {
                    line.units.extend_from_slice(&spaces);
                    line.width = line.width.checked_add(gap)?;
                }
                line.width = line.width.checked_add(token_width)?;
                line.units.extend(token);
                pending_spaces = 0;
            }
        }
    }
    if !wrap && pending_spaces != 0 {
        let spaces = vec![32; pending_spaces];
        line.width = line.width.checked_add(measure(&spaces)?)?;
        line.units.extend(spaces);
    }
    if !line.units.is_empty() || lines.is_empty() || !matches!(units.last(), Some(10 | 13)) {
        lines.push(line);
    }
    Some(lines)
}

fn capture(memory: &mut dyn GuestMemory, address: u64, count: i32) -> Result<Vec<u16>, u32> {
    if count == -1 {
        let mut units = Vec::new();
        for index in 0..=MAX_UNITS {
            let pointer = address.checked_add(index as u64 * 2).ok_or(NOACCESS)?;
            let unit = word(memory, pointer, 2)? as u16;
            if unit == 0 {
                return Ok(units);
            }
            units.push(unit);
        }
        return Err(INVALID_PARAMETER);
    }
    let count = usize::try_from(count).map_err(|_| INVALID_PARAMETER)?;
    if count > MAX_UNITS {
        return Err(INVALID_PARAMETER);
    }
    Ok(read(memory, address, count * 2)?
        .chunks_exact(2)
        .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
        .collect())
}

pub(super) fn invoke(
    image: &mut GuestImage,
    args: &[u64],
    services: &mut dyn GuestServices,
) -> Result<u64, ApiError> {
    let [dc, text, count, rect, format, params] =
        *<&[u64; 6]>::try_from(args.get(..6).ok_or(INVALID_PARAMETER)?)
            .map_err(|_| INVALID_PARAMETER)?;
    let flags = format as u32;
    if flags & !SUPPORTED != 0 {
        return Err(120u32.into());
    }
    let units = capture(&mut image.memory, text, count as i32)?;
    if units.is_empty() {
        return Ok(0);
    }
    let rectangle = read(&mut image.memory, rect, 16)?;
    let read_i32 =
        |bytes: &[u8], offset| i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let [left, top, right, bottom] = [0, 4, 8, 12].map(|offset| read_i32(&rectangle, offset));
    let (tab_chars, left_margin, right_margin) = if params != 0 {
        let parameters = read(&mut image.memory, params, 20)?;
        if read_i32(&parameters, 0) != 20 || !image.memory.probe_write(params, 20) {
            return Err(INVALID_PARAMETER.into());
        }
        (
            read_i32(&parameters, 4),
            read_i32(&parameters, 8),
            read_i32(&parameters, 12),
        )
    } else {
        (0, 0, 0)
    };
    if flags & CALCRECT != 0 && !image.memory.probe_write(rect, 16) {
        return Err(NOACCESS.into());
    }
    let tab_chars = if flags & TABSTOP != 0 && tab_chars != 0 {
        tab_chars.clamp(1, 32)
    } else {
        8
    } as usize;
    let printer = printing::owns_dc(image, dc);
    let (height, ascent, leading, average) = if printer {
        (16, 12, 0, 10)
    } else {
        let metrics = services.text_metrics_w(dc).ok_or(6u32)?;
        (
            read_i32(&metrics, 0),
            read_i32(&metrics, 4),
            read_i32(&metrics, 16),
            read_i32(&metrics, 20),
        )
    };
    let line_height = height
        .saturating_add(if flags & EXTERNALLEADING != 0 {
            leading
        } else {
            0
        })
        .max(1);
    let left_margin = average.saturating_mul(left_margin);
    let right_margin = average.saturating_mul(right_margin);
    let margins = left_margin.saturating_add(right_margin);
    let available = right.saturating_sub(left).saturating_sub(margins).max(0);
    let lines = line_layout(&units, flags, available, tab_chars, |units| {
        if units.is_empty() {
            return Some(0);
        }
        if printer {
            return Some((units.len() as i32 * 48 + 2) / 5);
        }
        services.text_extent_w(dc, units).map(|size| size[0])
    })
    .ok_or(INVALID_PARAMETER)?;
    let total_height = line_height.saturating_mul(lines.len() as i32);
    if flags & CALCRECT != 0 {
        let maximum = lines.iter().map(|line| line.width).max().unwrap_or(0);
        let width = if flags & SINGLELINE != 0 || maximum > available {
            maximum
        } else {
            available
        };
        put(
            &mut image.memory,
            rect + 8,
            left.saturating_add(width).saturating_add(margins) as u32 as u64,
            4,
        )?;
        put(
            &mut image.memory,
            rect + 12,
            top.saturating_add(total_height) as u32 as u64,
            4,
        )?;
    } else {
        let mut line_top = if flags & SINGLELINE != 0 && flags & BOTTOM != 0 {
            bottom.saturating_sub(line_height)
        } else if flags & SINGLELINE != 0 && flags & VCENTER != 0 {
            top.saturating_add(bottom.saturating_sub(top).saturating_sub(line_height) / 2)
        } else {
            top
        };
        let mut fragments = Vec::new();
        for line in &lines {
            if flags & EDITCONTROL != 0
                && flags & NOCLIP == 0
                && line_top.saturating_add(line_height) > bottom
            {
                break;
            }
            let remaining = available.saturating_sub(line.width);
            let offset = if flags & RIGHT != 0 {
                remaining
            } else if flags & CENTER != 0 {
                remaining / 2
            } else {
                0
            };
            let x = left.saturating_add(left_margin).saturating_add(offset);
            let y = if printer {
                line_top.saturating_add(ascent)
            } else {
                line_top
            };
            fragments.push((x, y, line.units.as_slice()));
            line_top = line_top.saturating_add(line_height);
        }
        let clip = (flags & NOCLIP == 0).then_some([left, top, right, bottom]);
        if printer {
            printing::draw_layout(image, dc, clip, &fragments)?;
        } else if !services.draw_text_layout(dc, clip, &fragments) {
            return Err(120u32.into());
        }
    }
    if params != 0 {
        put(&mut image.memory, params + 16, units.len() as u64, 4)?;
    }
    Ok(total_height as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_at_spaces_and_preserves_explicit_blank_lines() {
        let input = "ab cd\r\n\r\nef".encode_utf16().collect::<Vec<_>>();
        let lines = line_layout(&input, WORDBREAK, 4, 8, |text| Some(text.len() as i32)).unwrap();
        let actual = lines
            .iter()
            .map(|line| String::from_utf16(&line.units).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(actual, ["ab", "cd", "", "ef"]);
    }
}
