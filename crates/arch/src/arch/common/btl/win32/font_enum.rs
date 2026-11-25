use super::*;

pub(super) struct Enumeration {
    fonts: alloc::vec::IntoIter<nt61::FontEnumeration>,
    procedure: u64,
    parameter: u64,
    buffer: u64,
}

const LOGICAL_SIZE: usize = 92;
const METRIC_SIZE: usize = 60;
const TRUETYPE_FONTTYPE: u64 = 4;

impl Enumeration {
    fn prepare(&mut self, image: &mut GuestImage) -> Result<Option<(u64, [u64; 4])>, u32> {
        let Some((logical, metrics)) = self.fonts.next() else { return Ok(None); };
        let mut bytes = [0; LOGICAL_SIZE + METRIC_SIZE];
        bytes[..LOGICAL_SIZE].copy_from_slice(&logical);
        bytes[LOGICAL_SIZE..].copy_from_slice(&metrics);
        if !image.memory.write_virt(self.buffer, &bytes) { return Err(NOACCESS); }
        Ok(Some((self.procedure, [
            self.buffer, self.buffer + LOGICAL_SIZE as u64, TRUETYPE_FONTTYPE, self.parameter,
        ])))
    }
}

pub(super) fn start(image: &mut GuestImage, args: &[u64], services: &mut dyn GuestServices) -> Result<u64, ApiError> {
    if args[2] == 0 || image.memory.fetch_virt(args[2], &mut [0]) != 1 { return Ok(0); }
    let family = if args[1] == 0 { None } else {
        let Ok(name) = string(&mut image.memory, args[1], true) else { return Ok(0); };
        Some(name)
    };
    let Some(fonts) = services.enumerate_fonts_w(args[0], family.as_deref()) else { return Ok(0); };
    if fonts.is_empty() { return Ok(0); }
    let buffer = heap_allocate(image, (LOGICAL_SIZE + METRIC_SIZE) as u64)?;
    let mut enumeration = Enumeration {
        fonts: fonts.into_iter(), procedure: args[2], parameter: args[3], buffer,
    };
    match enumeration.prepare(image) {
        Ok(Some(callback)) => {
            image.win32.local.user.completion = Some(user::Completion::EnumFont(enumeration));
            image.win32.local.user.callback = Some(callback);
            Ok(0)
        }
        result => {
            heap_free(image, buffer)?;
            result?;
            Ok(0)
        }
    }
}

pub(super) fn resume(image: &mut GuestImage, mut enumeration: Enumeration, result: u32) -> Result<Outcome, u32> {
    if result != 0 {
        if let Some((procedure, args)) = enumeration.prepare(image)? {
            image.win32.local.user.completion = Some(user::Completion::EnumFont(enumeration));
            user::enter_callback(image, procedure, args)?;
            return Ok(Outcome::Continue);
        }
    }
    heap_free(image, enumeration.buffer)?;
    Ok(Outcome::Return(u64::from(result)))
}
