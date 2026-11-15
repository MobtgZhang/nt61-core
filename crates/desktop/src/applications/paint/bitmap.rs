//! Bounded BMP import and Windows-compatible bitmap export.
use super::*;
impl Paint {
    pub fn load_bmp(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        // Reject dimensions before allocating the decoder's RGBA buffer.
        if bytes.len() < 26 {
            return Err("Invalid bitmap header.");
        }
        let w = i32::from_le_bytes(bytes[18..22].try_into().unwrap());
        let h = i32::from_le_bytes(bytes[22..26].try_into().unwrap());
        if w <= 0
            || h == 0
            || (w as u32)
                .checked_mul(h.unsigned_abs())
                .is_none_or(|n| n > MAX_PIXELS)
        {
            return Err("Bitmap exceeds the supported 1,048,576 pixels.");
        }
        let image = crate::bmp::decode_bmp(bytes).map_err(|_| "Invalid or unsupported bitmap.")?;
        let mut canvas = Surface::new(image.width, image.height);
        for (y, row) in image
            .pixels
            .chunks_exact(image.width as usize * 4)
            .enumerate()
        {
            for (x, c) in row.chunks_exact(4).enumerate() {
                canvas.set_pixel(
                    x as u32,
                    y as u32,
                    u32::from_le_bytes([c[2], c[1], c[0], 0xff]),
                );
            }
        }
        self.canvas = canvas;
        self.undo.clear();
        self.redo.clear();
        self.last = None;
        self.origin = None;
        self.scroll = (0, 0);
        self.mark_saved();
        Ok(())
    }
}
/// Windows-compatible uncompressed bottom-up 32-bit BMP, including full file/DIB headers.
pub fn encode_bmp(surface: &Surface) -> Vec<u8> {
    let size = 54 + surface.pixels().len() * 4;
    let mut out = vec![0; 54];
    out[..2].copy_from_slice(b"BM");
    out[2..6].copy_from_slice(&(size as u32).to_le_bytes());
    out[10..14].copy_from_slice(&54u32.to_le_bytes());
    out[14..18].copy_from_slice(&40u32.to_le_bytes());
    out[18..22].copy_from_slice(&surface.width().to_le_bytes());
    out[22..26].copy_from_slice(&surface.height().to_le_bytes());
    out[26..28].copy_from_slice(&1u16.to_le_bytes());
    out[28..30].copy_from_slice(&32u16.to_le_bytes());
    for y in (0..surface.height()).rev() {
        for x in 0..surface.width() {
            out.extend_from_slice(&surface.pixel(x, y).unwrap_or(0).to_le_bytes());
        }
    }
    out
}
