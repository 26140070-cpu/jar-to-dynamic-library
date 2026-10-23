#[derive(Debug)]
pub struct InjectorError {
    pub message: &'static str,
    pub os_error: u32,
}

impl InjectorError {
    pub fn new(message: &'static str) -> Self {
        let os_error = std::io::Error::last_os_error().raw_os_error().unwrap_or(0) as u32;
        Self { message, os_error }
    }
}

pub type Result<T> = core::result::Result<T, InjectorError>;
