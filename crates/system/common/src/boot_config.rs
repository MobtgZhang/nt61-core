//! BCDEdit's store model. Parses and rewrites actual REGF keys, never synthesized entries.
use alloc::{format,string::String,vec::Vec};
use crate::registry::{hive::{Hive,KeyNode,ValueType},hive_writer::{HiveKey,HiveValue,HiveWriter}};
pub const BOOTMGR:&str="{9dea862c-5cdd-4e70-acc1-f32b344d4795}";
pub struct Store{root:HiveKey}
fn copy_key(h:&Hive<'_>,n:&KeyNode,depth:usize)->Result<HiveKey,&'static str>{
    if depth>32{return Err("BCD tree nesting is too deep.");}
    let mut out=HiveKey::new(&n.name);out.flags=n.flags;
    for v in h.values(n).map_err(|_|"Invalid BCD values.")?{out.values.push(HiveValue{name:v.name,value_type:v.value_type,data:v.data});}
    for child in h.subkeys(n).map_err(|_|"Invalid BCD subkeys.")?{out.subkeys.push(copy_key(h,&child,depth+1)?);}
    Ok(out)
}
fn child<'a>(key:&'a HiveKey,name:&str)->Option<&'a HiveKey>{key.subkeys.iter().find(|k|k.name.eq_ignore_ascii_case(name))}
fn child_mut<'a>(key:&'a mut HiveKey,name:&str)->Option<&'a mut HiveKey>{key.subkeys.iter_mut().find(|k|k.name.eq_ignore_ascii_case(name))}
fn wide(text:&str)->Vec<u8>{text.encode_utf16().chain(core::iter::once(0)).flat_map(u16::to_le_bytes).collect()}
fn string(data:&[u8])->Option<String>{if data.len()%2!=0{return None;}String::from_utf16(&data.chunks_exact(2).map(|s|u16::from_le_bytes([s[0],s[1]])).take_while(|u|*u!=0).collect::<Vec<_>>()).ok()}
impl Store{
    pub fn parse(bytes:&[u8])->Result<Self,&'static str>{
        if bytes.len()>4*1024*1024{return Err("BCD store is too large.");}
        let hive=Hive::parse(bytes).map_err(|_|"The boot configuration data store is invalid.")?;
        let root=copy_key(&hive,&hive.root().map_err(|_|"Missing BCD root.")?,0)?;
        let store=Self{root};if store.objects()?.subkeys.is_empty(){return Err("BCD has no boot objects.");}Ok(store)
    }
    fn objects(&self)->Result<&HiveKey,&'static str>{
        let root=child(&self.root,"NewStoreRoot").unwrap_or(&self.root);
        child(root,"Objects").ok_or("The store contains no BCD Objects key.")
    }
    fn objects_mut(&mut self)->Result<&mut HiveKey,&'static str>{
        let root=if self.root.subkeys.iter().any(|k|k.name.eq_ignore_ascii_case("NewStoreRoot")){child_mut(&mut self.root,"NewStoreRoot").unwrap()}else{&mut self.root};
        child_mut(root,"Objects").ok_or("The store contains no BCD Objects key.")
    }
    fn resolve(&self,id:&str)->Result<String,&'static str>{
        let name:String=if id.eq_ignore_ascii_case("{bootmgr}"){BOOTMGR.into()}
        else if id.eq_ignore_ascii_case("{default}")||id.eq_ignore_ascii_case("{current}"){
            return Err("This store does not record a resolvable current/default alias. Use an explicit identifier from /enum all.");
        }else{id.into()};
        self.objects()?.subkeys.iter().find(|k|k.name.eq_ignore_ascii_case(&name)).map(|k|k.name.clone()).ok_or("The specified boot entry does not exist.")
    }
    fn elements_mut(&mut self,id:&str)->Result<&mut HiveKey,&'static str>{let id=self.resolve(id)?;let o=child_mut(self.objects_mut()?,&id).ok_or("Entry not found.")?;child_mut(o,"Elements").ok_or("Missing Elements key.")}
    pub fn set(&mut self,id:&str,field:&str,value:&str)->Result<(),&'static str>{
        let(kind,value_type,data)=match field.to_ascii_lowercase().as_str(){
            "timeout"=>{if !id.eq_ignore_ascii_case("{bootmgr}")&&!id.eq_ignore_ascii_case(BOOTMGR){return Err("Timeout belongs to {bootmgr}.");}let n=value.parse::<u32>().map_err(|_|"Invalid timeout.")?;("15000011",ValueType::DWord,n.to_le_bytes().to_vec())},
            "description"=>("12000004",ValueType::String,wide(value)),
            "locale"=>("12000005",ValueType::String,wide(value)),
            "path"=>{if !value.starts_with('\\'){return Err("The application path must be absolute.");}("12000002",ValueType::String,wide(value))},
            "systemroot"=>{if !value.starts_with('\\'){return Err("Systemroot must be absolute.");}("22000002",ValueType::String,wide(value))},
            "loadoptions"=>("22000001",ValueType::String,wide(value)),
            _=>return Err("This BCD element is not supported by the boot loader."),
        };
        if data.len()>2048||value.contains('\0'){return Err("BCD element is too long or contains NUL.");}
        let elements=self.elements_mut(id)?;
        if child(elements,kind).is_none(){elements.subkeys.push(HiveKey::new(kind));}
        let element=child_mut(elements,kind).unwrap();
        element.values.retain(|v|!v.name.eq_ignore_ascii_case("Element"));element.values.push(HiveValue{name:"Element".into(),value_type,data});Ok(())
    }
    pub fn enumerate(&self,id:Option<&str>)->Result<String,&'static str>{
        let selected=id.filter(|id|!id.eq_ignore_ascii_case("all")).map(|id|self.resolve(id)).transpose()?;
        let mut out=String::new();
        for object in &self.objects()?.subkeys{
            if selected.as_ref().is_some_and(|id|!id.eq_ignore_ascii_case(&object.name)){continue;}
            out.push_str(if object.name.eq_ignore_ascii_case(BOOTMGR){"Windows Boot Manager\r\n--------------------\r\n"}else{"Windows Boot Loader\r\n-------------------\r\n"});
            out.push_str(&format!("identifier              {}\r\n",object.name));
            if let Some(elements)=child(object,"Elements"){for e in &elements.subkeys{if let Some(v)=e.values.iter().find(|v|v.name.eq_ignore_ascii_case("Element")){
                let label=match e.name.as_str(){"12000004"=>"description","12000002"=>"path","12000005"=>"locale","22000002"=>"systemroot","22000001"=>"loadoptions","15000011"=>"timeout",_=>&e.name};
                let value=match v.value_type{ValueType::String|ValueType::ExpandString=>string(&v.data).unwrap_or_else(||"<invalid string>".into()),ValueType::DWord if v.data.len()==4=>format!("{}",u32::from_le_bytes(v.data[..4].try_into().unwrap())),ValueType::MultiString=>String::from_utf16_lossy(&v.data.chunks_exact(2).map(|c|u16::from_le_bytes([c[0],c[1]])).collect::<Vec<_>>()).replace('\0'," "),_=>v.data.iter().map(|b|format!("{b:02X}")).collect()};
                out.push_str(&format!("{label:<24}{value}\r\n"));
            }}}
            out.push_str("\r\n");
        }Ok(out)
    }
    pub fn bytes(&self)->Result<Vec<u8>,&'static str>{HiveWriter::new().serialise(&self.root,0)}
}
