pub const INJECTOR_CLASS_NAME: &str = "ForgeInjector";

pub static INJECTOR_CLASS_DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ForgeInjector.class"));

pub const JAR_INJECTOR_CLASS_NAME: &str = "JarInjector";

pub static JAR_INJECTOR_CLASS_DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/JarInjector.class"));

#[cfg(feature = "gui")]
pub static AGENT_JAR_DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/jtdl-agent.jar"));

include!(concat!(env!("OUT_DIR"), "/jar_injector_classes.rs"));

include!(concat!(env!("OUT_DIR"), "/jar_classes.rs"));
