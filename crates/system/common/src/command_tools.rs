//! Small text utilities shared by the console. All input comes from the mounted VFS.
use alloc::{format, string::String, vec::Vec};

/// Windows-style quoted arguments; a backslash remains a path separator.
pub fn arguments(text: &str) -> Result<Vec<String>, &'static str> {
    let (mut args, mut word, mut quoted, mut started) = (Vec::new(), String::new(), false, false);
    for c in text.chars() {
        if c == '"' { quoted = !quoted; started = true; }
        else if c.is_whitespace() && !quoted { if started { args.push(core::mem::take(&mut word)); started = false; } }
        else { word.push(c); started = true; }
    }
    if quoted { return Err("Unclosed quotation mark."); }
    if started { args.push(word); }
    Ok(args)
}
pub fn path(name: &str, cwd: &[u8]) -> String {
    let cwd = core::str::from_utf8(cwd).unwrap_or("C:\\").trim_end_matches('\0');
    if name.as_bytes().get(1) == Some(&b':') { name.into() }
    else if name.starts_with(['\\', '/']) { format!("{}:{name}", cwd.chars().next().unwrap_or('C')) }
    else { format!("{}\\{name}", cwd.trim_end_matches('\\')) }
}
fn decode(bytes: &[u8]) -> Result<String, &'static str> {
    if bytes.starts_with(&[0xff,0xfe]) || bytes.starts_with(&[0xfe,0xff]) {
        if bytes.len()%2!=0 { return Err("Invalid UTF-16 input."); }
        let le=bytes[0]==0xff;
        let units:Vec<u16>=bytes[2..].chunks_exact(2).map(|b|if le {u16::from_le_bytes([b[0],b[1]])}else{u16::from_be_bytes([b[0],b[1]])}).collect();
        String::from_utf16(&units).map_err(|_|"Invalid UTF-16 input.")
    } else { core::str::from_utf8(bytes.strip_prefix(&[0xef,0xbb,0xbf]).unwrap_or(bytes)).map(String::from).map_err(|_|"Input must be UTF-8 or BOM-marked UTF-16.") }
}
/// Returns (Win32 exit code, output). Read is injected so the identical parser can be tested.
pub fn execute(command:&str, text:&str, mut read:impl FnMut(&str)->Result<Vec<u8>,u32>)->(u32,String) {
    match run(command,text,&mut read) { Ok(result)=>result, Err(e)=>(2,format!("{e}\r\n")) }
}
fn run(command:&str,text:&str,read:&mut impl FnMut(&str)->Result<Vec<u8>,u32>)->Result<(u32,String),String> {
    let args=arguments(text).map_err(String::from)?;
    let (mut files,mut needle,mut ignore,mut inverse,mut numbered,mut count,mut reverse,mut binary)=(Vec::new(),None,false,false,false,false,false,false);
    let mut literal=command=="find";
    for a in &args {
        if a.starts_with('/') {
            match a.to_ascii_lowercase().as_str() {
                "/i" if command=="find"||command=="findstr"=>ignore=true,
                "/v" if command=="find"||command=="findstr"=>inverse=true,
                "/n" if matches!(command,"find"|"findstr"|"fc")=>numbered=true,
                "/c" if command=="find"=>count=true,
                "/c" if command=="fc"=>ignore=true,
                "/r" if command=="sort"=>reverse=true,
                "/b" if command=="fc"=>binary=true,
                "/l" if command=="fc"=>{},
                "/l" if command=="findstr"=>literal=true,
                "/?"=>return Ok((0,format!("{}: files from VFS; quoted paths supported. FIND [/I /V /N /C] text file; FINDSTR /L [/I /V /N] text file; FC [/B /C /N] file1 file2; SORT [/R] file; MORE file.\r\n",command.to_ascii_uppercase()))),
                _=>return Err(format!("Unsupported {} option: {a}",command.to_ascii_uppercase())),
            }
        } else if matches!(command,"find"|"findstr")&&needle.is_none() { needle=Some(a.as_str()); }
        else { files.push(a.as_str()); }
    }
    if files.is_empty() { return Err("Specify an input file. Interactive standard input is not implemented.".into()); }
    if matches!(command,"sort"|"more")&&files.len()!=1 || command=="fc"&&files.len()!=2 {return Err("Incorrect number of file names.".into());}
    let mut inputs=Vec::new();
    for name in &files {inputs.push(read(name).map_err(|s|format!("Cannot read {name} (0x{s:08X})."))?);}
    if command=="fc"&&binary {
        let(a,b)=(&inputs[0],&inputs[1]);let mut out=String::new();
        for i in 0..a.len().max(b.len()) {if a.get(i)!=b.get(i){let left=a.get(i).map(|b|format!("{b:02X}")).unwrap_or_else(||"--".into());let right=b.get(i).map(|b|format!("{b:02X}")).unwrap_or_else(||"--".into());out.push_str(&format!("{i:08X}: {left} {right}\r\n"));}}
        return Ok((if out.is_empty(){0}else{1},out));
    }
    let decoded:Vec<String>=inputs.iter().map(|b|decode(b)).collect::<Result<_,_>>().map_err(String::from)?;
    let mut out=String::new();
    match command {
        "find"|"findstr"=>{
            let needle=needle.ok_or_else(||String::from("Missing search string."))?;
            if command=="findstr"&&!literal&&needle.contains(['.','*','^','$','[',']','\\']) {return Err("FINDSTR regular expressions are not implemented. Use /L for literal search.".into());}
            let needle=if ignore{needle.to_lowercase()}else{needle.into()};let mut matched=false;
            for (name,text) in files.iter().zip(&decoded) {
                if command=="find"{out.push_str(&format!("\r\n---------- {name}\r\n"));}
                let mut matches=0;
                for(i,line)in text.lines().enumerate(){let candidate=if ignore{line.to_lowercase()}else{line.into()};if candidate.contains(&needle)!=inverse{matched=true;matches+=1;if !count{if files.len()>1&&command=="findstr"{out.push_str(&format!("{name}:"));}if numbered{out.push_str(&if command=="find"{format!("[{}]",i+1)}else{format!("{}:",i+1)});}out.push_str(line);out.push_str("\r\n");}}}
                if count{out.push_str(&format!("{matches}\r\n"));}
            }
            Ok((if matched{0}else{1},out))
        },
        "sort"=>{let mut lines:Vec<&str>=decoded[0].lines().collect();lines.sort_by_key(|s|s.to_lowercase());if reverse{lines.reverse();}for line in lines{out.push_str(line);out.push_str("\r\n");}Ok((0,out))},
        "more"=>{for line in decoded[0].lines(){out.push_str(line);out.push_str("\r\n");}Ok((0,out))},
        "fc"=>{let a:Vec<&str>=decoded[0].lines().collect();let b:Vec<&str>=decoded[1].lines().collect();let mut differs=false;
            for i in 0..a.len().max(b.len()){let same=match(a.get(i),b.get(i)){(Some(a),Some(b))=>if ignore{a.to_lowercase()==b.to_lowercase()}else{a==b},_=>false};if !same{differs=true;out.push_str(&format!("***** {}\r\n",files[0]));if numbered{out.push_str(&format!("{}: ",i+1));}out.push_str(a.get(i).copied().unwrap_or("<EOF>"));out.push_str(&format!("\r\n***** {}\r\n",files[1]));if numbered{out.push_str(&format!("{}: ",i+1));}out.push_str(b.get(i).copied().unwrap_or("<EOF>"));out.push_str("\r\n*****\r\n");}}
            if !differs{out.push_str("FC: no differences encountered\r\n");}Ok((if differs{1}else{0},out))},
        _=>Err("Unknown text utility.".into()),
    }
}
