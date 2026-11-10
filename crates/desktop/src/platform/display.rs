use crate::compositor::damage::DamageRect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenInfo {
    pub width: u32,
    pub height: u32,
    pub pitch: usize,
    pub bpp: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayError {
    Unsupported,
    OutOfBounds,
}

pub trait DisplayBackend {
    fn screen_info(&self) -> Result<ScreenInfo, DisplayError>;

    fn present(&mut self, pixels: &[u8], damaged: &[DamageRect]) -> Result<(), DisplayError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayCapabilities {
    pub format: PixelFormat,
    pub alignment: usize,
    pub max_width: u32,
    pub max_height: u32,
    pub supports_partial_present: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Bgra8888,
    Rgba8888,
}

pub fn query_display_capabilities(info: ScreenInfo) -> Result<DisplayCapabilities, DisplayError> {
    if info.width == 0 || info.height == 0 || info.bpp != 32 || info.pitch < info.width as usize * 4
    {
        return Err(DisplayError::Unsupported);
    }
    Ok(DisplayCapabilities {
        format: PixelFormat::Bgra8888,
        alignment: 4,
        max_width: info.width,
        max_height: info.height,
        supports_partial_present: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_capabilities_reject_empty_or_unsupported_screens() {
        let info = ScreenInfo {
            width: 1024,
            height: 768,
            pitch: 4096,
            bpp: 32,
        };
        assert!(query_display_capabilities(info).is_ok());
        assert_eq!(
            query_display_capabilities(ScreenInfo {
                width: 0,
                height: 768,
                pitch: 0,
                bpp: 32,
            }),
            Err(DisplayError::Unsupported)
        );
        assert_eq!(
            query_display_capabilities(ScreenInfo {
                width: 1024,
                height: 768,
                pitch: 4096,
                bpp: 24,
            }),
            Err(DisplayError::Unsupported)
        );
        assert_eq!(
            query_display_capabilities(ScreenInfo {
                width: 1024,
                height: 768,
                pitch: 4092,
                bpp: 32,
            }),
            Err(DisplayError::Unsupported)
        );
    }
}
