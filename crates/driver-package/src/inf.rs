//! Syntax layer; keeps repeated directives and keyless list records in order.
use super::alloc::{string::String, vec::Vec};

const MAX_BYTES: usize = 1024 * 1024;
const MAX_LINE: usize = 4096;
const MAX_ENTRIES: usize = 32768;
const MAX_SECTIONS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Encoding,
    Limit,
    Syntax,
    MissingSection,
    MissingKey,
    Ambiguous,
    UndefinedString,
    StringCycle,
    InvalidVersion,
    InvalidPath,
    UnsupportedTarget,
    UnsupportedDirective,
    MissingDestination,
    NoMatch,
    Excluded,
    MissingInclude,
    DependencyCycle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub key: Option<String>,
    pub value: String,
    pub line: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub entries: Vec<Entry>,
}

pub struct Inf {
    sections: Vec<Section>,
}
impl Inf {
    /// Parse ASCII or BOM-marked UTF-16LE/BE. Non-ASCII legacy codepages and
    /// UTF-8 are deliberately rejected instead of being decoded as mojibake.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_BYTES {
            return Err(Error::Limit);
        }
        let text = if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
            if bytes.len() % 2 != 0 {
                return Err(Error::Encoding);
            }
            let little = bytes[0] == 0xff;
            let units: Vec<_> = bytes[2..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| {
                    if little {
                        u16::from_le_bytes([c[0], c[1]])
                    } else {
                        u16::from_be_bytes([c[0], c[1]])
                    }
                })
                .collect();
            String::from_utf16(&units).map_err(|_| Error::Encoding)?
        } else {
            if !bytes.is_ascii() {
                return Err(Error::Encoding);
            }
            String::from_utf8(bytes.to_vec()).map_err(|_| Error::Encoding)?
        };
        // SP1 KS.INF ends with a DOS EOF marker. Permit one terminal marker
        // followed only by whitespace; an embedded marker must not hide
        // additional directives from validation.
        let text = text
            .trim_end_matches(['\r', '\n', '\t', ' '])
            .strip_suffix('\u{1a}')
            .unwrap_or(&text);
        if text
            .chars()
            .any(|c| c == '\0' || (c.is_control() && !matches!(c, '\r' | '\n' | '\t')))
        {
            return Err(Error::Syntax);
        }
        let mut inf = Self {
            sections: Vec::new(),
        };
        let mut current = None;
        let mut logical = String::new();
        let mut continued = false;
        let mut first_line = 0;
        let mut count = 0;
        for (line_index, physical) in text.split_terminator('\n').enumerate() {
            if physical.len() > MAX_LINE {
                return Err(Error::Limit);
            }
            let clean = strip_comment(physical.trim_end_matches('\r'))?;
            let clean = clean.trim();
            if continued && clean.is_empty() {
                continue;
            }
            if logical.is_empty() {
                first_line = line_index + 1;
            }
            // Continuation backslash outside quotes joins the next line.
            let continuation = clean.ends_with('\\');
            continued = continuation;
            let fragment = if continuation {
                &clean[..clean.len() - 1]
            } else {
                clean
            };
            if logical.len() + fragment.len() > MAX_LINE {
                return Err(Error::Limit);
            }
            logical.push_str(fragment);
            if continuation {
                continue;
            }
            let line = logical.trim();
            if !line.is_empty() {
                if line.starts_with('[') {
                    if !line.ends_with(']')
                        || line.len() < 3
                        || line[1..line.len() - 1].contains(['[', ']'])
                    {
                        return Err(Error::Syntax);
                    }
                    let name = line[1..line.len() - 1].trim();
                    if name.is_empty() {
                        return Err(Error::Syntax);
                    }
                    current = if let Some(i) = inf
                        .sections
                        .iter()
                        .position(|s| s.name.eq_ignore_ascii_case(name))
                    {
                        // INF permits multiple occurrences; append in file order.
                        Some(i)
                    } else {
                        if inf.sections.len() >= MAX_SECTIONS {
                            return Err(Error::Limit);
                        }
                        inf.sections.push(Section {
                            name: String::from(name),
                            entries: Vec::new(),
                        });
                        Some(inf.sections.len() - 1)
                    };
                } else {
                    let index = current.ok_or(Error::Syntax)?;
                    count += 1;
                    if count > MAX_ENTRIES {
                        return Err(Error::Limit);
                    }
                    let equal = unquoted_equal(line);
                    let (key, value) = match equal {
                        Some(i) => {
                            let key = line[..i].trim();
                            if key.is_empty() {
                                return Err(Error::Syntax);
                            }
                            let fields = csv(key)?;
                            if fields.len() != 1 || fields[0].is_empty() {
                                return Err(Error::Syntax);
                            }
                            (Some(fields[0].clone()), String::from(line[i + 1..].trim()))
                        }
                        None => (None, String::from(line)),
                    };
                    // Validate quote placement now, even for unvisited sections.
                    csv(&value)?;
                    inf.sections[index].entries.push(Entry {
                        key,
                        value,
                        line: first_line,
                    });
                }
            }
            logical.clear();
        }
        if continued || !logical.is_empty() {
            return Err(Error::Syntax);
        }
        Ok(inf)
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }
    pub fn single(&self, section: &str, key: &str) -> Result<Option<&Entry>, Error> {
        let Some(section) = self.section(section) else {
            return Ok(None);
        };
        let mut matches = section.entries.iter().filter(|e| {
            e.key
                .as_deref()
                .is_some_and(|k| k.eq_ignore_ascii_case(key))
        });
        let result = matches.next();
        if matches.next().is_some() {
            return Err(Error::Ambiguous);
        }
        Ok(result)
    }
    pub fn fields(&self, entry: &Entry) -> Result<Vec<String>, Error> {
        // Split before expansion: a comma/semicolon/quote in a string token is
        // field content, never a new directive or source/destination argument.
        csv(&entry.value)?
            .into_iter()
            .map(|f| self.expand(&f))
            .collect()
    }
    pub fn one_field(&self, entry: &Entry) -> Result<String, Error> {
        let mut fields = self.fields(entry)?;
        if fields.len() != 1 {
            return Err(Error::Syntax);
        }
        Ok(fields.remove(0))
    }
    pub fn optional(&self, section: &str, key: &str) -> Result<Option<String>, Error> {
        self.single(section, key)?
            .map(|entry| self.one_field(entry))
            .transpose()
    }
    pub fn required(&self, section: &str, key: &str) -> Result<String, Error> {
        self.optional(section, key)?
            .filter(|s| !s.is_empty())
            .ok_or(Error::MissingKey)
    }
    pub fn expand(&self, value: &str) -> Result<String, Error> {
        let mut stack = Vec::new();
        let mut budget = 256;
        self.expand_inner(value, &mut stack, &mut budget)
    }
    fn expand_inner<'a>(
        &'a self,
        mut value: &str,
        stack: &mut Vec<&'a str>,
        budget: &mut usize,
    ) -> Result<String, Error> {
        if stack.len() >= 16 {
            return Err(Error::StringCycle);
        }
        let mut result = String::new();
        while let Some(start) = value.find('%') {
            append(&mut result, &value[..start])?;
            value = &value[start + 1..];
            if let Some(rest) = value.strip_prefix('%') {
                append(&mut result, "%")?;
                value = rest;
                continue;
            }
            let end = value.find('%').ok_or(Error::UndefinedString)?;
            let token = &value[..end];
            if !token.is_empty() && token.bytes().all(|b| b.is_ascii_digit()) {
                // DIRIDs are resolved by the destination environment, not the
                // parser; in filenames they are subsequently rejected.
                append(&mut result, "%")?;
                append(&mut result, token)?;
                append(&mut result, "%")?;
            } else {
                if *budget == 0 {
                    return Err(Error::Limit);
                }
                *budget -= 1;
                if stack.iter().any(|s| s.eq_ignore_ascii_case(token)) {
                    return Err(Error::StringCycle);
                }
                let entry = self
                    .single("Strings", token)?
                    .ok_or(Error::UndefinedString)?;
                let key = entry.key.as_deref().ok_or(Error::Syntax)?;
                let fields = csv(&entry.value)?;
                if fields.len() != 1 {
                    return Err(Error::Syntax);
                }
                stack.push(key);
                append(&mut result, &self.expand_inner(&fields[0], stack, budget)?)?;
                stack.pop();
            }
            value = &value[end + 1..];
        }
        append(&mut result, value)?;
        Ok(result)
    }
}

fn append(to: &mut String, value: &str) -> Result<(), Error> {
    if to.len() + value.len() > MAX_LINE {
        return Err(Error::Limit);
    }
    to.push_str(value);
    Ok(())
}
fn strip_comment(line: &str) -> Result<&str, Error> {
    let mut quote = false;
    for (i, c) in line.char_indices() {
        if c == '"' {
            quote = !quote;
        }
        if c == ';' && !quote {
            return Ok(&line[..i]);
        }
    }
    if quote {
        return Err(Error::Syntax);
    }
    Ok(line)
}
fn unquoted_equal(line: &str) -> Option<usize> {
    let mut quote = false;
    for (i, c) in line.char_indices() {
        if c == '"' {
            quote = !quote;
        }
        if c == '=' && !quote {
            return Some(i);
        }
    }
    None
}
fn csv(value: &str) -> Result<Vec<String>, Error> {
    let mut fields = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    for (i, c) in value.char_indices() {
        if c == '"' {
            quoted = !quoted;
        }
        if c == ',' && !quoted {
            fields.push(unquote(value[start..i].trim())?);
            start = i + 1;
            if fields.len() > 256 {
                return Err(Error::Limit);
            }
        }
    }
    if quoted {
        return Err(Error::Syntax);
    }
    if fields.len() >= 256 {
        return Err(Error::Limit);
    }
    fields.push(unquote(value[start..].trim())?);
    Ok(fields)
}
fn unquote(value: &str) -> Result<String, Error> {
    if let Some(body) = value.strip_prefix('"') {
        let body = body.strip_suffix('"').ok_or(Error::Syntax)?;
        let mut out = String::new();
        let mut chars = body.chars();
        while let Some(c) = chars.next() {
            if c == '"' && chars.next() != Some('"') {
                return Err(Error::Syntax);
            }
            out.push(c);
        }
        Ok(out)
    } else if value.contains('"') {
        Err(Error::Syntax)
    } else {
        Ok(String::from(value))
    }
}
