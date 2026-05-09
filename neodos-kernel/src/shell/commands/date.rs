use crate::print;
use crate::println;
use crate::shell::shell::DosShell;
use crate::drivers::rtc::Rtc;

impl<'a> DosShell<'a> {
    pub fn cmd_date(&mut self, _args: &[&str]) {
        let mut rtc = Rtc::new();
        let dt = rtc.get_datetime();
        
        // MS-DOS format: dd-mm-yy
        if dt.day < 10 { print!("0{}", dt.day); }
        else { print!("{}", dt.day); }
        print!("/");
        if dt.month < 10 { print!("0{}", dt.month); }
        else { print!("{}", dt.month); }
        print!("/");
        if dt.year < 10 { print!("0{}", dt.year); }
        else { print!("{}", dt.year); }
        
        println!();
    }

    pub fn cmd_time(&mut self, _args: &[&str]) {
        let mut rtc = Rtc::new();
        let dt = rtc.get_datetime();
        
        // MS-DOS format: hh:mm:ss
        if dt.hour < 10 { print!("0{}", dt.hour); }
        else { print!("{}", dt.hour); }
        print!(":");
        if dt.minute < 10 { print!("0{}", dt.minute); }
        else { print!("{}", dt.minute); }
        print!(":");
        if dt.second < 10 { print!("0{}", dt.second); }
        else { print!("{}", dt.second); }
        println!();
    }
}