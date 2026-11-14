//! Standard calculator: decimal input, immediate execution, memory and repeated equals.
use alloc::{format, string::String};
#[derive(Clone, Debug, PartialEq)]
pub struct Calculator {
    pub display: String, accumulator: f64, pending: Option<u8>, repeat: Option<(u8,f64)>,
    fresh: bool, pub memory: f64, pub error: bool,
}
impl Default for Calculator {
    fn default() -> Self { Self { display:"0".into(), accumulator:0.0, pending:None, repeat:None, fresh:true, memory:0.0, error:false } }
}
impl Calculator {
    fn value(&self) -> f64 { self.display.parse().unwrap_or(0.0) }
    fn show(&mut self, value:f64) {
        if !value.is_finite() { self.display="Cannot divide by zero / overflow".into(); self.error=true; }
        else { self.display=format!("{value:.10}"); while self.display.ends_with('0') { self.display.pop(); } if self.display.ends_with('.') { self.display.pop(); } if self.display=="-0" { self.display="0".into(); } }
    }
    fn apply(&mut self, op:u8, rhs:f64) { let v=match op { b'+'=>self.accumulator+rhs,b'-'=>self.accumulator-rhs,b'*'=>self.accumulator*rhs,b'/'=>self.accumulator/rhs,_=>rhs }; self.show(v); self.accumulator=v; }
    pub fn press(&mut self, key:u8) {
        if matches!(key,b'c'|b'C'|27) { *self=Self {memory:self.memory,..Self::default()}; return; }
        if self.error { return; }
        match key {
            b'0'..=b'9'|b'.' => {
                if self.fresh { self.display="0".into(); self.fresh=false; }
                if key==b'.' { if !self.display.contains('.') { self.display.push('.'); } }
                else if self.display.len()<16 { if self.display=="0" { self.display.clear(); } self.display.push(key as char); }
            }
            8 => { if !self.fresh { self.display.pop(); if self.display.is_empty()||self.display=="-" { self.display="0".into(); } } }
            b'+'|b'-'|b'*'|b'/' => { if !self.fresh { if let Some(op)=self.pending { self.apply(op,self.value()); } else { self.accumulator=self.value(); } } else if self.pending.is_none() { self.accumulator=self.value(); } self.pending=Some(key);self.fresh=true;self.repeat=None; }
            b'='|13|10 => { if let Some(op)=self.pending.take() { let rhs=self.value();self.apply(op,rhs);self.repeat=Some((op,rhs)); } else if let Some((op,rhs))=self.repeat {self.accumulator=self.value();self.apply(op,rhs);} self.fresh=true; }
            b'%' => self.show(self.accumulator*self.value()/100.0),
            b'n' => self.show(-self.value()),
            b'r' => self.show(1.0/self.value()),
            b'm' => self.memory=self.value(), b'a'=>self.memory+=self.value(),
            b'p' => {self.show(self.memory);self.fresh=true;}, b'x'=>self.memory=0.0,
            b'e'=>{self.display="0".into();self.fresh=true;},
            _=>{}
        }
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn arithmetic_repeated_equals_and_memory() {
        let mut c=Calculator::default();for k in b"12.5+7.5==" {c.press(*k);} assert_eq!(c.display,"27.5");
        c.press(b'm');c.press(b'c');c.press(b'p');assert_eq!(c.display,"27.5");
        for k in b"c8/0=" {c.press(*k);}assert!(c.error);c.press(b'c');assert_eq!(c.display,"0");
    }
    #[test] fn operator_replacement_and_decimal_editing() { let mut c=Calculator::default(); for k in b"5+-2=" { c.press(*k); } assert_eq!(c.display,"3");for k in b"c1..2" {c.press(*k);}assert_eq!(c.display,"1.2"); }
}
