pub const INJECTOR_CLASS_NAME: &str = "ForgeInjector";

pub static INJECTOR_CLASS_DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ForgeInjector.class"));

include!(concat!(env!("OUT_DIR"), "/jar_classes.rs"));
