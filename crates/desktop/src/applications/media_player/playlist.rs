//! Windows Media playlist (SMIL) persistence; external network locations are never fetched.
use alloc::{string::String, vec::Vec};
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
pub fn encode<'a>(name: &str, paths: impl Iterator<Item = &'a str>) -> Vec<u8> {
    let mut xml=alloc::format!("<?wpl version=\"1.0\"?>\r\n<smil><head><meta name=\"Generator\" content=\"NT61 Rust Media Player\"/><title>{}</title></head><body><seq>\r\n",escaped(name));
    for path in paths {
        xml.push_str(&alloc::format!(
            "<media src=\"{}\"/>\r\n",
            escaped(&path.replace('/', "\\"))
        ));
    }
    xml.push_str("</seq></body></smil>\r\n");
    xml.into_bytes()
}
pub fn decode(bytes: &[u8]) -> Result<Vec<String>, &'static str> {
    if bytes.len() > 1024 * 1024 {
        return Err("The playlist is too large.");
    }
    let xml = core::str::from_utf8(bytes).map_err(|_| "The playlist is not UTF-8.")?;
    let doc = roxmltree::Document::parse(xml).map_err(|_| "The playlist is damaged.")?;
    if doc.root_element().tag_name().name() != "smil" {
        return Err("This is not a Windows Media playlist.");
    }
    let mut paths = Vec::new();
    for node in doc.descendants().filter(|n| n.has_tag_name("media")) {
        let path = node
            .attribute("src")
            .ok_or("A playlist item has no source.")?;
        if path.is_empty()
            || path.len() > 1024
            || paths.len() >= 2048
            || path.contains('\0')
            || path.contains("://")
            || path.starts_with("\\\\")
        {
            return Err("Only local playlist files are supported.");
        }
        paths.push(path.into());
    }
    Ok(paths)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wpl_round_trip_preserves_paths_and_rejects_network_and_entities() {
        let bytes = encode(
            "A & B",
            ["C:/Music/A & B.mp3", "C:/Music/曲.wav"].into_iter(),
        );
        assert_eq!(
            decode(&bytes).unwrap(),
            alloc::vec!["C:\\Music\\A & B.mp3", "C:\\Music\\曲.wav"]
        );
        assert!(decode(b"<smil><media src='http://example/a.mp3'/></smil>").is_err());
        assert!(
            decode(b"<!DOCTYPE smil [<!ENTITY x SYSTEM 'file:///secret'>]><smil>&x;</smil>")
                .is_err()
        );
    }
}
