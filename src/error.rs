use std::error;

pub type Error = Box<dyn error::Error + Sync + Send>;
